//! Extension worker credentials entered once in Relay, kept in a private file in Relay's own
//! support folder (mode 0600), and seeded into the extension inside each Chrome Relay opens.
//! The token is never returned to the page after saving and never written to browser storage.
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Only kinds with a per-account binding can be seeded; audio has none, so it is never offered.
pub(crate) const KINDS: [&str; 3] = ["image", "video", "audio"];
const FOLDER_KINDS: [&str; 3] = ["image", "video", "audio"];
const FILE_VERSION: u8 = 1;

static STORE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
pub(crate) fn init_store(path: PathBuf) {
    let _ = STORE.set(path);
}
fn store_path() -> Result<&'static PathBuf, String> {
    STORE.get().ok_or_else(|| "Relay's worker credential file is unavailable.".to_string())
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct Stored {
    worker_id: String,
    token: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialFile {
    version: u8,
    workers: BTreeMap<String, Stored>,
}

impl CredentialFile {
    fn empty() -> Self {
        Self { version: FILE_VERSION, workers: BTreeMap::new() }
    }
}

#[derive(Serialize)]
pub(crate) struct Status {
    kind: &'static str,
    worker_id: Option<String>,
    has_token: bool,
    /// True when the file exists but cannot be read or parsed; the token must be saved again.
    unreadable: bool,
}

#[derive(Debug)]
enum ReadOutcome {
    Found(Stored),
    Missing,
    Unreadable,
}

fn label(kind: &str) -> &'static str {
    match kind {
        "image" => "Image",
        "video" => "Video",
        _ => "Audio",
    }
}
fn load_file(path: &Path) -> Result<CredentialFile, ReadOutcome> {
    match crate::storage::read_private(path) {
        Ok(bytes) => {
            let file: CredentialFile =
                serde_json::from_slice(&bytes).map_err(|_| ReadOutcome::Unreadable)?;
            (file.version == FILE_VERSION).then_some(file).ok_or(ReadOutcome::Unreadable)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CredentialFile::empty()),
        Err(_) => Err(ReadOutcome::Unreadable),
    }
}

fn save_file(path: &Path, file: &CredentialFile) -> Result<(), String> {
    let bytes = serde_json::to_vec(file)
        .map_err(|_| "Could not encode the worker credentials.".to_string())?;
    crate::storage::write_private(path, &bytes)
        .map_err(|_| "Relay could not save the worker token in its data folder.".to_string())
}

fn valid(stored: &Stored) -> bool {
    !stored.worker_id.is_empty() && !stored.token.is_empty() && stored.worker_id.len() <= 160
}

fn read(kind: &str) -> ReadOutcome {
    let Ok(path) = store_path() else {
        return ReadOutcome::Unreadable;
    };
    match load_file(path) {
        Ok(file) => match file.workers.get(kind) {
            Some(stored) if valid(stored) => ReadOutcome::Found(stored.clone()),
            Some(_) => ReadOutcome::Unreadable,
            None => ReadOutcome::Missing,
        },
        Err(outcome) => outcome,
    }
}

fn write(kind: &str, worker_id: &str, token: &str) -> Result<(), String> {
    let path = store_path()?;
    let mut file = match load_file(path) {
        Ok(file) => file,
        Err(_) => CredentialFile::empty(),
    };
    file.workers.insert(kind.to_string(), Stored { worker_id: worker_id.to_string(), token: token.to_string() });
    save_file(path, &file)
}

fn remove(kind: &str) -> Result<(), String> {
    let path = store_path()?;
    let mut file = match load_file(path) {
        Ok(file) => file,
        Err(_) => CredentialFile::empty(),
    };
    file.workers.remove(kind);
    save_file(path, &file)
}

pub(crate) fn status_all() -> Vec<Status> {
    KINDS
        .iter()
        .map(|kind| match read(kind) {
            ReadOutcome::Found(stored) => Status {
                kind,
                worker_id: Some(stored.worker_id),
                has_token: true,
                unreadable: false,
            },
            ReadOutcome::Missing => Status { kind, worker_id: None, has_token: false, unreadable: false },
            ReadOutcome::Unreadable => Status { kind, worker_id: None, has_token: false, unreadable: true },
        })
        .collect()
}

/// The stored credential for a kind, for the pairing check: (worker id, token).
pub(crate) fn stored(kind: &str) -> Option<(String, String)> {
    match read(kind) {
        ReadOutcome::Found(entry) => Some((entry.worker_id, entry.token)),
        _ => None,
    }
}

/// Stores a token minted through the local pairing file.
pub(crate) fn store_credential(kind: &str, worker_id: &str, token: &str) -> Result<(), String> {
    valid_kind(kind)?;
    write(kind, worker_id, token)
}

fn valid_kind(kind: &str) -> Result<(), String> {
    KINDS.contains(&kind).then_some(()).ok_or_else(|| "Unknown extension kind.".to_string())
}

pub(crate) fn save(kind: &str, worker_id: &str, token: &str) -> Result<(), String> {
    valid_kind(kind)?;
    let worker_id = worker_id.trim();
    let token = token.trim();
    if worker_id.is_empty() || worker_id.len() > 160 {
        return Err("Enter the worker id Gateway showed with this token.".into());
    }
    if token.is_empty() {
        return Err("Enter the worker token.".into());
    }
    write(kind, worker_id, token)
}

