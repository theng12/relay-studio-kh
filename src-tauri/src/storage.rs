//! Private app-data files for credentials. Plain values in Relay's own support folder with mode
//! 0600, written atomically; no Keychain, so no system prompt can ever appear.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let pending = path.with_extension("pending");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    let mut file = options.open(&pending)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&pending, path)
}

pub(crate) fn read_private(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path)
}

pub(crate) fn remove_private(path: &Path) {
    let _ = fs::remove_file(path);
}
