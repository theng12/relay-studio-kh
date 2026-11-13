//! Automatic pairing with the Gateway on this Mac. The Gateway writes
//! `~/.kh/gateway-endpoint.json` (0600) with its URL, a machine id and a pairing token that can do
//! exactly one thing: mint a worker token over loopback. Relay reads it at launch, points itself at
//! that Gateway, and mints a worker token per kind when its own file has none. Manual entry remains
//! the fallback when the file is absent.
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

const SCHEMA: u8 = 1;
const MINT_PATH: &str = "/admin/api/extension-workers/pair";
const HEALTH_PATH: &str = "/api/workers/health";

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointFile {
    schema: u8,
    url: String,
    #[serde(rename = "machineId")]
    machine_id: String,
    #[serde(rename = "pairingToken")]
    pairing_token: String,
}

#[derive(Clone)]
pub(crate) struct Endpoint {
    pub url: String,
    pub machine_id: String,
    token: String,
}

#[derive(Serialize)]
pub(crate) struct PairingStatus {
    automatic: bool,
    url: Option<String>,
    machine_id: Option<String>,
    note: String,
}

static STATUS: std::sync::Mutex<Option<PairingStatus>> = std::sync::Mutex::new(None);

pub(crate) fn status() -> PairingStatus {
    let guard = STATUS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_ref() {
        Some(status) => PairingStatus {
            automatic: status.automatic,
            url: status.url.clone(),
            machine_id: status.machine_id.clone(),
            note: status.note.clone(),
        },
        None => PairingStatus {
            automatic: false,
            url: None,
            machine_id: None,
            note: "No local pairing yet; enter the Gateway address and worker tokens below.".into(),
        },
    }
}

fn remember(status: PairingStatus) {
    *STATUS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(status);
}

fn endpoint_path() -> PathBuf {
    std::env::var_os("GATEWAY_ENDPOINT_FILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".kh/gateway-endpoint.json")))
        .unwrap_or_else(|| PathBuf::from(".kh/gateway-endpoint.json"))
}

/// Reads the Gateway's pairing file. A file readable by other users is refused, not trusted.
pub(crate) fn read_endpoint() -> Result<Option<Endpoint>, String> {
    let path = endpoint_path();
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("The local pairing file could not be read.".into()),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("The local pairing file is readable by other users; it was not used.".into());
        }
    }
    let bytes = std::fs::read(&path).map_err(|_| "The local pairing file could not be read.")?;
    let file: EndpointFile =
        serde_json::from_slice(&bytes).map_err(|_| "The local pairing file is not valid.".to_string())?;
    if file.schema != SCHEMA {
        return Err("The local pairing file uses an unsupported schema.".to_string());
    }
    let url = url::Url::parse(&file.url).map_err(|_| "The local pairing file has an invalid URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("The local pairing file has an invalid URL.".into());
    }
    if !file.pairing_token.starts_with("gwp_") || file.pairing_token.len() < 16 {
        return Err("The local pairing file has an invalid pairing token.".into());
    }
    if file.machine_id.is_empty() || file.machine_id.len() > 256 {
        return Err("The local pairing file has an invalid machine id.".into());
    }
    Ok(Some(Endpoint {
        url: url[..url::Position::BeforePath].trim_end_matches('/').to_string(),
        machine_id: file.machine_id,
        token: file.pairing_token,
    }))
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .redirect(reqwest::redirect::Policy::none())
        // The pairing token must never reach a proxy.
        .no_proxy()
        .build()
        .map_err(|_| "Relay could not prepare a pairing request.".to_string())
}

/// Mints one worker token for this worker id through the pairing token.
pub(crate) async fn mint(endpoint: &Endpoint, worker_id: &str, kind: &str) -> Result<String, String> {
    let body = serde_json::to_string(&serde_json::json!({ "worker_id": worker_id, "kind": kind }))
        .map_err(|_| "Relay could not encode the pairing request.".to_string())?;
    let response = client()?
        .post(format!("{}{MINT_PATH}", endpoint.url))
        .bearer_auth(&endpoint.token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| "Gateway did not answer the pairing request.".to_string())?;
    if !response.status().is_success() {
        return Err(format!("Gateway refused the pairing request (HTTP {}).", response.status().as_u16()));
    }
    let text = response
        .text()
        .await
        .map_err(|_| "Gateway returned an unreadable pairing answer.".to_string())?;
    let body: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| "Gateway returned an unreadable pairing answer.".to_string())?;
    body.get("token")
        .and_then(|token| token.as_str())
        .filter(|token| token.starts_with("ekw_"))
        .map(str::to_string)
        .ok_or_else(|| "Gateway did not return a worker token.".to_string())
}