pub(crate) fn clear(kind: &str) -> Result<(), String> {
    valid_kind(kind)?;
    remove(kind)
}

/// The storage write for one extension. Each extension keeps its own shape; never invent fields.
fn expression(kind: &str, stored: &Stored, origin: &str) -> Option<String> {
    let worker_id = serde_json::to_string(&stored.worker_id).ok()?;
    let token = serde_json::to_string(&stored.token).ok()?;
    let origin = serde_json::to_string(origin).ok()?;
    let worker_id_value = worker_id.clone();
    match kind {
        "image" => {
            let connection = format!(
                "{{ gatewayModeEnabled: true, gatewayUrl: {origin}, gatewayToken: {token}, gatewayWorkerId: {worker_id_value} }}"
            );
            Some(format!(
                "(async () => {{ await chrome.storage.local.set({{ gatewayConnection: {connection}, imageGeneratorWorkMode: 'gateway' }}); return true; }})()"
            ))
        }
        "video" => Some(format!(
            "(async () => {{ const key = 'settings'; const got = await chrome.storage.sync.get(key); const settings = Object.assign({{}}, got[key] || {{}}, {{ mode: 'gateway', gatewayUrl: {origin}, gatewayWorkerId: {worker_id_value} }}); await chrome.storage.sync.set({{ [key]: settings }}); await chrome.storage.local.set({{ gatewayWorkerToken: {token} }}); return true; }})()"
        )),
        "audio" => Some(format!(
            "(async () => {{ const key = 'settings'; const got = await chrome.storage.sync.get(key); const settings = Object.assign({{}}, got[key] || {{}}, {{ mode: 'gateway' }}); await chrome.storage.sync.set({{ [key]: settings }}); await chrome.storage.local.set({{ gatewayModeEnabled: true, gatewayBaseUrl: {origin}, gatewayWorkerId: {worker_id_value}, gatewayWorkerToken: {token} }}); return true; }})()"
        )),
        _ => None,
    }
}

/// Seeds one loaded extension for this profile. Returns the honest line shown on the profile row.
pub(crate) fn seed_extension(devtools: &mut super::devtools::Devtools, kind: &str, extension_id: &str, bound_worker_id: Option<&str>, origin: &str) -> String {
    let Some(bound) = bound_worker_id.filter(|value| !value.is_empty()) else {
        return format!("{}: not seeded — no worker id is bound to this account", label(kind));
    };
    let stored = match read(kind) {
        ReadOutcome::Found(value) => value,
        ReadOutcome::Missing => return format!("{}: not seeded — no worker token saved in Relay", label(kind)),
        ReadOutcome::Unreadable => return format!("{}: not seeded — its saved worker token could not be read; save it again", label(kind)),
    };
    if stored.worker_id != bound {
        return format!("{}: not seeded — the saved token belongs to worker {}, not {}", label(kind), stored.worker_id, bound);
    }
    let Some(expression) = expression(kind, &stored, origin) else {
        return format!("{}: not seeded — unsupported kind", label(kind));
    };
    match devtools.eval_in_extension(extension_id, &expression) {
        Ok(_) => format!("{}: seeded as {}", label(kind), bound),
        Err(error) => format!("{}: not seeded — {error}", label(kind)),
    }
}

#[tauri::command]
pub(crate) async fn extension_worker_status(window: tauri::WebviewWindow) -> Result<Vec<Status>, String> {
    super::local_only(&window)?;
    Ok(status_all())
}

#[tauri::command]
pub(crate) async fn save_extension_worker(
    window: tauri::WebviewWindow,
    kind: String,
    worker_id: String,
    token: String,
) -> Result<Vec<Status>, String> {
    super::local_only(&window)?;
    save(&kind, &worker_id, &token)?;
    Ok(status_all())
}

#[tauri::command]
pub(crate) async fn clear_extension_worker(window: tauri::WebviewWindow, kind: String) -> Result<Vec<Status>, String> {
    super::local_only(&window)?;
    clear(&kind)?;
    Ok(status_all())
}

/// True when the folder belongs to the kind (used to match Chrome's loaded ids to kinds).
pub(crate) fn kind_for_dir(directory: &Path) -> Option<&'static str> {
    let parent = directory.parent()?.file_name()?.to_str()?;
    FOLDER_KINDS.iter().copied().find(|kind| *kind == parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("relay-seed-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("extension-workers.json")
    }

    #[test]
    fn credential_file_round_trips_and_never_parses_garbage() {
        let path = temp_path("roundtrip");
        let mut file = CredentialFile::empty();
        file.workers.insert("image".into(), Stored { worker_id: "local-image-1".into(), token: "ekw_secret".into() });
        save_file(&path, &file).unwrap();
        let loaded = load_file(&path).unwrap();
        assert_eq!(loaded.workers.get("image").unwrap().worker_id, "local-image-1");
        std::fs::write(&path, b"not json").unwrap();
        assert!(matches!(load_file(&path), Err(ReadOutcome::Unreadable)));
        let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions());
        assert_eq!(mode & 0o777, 0o600, "credential files are readable only by this user");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn private_writes_are_0600_and_atomic() {
        let path = temp_path("private");
        crate::storage::write_private(&path, b"secret").unwrap();
        let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions());
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(crate::storage::read_private(&path).unwrap(), b"secret");
        assert!(!path.with_extension("pending").exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
