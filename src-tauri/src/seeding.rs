//! Extension worker credentials entered once in Relay, kept in the Keychain, and seeded into the
//! extension inside each Chrome Relay opens. The token is never returned to the page after saving
//! and never written to browser storage by Relay itself.
use serde::Serialize;
use std::path::Path;

// Only kinds with a per-account binding can be seeded; audio has none, so it is never offered.
pub(crate) const KINDS: [&str; 2] = ["image", "video"];
const FOLDER_KINDS: [&str; 3] = ["image", "video", "audio"];
const KEYCHAIN_SERVICE: &str = "kh.relay.desktop.extension-worker";
const CREDENTIAL_VERSION: u8 = 1;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct Stored {
    version: u8,
    worker_id: String,
    token: String,
}

#[derive(Serialize)]
pub(crate) struct Status {
    kind: &'static str,
    worker_id: Option<String>,
    has_token: bool,
    /// True when macOS refused the read (a prompt was declined, or interaction is blocked).
    denied: bool,
}

enum ReadOutcome {
    Found(Stored),
    Missing,
    Denied,
}

fn label(kind: &str) -> &'static str {
    match kind {
        "image" => "Image",
        "video" => "Video",
        _ => "Audio",
    }
}
fn account(kind: &str) -> String {
    format!("worker-{kind}")
}

#[cfg(target_os = "macos")]
fn read(kind: &str) -> ReadOutcome {
    match security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, &account(kind)) {
        Ok(bytes) => match serde_json::from_slice::<Stored>(&bytes) {
            Ok(stored)
                if stored.version == CREDENTIAL_VERSION
                    && !stored.worker_id.is_empty()
                    && !stored.token.is_empty()
                    && stored.worker_id.len() <= 160 =>
            {
                ReadOutcome::Found(stored)
            }
            _ => ReadOutcome::Missing,
        },
        // errSecItemNotFound: nothing saved.
        Err(error) if error.code() == -25300 => ReadOutcome::Missing,
        // Anything else is a refusal (declined prompt, interaction not allowed, auth failure,
        // timeout). Say so instead of pretending no token exists.
        Err(_) => ReadOutcome::Denied,
    }
}
#[cfg(not(target_os = "macos"))]
fn read(_: &str) -> ReadOutcome {
    ReadOutcome::Missing
}

#[cfg(target_os = "macos")]
fn write(kind: &str, worker_id: &str, token: &str) -> Result<(), String> {
    let stored = Stored { version: CREDENTIAL_VERSION, worker_id: worker_id.to_string(), token: token.to_string() };
    let bytes = serde_json::to_vec(&stored).map_err(|_| "Could not encode the worker credential.")?;
    // Delete + add re-mints the item's ACL for THIS build; security_framework's update would keep
    // an older ACL that a rebuild may no longer satisfy.
    let _ = security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, &account(kind));
    security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, &account(kind), &bytes)
        .map_err(|error| format!("Relay could not save this credential in the Keychain (status {}).", error.code()))
}
#[cfg(not(target_os = "macos"))]
fn write(_: &str, _: &str, _: &str) -> Result<(), String> {
    Err("Extension worker credentials are available only on macOS.".into())
}

#[cfg(target_os = "macos")]
fn remove(kind: &str) -> Result<(), String> {
    match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, &account(kind)) {
        Ok(()) => Ok(()),
        // Deleting only our own item; an absent item is already the goal.
        Err(error) if error.code() == -25300 => Ok(()),
        Err(error) => Err(format!("Relay could not remove this credential (status {}).", error.code())),
    }
}
#[cfg(not(target_os = "macos"))]
fn remove(_: &str) -> Result<(), String> {
    Err("Extension worker credentials are available only on macOS.".into())
}

pub(crate) fn status_all() -> Vec<Status> {
    KINDS
        .iter()
        .map(|kind| match read(kind) {
            ReadOutcome::Found(stored) => {
                // Self-heal: a token that could be read is rewritten once per launch so its ACL
                // belongs to this build (macOS may have asked once after an update).
                if !refreshed(kind) {
                    let _ = write(kind, &stored.worker_id, &stored.token);
                }
                Status { kind, worker_id: Some(stored.worker_id), has_token: true, denied: false }
            }
            ReadOutcome::Missing => Status { kind, worker_id: None, has_token: false, denied: false },
            ReadOutcome::Denied => Status { kind, worker_id: None, has_token: false, denied: true },
        })
        .collect()
}

fn refreshed(kind: &str) -> bool {
    use std::sync::{Mutex, OnceLock};
    static REFRESHED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let set = REFRESHED.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    let mut guard = set.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    !guard.insert(kind.to_string())
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
        ReadOutcome::Denied => return format!("{}: not seeded — Keychain access was denied; allow Relay and retry", label(kind)),
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
// Async so a slow or prompting Keychain read runs off the main thread and can never freeze the app.
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