/// True when the worker token is still accepted; false on an explicit refusal (401/403).
pub(crate) async fn worker_token_accepted(endpoint: &Endpoint, token: &str) -> Result<bool, String> {
    let response = client()?
        .get(format!("{}{HEALTH_PATH}", endpoint.url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "Gateway did not answer the worker check.".to_string())?;
    match response.status().as_u16() {
        401 | 403 => Ok(false),
        status if (200..300).contains(&status) => Ok(true),
        _ => Err("Gateway could not confirm this worker token.".into()),
    }
}

/// Turns dispatch on for the bound worker ids through the manager control the extension obeys.
/// Pairing only mints tokens; dispatch is a separate switch and Gateway starts it off.
pub(crate) async fn enable_dispatch_for(worker_ids: &[String]) -> Result<(), String> {
    let ids: Vec<String> = worker_ids.iter().map(|id| id.trim().to_string()).filter(|id| !id.is_empty()).collect();
    if ids.is_empty() {
        return Err("no worker id is bound".into());
    }
    let endpoint = read_endpoint()?.ok_or_else(|| "no local pairing file on this Mac".to_string())?;
    if !crate::manager_gateway::paired(&endpoint.url).unwrap_or(false) {
        return Err("Relay has no Gateway manager token yet".into());
    }
    crate::manager_gateway::control(&endpoint.url, &ids, true, Some(false), None)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// At launch: record the pairing for the UI, and ensure a worker token exists for every kind that
/// has a bound worker id. Failure is reported as a note, never fatal.
pub(crate) async fn autopair(app: tauri::AppHandle) -> Vec<String> {
    let mut messages = Vec::new();
    let endpoint = match read_endpoint() {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => {
            remember(status());
            return messages;
        }
        Err(error) => {
            remember(PairingStatus {
                automatic: false,
                url: None,
                machine_id: None,
                note: format!("{error} Enter the Gateway address and worker tokens manually."),
            });
            return messages;
        }
    };
    remember(PairingStatus {
        automatic: true,
        url: Some(endpoint.url.clone()),
        machine_id: Some(endpoint.machine_id.clone()),
        note: "Connected through the local pairing file the Gateway writes on this Mac.".into(),
    });
    let mut failures = Vec::new();
    for kind in crate::seeding::KINDS {
        let Some(worker_id) = crate::browser_manager::first_bound_worker_id(&app, kind) else {
            continue;
        };
        let stored = crate::seeding::stored(kind);
        if let Some((stored_id, token)) = &stored {
            if stored_id == &worker_id {
                match worker_token_accepted(&endpoint, token).await {
                    Ok(true) => {
                        // Keep the switch on even when the token is unchanged.
                        let _ = enable_dispatch_for(std::slice::from_ref(&worker_id)).await;
                        continue;
                    }
                    Ok(false) => messages.push(format!("{kind}: its worker token was revoked; minting a new one")),
                    Err(_) => continue,
                }
            }
        }
        match mint(&endpoint, &worker_id, kind).await {
            Ok(token) => match crate::seeding::store_credential(kind, &worker_id, &token) {
                Ok(()) => {
                    let dispatch = match enable_dispatch_for(std::slice::from_ref(&worker_id)).await {
                        Ok(()) => "dispatch on".to_string(),
                        Err(error) => format!("dispatch off — {error}; jobs will not be claimed"),
                    };
                    messages.push(format!("{kind}: paired automatically for {worker_id} · {dispatch}"));
                }
                Err(error) => messages.push(format!("{kind}: minted a token but could not save it — {error}")),
            },
            Err(error) => messages.push(format!("{kind}: automatic pairing failed — {error}")),
        }
    }
    if !messages.is_empty() {
        if messages.iter().any(|note| note.contains("failed") || note.contains("could not")) {
            failures.extend(messages.iter().cloned());
        }
    }
    if !failures.is_empty() {
        let mut status = status();
        status.note = failures.join(" · ");
        remember(status);
    }
    messages
}

#[tauri::command]
pub(crate) fn pairing_status(window: tauri::WebviewWindow) -> Result<PairingStatus, String> {
    super::local_only(&window)?;
    Ok(status())
}

/// Re-runs pairing on demand (Gateway page opened/refreshed) instead of only at launch.
#[tauri::command]
pub(crate) async fn pairing_refresh(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<PairingStatus, String> {
    super::local_only(&window)?;
    let _ = autopair(app).await;
    Ok(status())
}

#[cfg(test)]
mod tests {
    use super::*;

    // GATEWAY_ENDPOINT_FILE is process-wide; the tests must not race on it.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn write_endpoint(bytes: &str, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("relay-pairing-{}-{}", std::process::id(), mode));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gateway-endpoint.json");
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn a_private_schema_one_file_is_accepted_and_other_schemas_are_not() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let path = write_endpoint(
            r#"{"schema":1,"url":"http://127.0.0.1:8787","machineId":"mac-a","pairingToken":"gwp_0123456789abcdef"}"#,
            0o600,
        );
        std::env::set_var("GATEWAY_ENDPOINT_FILE", &path);
        let endpoint = read_endpoint().unwrap().unwrap();
        assert_eq!(endpoint.url, "http://127.0.0.1:8787");
        assert_eq!(endpoint.machine_id, "mac-a");
        std::fs::write(&path, r#"{"schema":2,"url":"http://127.0.0.1:8787","machineId":"mac-a","pairingToken":"gwp_0123456789abcdef"}"#).unwrap();
        assert!(read_endpoint().is_err());
        std::env::remove_var("GATEWAY_ENDPOINT_FILE");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_world_readable_file_is_refused() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let path = write_endpoint(
            r#"{"schema":1,"url":"http://127.0.0.1:8787","machineId":"mac-a","pairingToken":"gwp_0123456789abcdef"}"#,
            0o644,
        );
        std::env::set_var("GATEWAY_ENDPOINT_FILE", &path);
        assert!(read_endpoint().is_err());
        std::env::remove_var("GATEWAY_ENDPOINT_FILE");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
