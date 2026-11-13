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

    /// Loads each directory and returns one honest line per failure plus the Chrome extension id
    /// for each loaded folder; Chrome stays open regardless.
    pub(crate) fn load_unpacked(&mut self, directories: &[PathBuf]) -> (Vec<String>, Vec<(PathBuf, String)>) {
        let mut problems = Vec::new();
        let mut loaded = Vec::new();
        for directory in directories {
            let reply = self.call(
                "Extensions.loadUnpacked",
                serde_json::json!({ "path": directory.to_string_lossy() }),
                None,
                Duration::from_secs(10),
            );
            match reply {
                Ok(value) => {
                    if let Some(error) = value.get("error").and_then(|error| error.get("message")).and_then(Value::as_str) {
                        problems.push(format!("{}: {error}", directory.display()));
                    } else if let Some(id) = value.get("result").and_then(|result| result.get("id")).and_then(Value::as_str) {
                        loaded.push((directory.clone(), id.to_string()));
                    } else {
                        problems.push(format!("{}: Chrome returned no extension id", directory.display()));
                    }
                }
                Err(error) => {
                    problems.push(error);
                    break;
                }
            }
        }
        (problems, loaded)
    }

    /// Runs an expression inside one loaded extension's service worker, which is the only
    /// context allowed to write its storage while its own pages are unavailable.
    pub(crate) fn eval_in_extension(&mut self, extension_id: &str, expression: &str) -> Result<Value, String> {
        let targets = self.call("Target.getTargets", serde_json::json!({}), None, Duration::from_secs(5))?;
        let prefix = format!("chrome-extension://{extension_id}/");
        let target_id = targets
            .get("result")
            .and_then(|result| result.get("targetInfos"))
            .and_then(Value::as_array)
            .and_then(|infos| {
                infos.iter().find_map(|info| {
                    let kind = info.get("type").and_then(Value::as_str);
                    let url = info.get("url").and_then(Value::as_str).unwrap_or("");
                    (kind == Some("service_worker") && url.starts_with(&prefix))
                        .then(|| info.get("targetId").and_then(Value::as_str).map(str::to_string))
                        .flatten()
                })
            })
            .ok_or("The extension's worker is not running in this browser.")?;
        let attached = self.call(
            "Target.attachToTarget",
            serde_json::json!({ "targetId": target_id, "flatten": true }),
            None,
            Duration::from_secs(5),
        )?;
        let session = attached
            .get("result")
            .and_then(|result| result.get("sessionId"))
            .and_then(Value::as_str)
            .ok_or("Chrome refused to attach to the extension's worker.")?
            .to_string();
        let reply = self.call(
            "Runtime.evaluate",
            serde_json::json!({ "expression": expression, "awaitPromise": true, "returnByValue": true }),
            Some(&session),
            Duration::from_secs(10),
        )?;
        if let Some(exception) = reply.get("result").and_then(|result| result.get("exceptionDetails")) {
            return Err(format!("The extension rejected its settings: {}", exception.get("text").and_then(Value::as_str).unwrap_or("unknown error")));
        }
        Ok(evaluated_value(&reply))
    }

    fn call(&mut self, method: &str, params: Value, session: Option<&str>, timeout: Duration) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut message = serde_json::json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            message["sessionId"] = Value::String(session.to_string());
        }
        let sent = self
            .to_chrome
            .write_all(message.to_string().as_bytes())
            .and_then(|_| self.to_chrome.write_all(b"\0"))
            .is_ok();
        if !sent {
            return Err("Chrome's control pipe closed before it could answer.".into());
        }
        self.wait_reply(id, timeout)
    }

    /// Chrome's storage API for an extension, without waking or evaluating its worker.
    pub(crate) fn get_storage_items(&mut self, extension_id: &str, area: &str, keys: Option<Vec<String>>) -> Result<Value, String> {
        let mut params = serde_json::json!({ "id": extension_id, "storageArea": area });
        if let Some(keys) = keys {
            params["keys"] = serde_json::json!(keys);
        }
        let reply = self.call("Extensions.getStorageItems", params, None, Duration::from_secs(5))?;
        if let Some(error) = reply.get("error").and_then(|error| error.get("message")).and_then(Value::as_str) {
            return Err(error.to_string());
        }
        Ok(reply.pointer("/result/data").cloned().unwrap_or(Value::Null))
    }

    pub(crate) fn set_storage_items(&mut self, extension_id: &str, area: &str, values: Value) -> Result<(), String> {
        let reply = self.call(
            "Extensions.setStorageItems",
            serde_json::json!({ "id": extension_id, "storageArea": area, "values": values }),
            None,
            Duration::from_secs(5),
        )?;
        if let Some(error) = reply.get("error").and_then(|error| error.get("message")).and_then(Value::as_str) {
            return Err(error.to_string());
        }
        Ok(())
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

/// Runtime.evaluate nests the value twice: reply.result is the CDP command result, and
/// reply.result.result.value is the value the expression returned.
fn evaluated_value(reply: &Value) -> Value {
    reply
        .pointer("/result/result/value")
        .cloned()
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::evaluated_value;
    use serde_json::json;

    #[test]
    fn the_evaluated_value_is_read_from_the_nested_cdp_shape() {
        let reply = json!({
            "id": 7,
            "result": {
                "result": {
                    "type": "object",
                    "value": { "mode": true, "url": true, "worker": true, "token": true }
                }
            }
        });
        let value = evaluated_value(&reply);
        assert_eq!(value.get("token").and_then(serde_json::Value::as_bool), Some(true));
    }

    #[test]
    fn a_missing_value_is_null_not_an_outer_object() {
        let reply = json!({ "id": 7, "result": { "result": { "type": "undefined" } } });
        assert!(evaluated_value(&reply).is_null());
    }
}
