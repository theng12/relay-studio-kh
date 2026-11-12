//! Loads Relay's unpacked extensions into the Chrome it starts. Branded Chrome 137+ ignores
//! `--load-extension`; the supported route is `Extensions.loadUnpacked` over
//! `--remote-debugging-pipe`. The pipe is session-scoped and closing it ends Chrome, so the
//! spawn lease keeps both ends alive until the process exits.
use serde_json::Value;
use std::io::{pipe, PipeReader, PipeWriter, Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub(crate) struct Devtools {
    to_chrome: PipeWriter,
    from_chrome: PipeReader,
    // Parent copies of the child's ends; dropped once Chrome has spawned.
    child_ends: Option<(PipeReader, PipeWriter)>,
    pending: Vec<u8>,
    next_id: u64,
}

/// Adds Chrome's control pipe to `command`; call `spawned()` right after `spawn()` returns.
pub(crate) fn attach(command: &mut Command) -> Result<Devtools, String> {
    let (to_chrome_read, to_chrome_write) =
        pipe().map_err(|_| "Could not create Relay's Chrome control pipe.")?;
    let (from_chrome_read, from_chrome_write) =
        pipe().map_err(|_| "Could not create Relay's Chrome control pipe.")?;
    let read_fd = to_chrome_read.as_fd().as_raw_fd();
    let write_fd = from_chrome_write.as_fd().as_raw_fd();
    // Chrome's remote-debugging-pipe contract: commands on fd 3, responses on fd 4.
    command
        .arg("--enable-unsafe-extension-debugging")
        .arg("--remote-debugging-pipe");
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(read_fd, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::dup2(write_fd, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(Devtools {
        to_chrome: to_chrome_write,
        from_chrome: from_chrome_read,
        child_ends: Some((to_chrome_read, from_chrome_write)),
        pending: Vec::new(),
        next_id: 0,
    })
}

impl Devtools {
    pub(crate) fn spawned(&mut self) {
        self.child_ends = None;
    }

    /// Loads each directory and returns one honest line per failure; Chrome stays open regardless.
    pub(crate) fn load_unpacked(&mut self, directories: &[PathBuf]) -> Vec<String> {
        let mut problems = Vec::new();
        for directory in directories {
            self.next_id += 1;
            let id = self.next_id;
            let message = serde_json::json!({
                "id": id,
                "method": "Extensions.loadUnpacked",
                "params": { "path": directory.to_string_lossy() }
            });
            let sent = self
                .to_chrome
                .write_all(message.to_string().as_bytes())
                .and_then(|_| self.to_chrome.write_all(b"\0"))
                .is_ok();
            if !sent {
                problems.push("Chrome's control pipe closed before the extensions could load.".into());
                break;
            }
            match self.wait_reply(id, Duration::from_secs(10)) {
                Ok(value) => {
                    if let Some(error) = value
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                    {
                        problems.push(format!("{}: {error}", directory.display()));
                    }
                }
                Err(error) => {
                    problems.push(error);
                    break;
                }
            }
        }
        problems
    }

    /// Chrome's own registry for this window: what is loaded, with versions.
    pub(crate) fn extensions(&mut self) -> Result<Vec<Value>, String> {
        self.next_id += 1;
        let id = self.next_id;
        let message = serde_json::json!({ "id": id, "method": "Extensions.getExtensions" });
        let sent = self
            .to_chrome
            .write_all(message.to_string().as_bytes())
            .and_then(|_| self.to_chrome.write_all(b"\0"))
            .is_ok();
        if !sent {
            return Err("Chrome's control pipe closed before it could answer.".into());
        }
        let reply = self.wait_reply(id, Duration::from_secs(5))?;
        if let Some(error) = reply
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
        {
            return Err(error.to_string());
        }
        Ok(reply
            .get("result")
            .and_then(|result| result.get("extensions"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    fn wait_reply(&mut self, id: u64, timeout: Duration) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        loop {
            while let Some(end) = self.pending.iter().position(|byte| *byte == 0) {
                let message: Vec<u8> = self.pending.drain(..=end).collect();
                let text = String::from_utf8_lossy(&message[..message.len() - 1]);
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if value.get("id").and_then(Value::as_u64) == Some(id) {
                    return Ok(value);
                }
                // Events carry no id; keep reading for the reply.
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Chrome did not answer its extension control pipe.".into());
            }
            let fd = self.from_chrome.as_fd().as_raw_fd();
            let mut descriptor = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let millis = remaining.as_millis().min(500) as i32;
            let ready = unsafe { libc::poll(&mut descriptor, 1, millis) };
            if ready < 0 {
                return Err("Chrome's extension control pipe failed.".into());
            }
            if ready == 0 {
                continue;
            }
            let mut buffer = [0u8; 8192];
            match self.from_chrome.read(&mut buffer) {
                Ok(0) => return Err("Chrome closed its extension control pipe.".into()),
                Ok(read) => self.pending.extend_from_slice(&buffer[..read]),
                Err(_) => return Err("Chrome's extension control pipe failed.".into()),
            }
        }
    }
}
