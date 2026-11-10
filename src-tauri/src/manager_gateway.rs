//! Private manager-auth adapter for Gateway's scoped browser-worker manager token.
//!
//! The token lives only in Relay's own macOS Keychain item, keyed by a SHA-256 of
//! the exact normalized Gateway origin. It is never written to settings, argv,
//! logs, rendered pages or process arguments. This adapter exposes only the two
//! manager routes: GET /api/workers/manager and POST /api/workers/manager/control.
//! It has no job, upload, mint, revoke, account-enable or paid-play method.
//!
//! Gateway wire source: docs/extension-account-workers.md in the Gateway
//! browser-account-workers worktree. The response uses the wire field names
//! (mixed camelCase and snake_case as documented) so callers do not re-key it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::Duration;
use tauri::WebviewWindow;
use url::Url;

const KEYCHAIN_SERVICE: &str = "kh.relay.desktop.manager-gateway.v1";
const MANAGER_PATH: &str = "/api/workers/manager";
const CONTROL_PATH: &str = "/api/workers/manager/control";
const CREDENTIAL_VERSION: u8 = 1;
const MAX_BODY_BYTES: usize = 256 * 1024;
const MAX_TOKEN_BYTES: usize = 4 * 1024;
const MAX_ID_BYTES: usize = 256;
const MAX_WORKERS: usize = 256;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ManagerError {
    UnsupportedPlatform,
    InvalidOrigin,
    InvalidToken,
    InvalidPacing,
    InvalidControl,
    InvalidView,
    NotPaired,
    Keychain(i32),
    StoredCredential,
    StoredCredentialMismatch,
    Client,
    Network,
    Timeout,
    RedirectRefused,
    Unauthorized,
    HttpStatus(u16),
    ResponseTooLarge,
    InvalidResponse,
    DispatchNotPaused,
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::UnsupportedPlatform => "Manager sign-in is available only in Relay's macOS app.",
            Self::InvalidOrigin => {
                "Use the Gateway origin without a path, credentials, query or fragment."
            }
            Self::InvalidToken => "The Gateway manager token is invalid.",
            Self::InvalidPacing => {
                "Pacing needs finite nonnegative seconds with the minimum not above the maximum."
            }
            Self::InvalidControl => {
                "The requested manager control change is invalid or could not be encoded."
            }
            Self::InvalidView => "Gateway's manager view did not match schema 1.",
            Self::NotPaired => "No Relay manager credential is stored for this Gateway.",
            Self::Keychain(code) => {
                return write!(
                    formatter,
                    "Relay could not use its macOS Keychain item (status {code})."
                )
            }
            Self::StoredCredential => {
                "Relay's stored manager credential is unreadable; paste a new token."
            }
            Self::StoredCredentialMismatch => {
                "Relay's stored manager credential belongs to a different Gateway."
            }
            Self::Client => "Relay could not prepare the Gateway request.",
            Self::Network => "Gateway could not be reached.",
            Self::Timeout => "Gateway did not answer in time.",
            Self::RedirectRefused => {
                "Gateway redirected the manager request; no credential was sent on."
            }
            Self::Unauthorized => "Gateway rejected the manager token.",
            Self::HttpStatus(status) => {
                return write!(
                    formatter,
                    "Gateway refused the manager request (HTTP {status})."
                )
            }
            Self::ResponseTooLarge => "Gateway's manager response was too large to read safely.",
            Self::InvalidResponse => "Gateway's manager response was not a JSON document.",
            Self::DispatchNotPaused => "Gateway did not confirm that dispatch is paused.",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ManagerError {}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Pacing {
    #[serde(rename = "minSeconds")]
    pub min_seconds: f64,
    #[serde(rename = "maxSeconds")]
    pub max_seconds: f64,
}

impl Pacing {
    fn validate(&self) -> Result<(), ManagerError> {
        if self.min_seconds.is_finite()
            && self.max_seconds.is_finite()
            && self.min_seconds >= 0.0
            && self.max_seconds >= 0.0
            && self.min_seconds <= self.max_seconds
        {
            Ok(())
        } else {
            Err(ManagerError::InvalidPacing)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Balance {
    pub value: f64,
    pub unit: String,
    pub source: String,
    #[serde(rename = "observedAt")]
    pub observed_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CreditBucket {
    pub id: String,
    pub value: f64,
    pub unit: String,
    pub source: String,
    #[serde(rename = "observedAt")]
    pub observed_at: String,
    #[serde(rename = "resetAt")]
    pub reset_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkerState {
    pub connection: Option<String>,
    pub sign_in: Option<String>,
    pub readiness: Option<String>,
    pub activity: Option<String>,
    pub attention: Option<bool>,
    pub enabled: Option<bool>,
    pub paused: Option<bool>,
    pub source: Option<String>,
    pub observed_at: Option<String>,
    pub next_eligible_at: Option<String>,
    pub control_revision: Option<String>,
    pub pacing: Option<Pacing>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkerControl {
    #[serde(rename = "dispatchEnabled")]
    pub dispatch_enabled: bool,
    #[serde(rename = "closeRequested")]
    pub close_requested: Option<bool>,
    pub revision: String,
    #[serde(rename = "requestedAt")]
    pub requested_at: Option<f64>,
    pub pacing: Option<Pacing>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ManagerWorker {
    pub worker_id: String,
    pub kind: String,
    pub has_live_token: bool,
    pub control: WorkerControl,
    pub state: Option<WorkerState>,
    #[serde(rename = "receivedAt")]
    pub received_at: Option<f64>,
    pub fresh: bool,
    pub current_job: Option<Value>,
    pub held_jobs: Vec<Value>,
    #[serde(rename = "durableBusy")]
    pub durable_busy: bool,
    #[serde(rename = "safeToClose")]
    pub safe_to_close: bool,
    #[serde(rename = "accountBinding")]
    pub account_binding: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ManagerAccount {
    pub provider: String,
    pub account_id: String,
    pub balance: Option<Balance>,
    pub reset_at: Option<String>,
    pub plan: Option<Value>,
    pub credit_buckets: Option<Vec<CreditBucket>>,
    pub source: Option<String>,
    pub observed_at: Option<String>,
    pub received_at: Option<f64>,
    pub fresh: bool,
    pub gateway_enabled: bool,
    pub reserved: Option<Value>,
    pub available: Option<f64>,
    pub reservation_states: Option<Value>,
    pub needs_reconciliation: Option<bool>,
    #[serde(default)]
    pub workers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ManagerView {
    pub schema: u8,
    #[serde(rename = "receivedAt")]
    pub received_at: f64,
    #[serde(rename = "maxAgeSeconds")]
    pub max_age_seconds: f64,
    pub workers: Vec<ManagerWorker>,
    pub accounts: Vec<ManagerAccount>,
}

impl ManagerView {
    fn validate(&self) -> Result<(), ManagerError> {
        if self.schema != 1
            || !self.received_at.is_finite()
            || self.received_at <= 0.0
            || !self.max_age_seconds.is_finite()
            || self.max_age_seconds < 0.0
        {
            return Err(ManagerError::InvalidView);
        }
        let mut worker_ids = HashSet::new();
        for worker in &self.workers {
            if !valid_id(&worker.worker_id)
                || worker.kind.is_empty()
                || worker.control.revision.is_empty()
                || !worker_ids.insert(worker.worker_id.as_str())
            {
                return Err(ManagerError::InvalidView);
            }
            if let Some(pacing) = &worker.control.pacing {
                pacing.validate()?;
            }
            if let Some(pacing) = worker
                .state
                .as_ref()
                .and_then(|state| state.pacing.as_ref())
            {
                pacing.validate()?;
            }
        }
        let mut accounts = HashSet::new();
        for account in &self.accounts {
            if account.provider.is_empty()
                || account.account_id.is_empty()
                || !accounts.insert((account.provider.as_str(), account.account_id.as_str()))
            {
                return Err(ManagerError::InvalidView);
            }
            if let Some(balance) = &account.balance {
                validate_balance(
                    balance.value,
                    &balance.unit,
                    &balance.source,
                    &balance.observed_at,
                )?;
            }
            if let Some(available) = account.available {
                if !available.is_finite() || available < 0.0 {
                    return Err(ManagerError::InvalidView);
                }
            }
            if let Some(buckets) = &account.credit_buckets {
                for bucket in buckets {
                    if !valid_id(&bucket.id) {
                        return Err(ManagerError::InvalidView);
                    }
                    validate_balance(
                        bucket.value,
                        &bucket.unit,
                        &bucket.source,
                        &bucket.observed_at,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// View-level freshness from Gateway's own receive stamp. Worker-level
    /// `fresh` remains authoritative for a pause/close acknowledgement.
    pub(crate) fn is_fresh(&self, now_epoch: f64) -> bool {
        now_epoch.is_finite()
            && now_epoch >= self.received_at
            && now_epoch - self.received_at <= self.max_age_seconds
    }
}

fn validate_balance(
    value: f64,
    unit: &str,
    source: &str,
    observed_at: &str,
) -> Result<(), ManagerError> {
    if value.is_finite()
        && value >= 0.0
        && !unit.is_empty()
        && !source.is_empty()
        && !observed_at.is_empty()
    {
        Ok(())
    } else {
        Err(ManagerError::InvalidView)
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CloseSafety {
    pub safe_to_close: bool,
    pub waiting_on: Vec<String>,
    pub reason: Option<String>,
}

/// Caller must pass every worker id sharing the profile. A pause-only control
/// call is never close permission, and a stale/missing acknowledgement waits.
pub(crate) fn close_safety(view: &ManagerView, worker_ids: &[String]) -> CloseSafety {
    let mut waiting_on = Vec::new();
    let mut reasons = Vec::new();
    if worker_ids.is_empty() {
        return CloseSafety {
            safe_to_close: false,
            waiting_on,
            reason: Some("No worker IDs were supplied.".into()),
        };
    }
    for id in worker_ids {
        let Some(worker) = view.workers.iter().find(|worker| &worker.worker_id == id) else {
            waiting_on.push(id.clone());
            reasons.push(format!("{id}: not present in the manager view"));
            continue;
        };
        let mut problems = Vec::new();
        if !worker.fresh {
            problems.push("telemetry is not fresh");
        }
        if worker.control.dispatch_enabled {
            problems.push("dispatch is still enabled");
        }
        if worker.control.close_requested != Some(true) {
            problems.push("close was not requested");
        }
        if worker.current_job.is_some() {
            problems.push("a current job is present");
        }
        if !worker.held_jobs.is_empty() {
            problems.push("held jobs are present");
        }
        if worker.durable_busy {
            problems.push("durable work is still active or unknown");
        }
        if !worker.safe_to_close {
            problems.push("Gateway reports safeToClose false");
        }
        match &worker.state {
            None => problems.push("worker state has not been reported"),
            Some(state) => {
                if state.paused != Some(true) {
                    problems.push("worker has not acknowledged pause");
                }
                if state.control_revision.as_deref() != Some(worker.control.revision.as_str()) {
                    problems.push("pause acknowledgement revision does not match");
                }
            }
        }
        if !problems.is_empty() {
            waiting_on.push(id.clone());
            reasons.push(format!("{id}: {}", problems.join(", ")));
        }
    }
    let safe_to_close = waiting_on.is_empty();
    CloseSafety {
        safe_to_close,
        waiting_on,
        reason: if safe_to_close {
            None
        } else {
            Some(reasons.join("; "))
        },
    }
}

fn normalize_origin(value: &str) -> Result<String, ManagerError> {
    let parsed = Url::parse(value.trim()).map_err(|_| ManagerError::InvalidOrigin)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(ManagerError::InvalidOrigin);
    }
    Ok(parsed.origin().ascii_serialization())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_BYTES && !value.chars().any(char::is_control)
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value.chars().all(|character| character.is_ascii_graphic())
}

fn validate_worker_ids(worker_ids: &[String]) -> Result<(), ManagerError> {
    if worker_ids.is_empty() || worker_ids.len() > MAX_WORKERS {
        return Err(ManagerError::InvalidControl);
    }
    let mut seen = HashSet::new();
    for id in worker_ids {
        if !valid_id(id) || !seen.insert(id.as_str()) {
            return Err(ManagerError::InvalidControl);
        }
    }
    Ok(())
}

fn origin_account(origin: &str) -> String {
    format!("{:x}", Sha256::digest(origin.as_bytes()))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    version: u8,
    origin: String,
    token: String,
}

fn encode_credential(origin: &str, token: &str) -> Result<Vec<u8>, ManagerError> {
    serde_json::to_vec(&Credential {
        version: CREDENTIAL_VERSION,
        origin: origin.to_owned(),
        token: token.to_owned(),
    })
    .map_err(|_| ManagerError::StoredCredential)
}

fn decode_credential(bytes: &[u8], origin: &str) -> Result<String, ManagerError> {
    let credential: Credential =
        serde_json::from_slice(bytes).map_err(|_| ManagerError::StoredCredential)?;
    if credential.version != CREDENTIAL_VERSION || !valid_token(&credential.token) {
        return Err(ManagerError::StoredCredential);
    }
    if credential.origin != origin {
        return Err(ManagerError::StoredCredentialMismatch);
    }
    Ok(credential.token)
}

#[cfg(target_os = "macos")]
fn store_token(origin: &str, token: &str) -> Result<(), ManagerError> {
    let bytes = encode_credential(origin, token)?;
    security_framework::passwords::set_generic_password(
        KEYCHAIN_SERVICE,
        &origin_account(origin),
        &bytes,
    )
    .map_err(|error| ManagerError::Keychain(error.code()))
}

#[cfg(not(target_os = "macos"))]
fn store_token(_: &str, _: &str) -> Result<(), ManagerError> {
    Err(ManagerError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
fn load_token(origin: &str) -> Result<Option<String>, ManagerError> {
    match security_framework::passwords::get_generic_password(
        KEYCHAIN_SERVICE,
        &origin_account(origin),
    ) {
        Ok(bytes) => decode_credential(&bytes, origin).map(Some),
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(error) => Err(ManagerError::Keychain(error.code())),
    }
}

#[cfg(not(target_os = "macos"))]
fn load_token(_: &str) -> Result<Option<String>, ManagerError> {
    Err(ManagerError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
fn clear_token(origin: &str) -> Result<(), ManagerError> {
    match security_framework::passwords::delete_generic_password(
        KEYCHAIN_SERVICE,
        &origin_account(origin),
    ) {
        Ok(()) => Ok(()),
        // Deleting only our own item; an already absent item is already the goal.
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
        Err(error) => Err(ManagerError::Keychain(error.code())),
    }
}

#[cfg(not(target_os = "macos"))]
fn clear_token(_: &str) -> Result<(), ManagerError> {
    Err(ManagerError::UnsupportedPlatform)
}

fn http_client() -> Result<reqwest::Client, ManagerError> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        // A configured proxy must never receive the manager token.
        .no_proxy()
        .build()
        .map_err(|_| ManagerError::Client)
}

fn endpoint(origin: &str, path: &str) -> Result<Url, ManagerError> {
    let base = Url::parse(origin).map_err(|_| ManagerError::InvalidOrigin)?;
    let url = base.join(path).map_err(|_| ManagerError::InvalidOrigin)?;
    if url.origin().ascii_serialization() != origin
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ManagerError::InvalidOrigin);
    }
    Ok(url)
}

fn network_error(error: reqwest::Error) -> ManagerError {
    if error.is_timeout() {
        ManagerError::Timeout
    } else {
        ManagerError::Network
    }
}

fn append_bounded(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), ManagerError> {
    if body.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
        return Err(ManagerError::ResponseTooLarge);
    }
    body.extend_from_slice(chunk);
    Ok(())
}

async fn json_response(response: reqwest::Response, origin: &str) -> Result<Value, ManagerError> {
    if response.url().origin().ascii_serialization() != origin {
        return Err(ManagerError::RedirectRefused);
    }
    let status = response.status();
    if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
        return Err(ManagerError::RedirectRefused);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ManagerError::Unauthorized);
    }
    if !status.is_success() {
        return Err(ManagerError::HttpStatus(status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY_BYTES as u64)
    {
        return Err(ManagerError::ResponseTooLarge);
    }
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        append_bounded(&mut body, &chunk)?;
    }
    // Only the parsed document leaves this module; raw responses and headers do not.
    serde_json::from_slice(&body).map_err(|_| ManagerError::InvalidResponse)
}

fn parse_view(document: Value) -> Result<ManagerView, ManagerError> {
    let view: ManagerView =
        serde_json::from_value(document).map_err(|_| ManagerError::InvalidView)?;
    view.validate()?;
    Ok(view)
}

fn pause_confirmed(view: &ManagerView, worker_ids: &[String]) -> bool {
    worker_ids.iter().all(|id| {
        view.workers
            .iter()
            .find(|worker| &worker.worker_id == id)
            .is_some_and(|worker| !worker.control.dispatch_enabled)
    })
}

pub(crate) struct ManagerClient {
    origin: String,
    token: String,
}

impl ManagerClient {
    pub(crate) fn new(origin: &str, token: &str) -> Result<Self, ManagerError> {
        if !valid_token(token) {
            return Err(ManagerError::InvalidToken);
        }
        Ok(Self {
            origin: normalize_origin(origin)?,
            token: token.to_owned(),
        })
    }

    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, ManagerError> {
        Ok(http_client()?
            .request(method, endpoint(&self.origin, path)?)
            .header(reqwest::header::ACCEPT, "application/json")
            .bearer_auth(&self.token))
    }

    pub(crate) async fn read(&self) -> Result<ManagerView, ManagerError> {
        let response = self
            .request(reqwest::Method::GET, MANAGER_PATH)?
            .send()
            .await
            .map_err(network_error)?;
        parse_view(json_response(response, &self.origin).await?)
    }

    async fn control(
        &self,
        worker_ids: &[String],
        dispatch_enabled: bool,
        close_requested: Option<bool>,
        pacing: Option<Pacing>,
    ) -> Result<ManagerView, ManagerError> {
        let body = serde_json::to_vec(&ControlRequest {
            worker_ids,
            dispatch_enabled,
            close_requested,
            pacing,
        })
        .map_err(|_| ManagerError::InvalidControl)?;
        let response = self
            .request(reqwest::Method::POST, CONTROL_PATH)?
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(network_error)?;
        let view = parse_view(json_response(response, &self.origin).await?)?;
        if !dispatch_enabled && !pause_confirmed(&view, worker_ids) {
            return Err(ManagerError::DispatchNotPaused);
        }
        Ok(view)
    }
}

#[derive(Serialize)]
struct ControlRequest<'a> {
    worker_ids: &'a [String],
    #[serde(rename = "dispatchEnabled")]
    dispatch_enabled: bool,
    #[serde(rename = "closeRequested", skip_serializing_if = "Option::is_none")]
    close_requested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pacing: Option<Pacing>,
}

/// Verify the token with the real manager GET, then store it against this origin.
pub(crate) async fn pair(origin: &str, token: &str) -> Result<ManagerView, ManagerError> {
    let client = ManagerClient::new(origin, token)?;
    let view = client.read().await?;
    store_token(&client.origin, &client.token)?;
    Ok(view)
}

/// Read-only. Never sends a control change, so it cannot resume paused work.
pub(crate) async fn read(origin: &str) -> Result<ManagerView, ManagerError> {
    let origin = normalize_origin(origin)?;
    let token = load_token(&origin)?.ok_or(ManagerError::NotPaired)?;
    ManagerClient::new(&origin, &token)?.read().await
}

/// Sends exactly the caller's control values; there is no implicit enable.
pub(crate) async fn control(
    origin: &str,
    worker_ids: &[String],
    dispatch_enabled: bool,
    close_requested: Option<bool>,
    pacing: Option<Pacing>,
) -> Result<ManagerView, ManagerError> {
    validate_worker_ids(worker_ids)?;
    if dispatch_enabled && close_requested == Some(true) {
        return Err(ManagerError::InvalidControl);
    }
    if let Some(pacing) = &pacing {
        pacing.validate()?;
    }
    let origin = normalize_origin(origin)?;
    let token = load_token(&origin)?.ok_or(ManagerError::NotPaired)?;
    ManagerClient::new(&origin, &token)?
        .control(worker_ids, dispatch_enabled, close_requested, pacing)
        .await
}

fn stored_token(origin: &str) -> Result<String, String> {
    let origin = normalize_origin(origin).map_err(|error| error.to_string())?;
    load_token(&origin)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| ManagerError::NotPaired.to_string())
}

#[tauri::command]
pub(crate) async fn manager_pair_gateway(
    window: WebviewWindow,
    origin: String,
    token: String,
) -> Result<ManagerView, String> {
    super::local_only(&window)?;
    pair(&origin, &token)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn manager_read_gateway(
    window: WebviewWindow,
    origin: String,
) -> Result<ManagerView, String> {
    super::local_only(&window)?;
    read(&origin).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn manager_control_gateway(
    window: WebviewWindow,
    origin: String,
    worker_ids: Vec<String>,
    dispatch_enabled: bool,
    close_requested: Option<bool>,
    pacing: Option<Pacing>,
) -> Result<ManagerView, String> {
    super::local_only(&window)?;
    control(
        &origin,
        &worker_ids,
        dispatch_enabled,
        close_requested,
        pacing,
    )
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn manager_clear_gateway(window: WebviewWindow, origin: String) -> Result<(), String> {
    super::local_only(&window)?;
    let origin = normalize_origin(&origin).map_err(|error| error.to_string())?;
    clear_token(&origin).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn worker(id: &str) -> Value {
        json!({
            "worker_id": id,
            "kind": "video",
            "has_live_token": true,
            "control": {
                "dispatchEnabled": false,
                "closeRequested": true,
                "revision": "revision-1",
                "requestedAt": 1794350000.0,
                "pacing": {"minSeconds": 5, "maxSeconds": 15}
            },
            "state": {
                "paused": true,
                "control_revision": "revision-1",
                "observed_at": "2026-11-11T01:00:00Z"
            },
            "receivedAt": 1794350000.0,
            "fresh": true,
            "current_job": null,
            "held_jobs": [],
            "durableBusy": false,
            "safeToClose": true,
            "accountBinding": null
        })
    }

    fn view(workers: Vec<Value>) -> ManagerView {
        parse_view(json!({
            "schema": 1,
            "receivedAt": 1794350000.0,
            "maxAgeSeconds": 90,
            "workers": workers,
            "accounts": []
        }))
        .unwrap()
    }

    #[test]
    fn origin_must_be_a_plain_http_origin() {
        assert_eq!(
            normalize_origin("https://gateway.example:8787/").unwrap(),
            "https://gateway.example:8787"
        );
        for bad in [
            "https://user:pw@gateway.example",
            "https://gateway.example/path",
            "https://gateway.example?query=1",
            "https://gateway.example#fragment",
            "ftp://gateway.example",
            "gateway.example:8787",
        ] {
            assert_eq!(normalize_origin(bad), Err(ManagerError::InvalidOrigin));
        }
    }

    #[test]
    fn keychain_account_binds_the_exact_origin_by_hash() {
        let first = origin_account("https://one.example");
        assert_ne!(first, origin_account("https://two.example"));
        assert_eq!(first, origin_account("https://one.example"));
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn stored_credential_rejects_a_different_origin() {
        let bytes = encode_credential("https://one.example", "secret-token").unwrap();
        assert_eq!(
            decode_credential(&bytes, "https://one.example").unwrap(),
            "secret-token"
        );
        assert_eq!(
            decode_credential(&bytes, "https://two.example"),
            Err(ManagerError::StoredCredentialMismatch)
        );
    }

    #[test]
    fn body_limit_is_enforced_before_parsing() {
        let mut body = Vec::new();
        append_bounded(&mut body, &vec![b'a'; MAX_BODY_BYTES]).unwrap();
        assert_eq!(
            append_bounded(&mut body, b"x"),
            Err(ManagerError::ResponseTooLarge)
        );
    }

    #[test]
    fn view_requires_schema_one_and_a_coherent_shape() {
        assert!(view(vec![worker("profile-a-video")]).workers.len() == 1);
        for document in [
            json!({"schema": 2, "receivedAt": 1.0, "maxAgeSeconds": 90, "workers": [], "accounts": []}),
            json!({"schema": 1, "receivedAt": 0.0, "maxAgeSeconds": 90, "workers": [], "accounts": []}),
            json!({"schema": 1, "receivedAt": 1.0, "maxAgeSeconds": -1, "workers": [], "accounts": []}),
            json!({"schema": 1, "receivedAt": 1.0, "maxAgeSeconds": 90, "workers": [worker("a"), worker("a")], "accounts": []}),
        ] {
            assert_eq!(parse_view(document).err(), Some(ManagerError::InvalidView));
        }
    }

    #[test]
    fn close_is_safe_only_with_fresh_pause_ack_revision_and_no_work() {
        let ids = vec!["profile-a-video".to_string()];
        let safe = close_safety(&view(vec![worker("profile-a-video")]), &ids);
        assert!(safe.safe_to_close);
        assert!(safe.waiting_on.is_empty());

        let mut stale = worker("profile-a-video");
        stale["fresh"] = json!(false);
        assert!(!close_safety(&view(vec![stale]), &ids).safe_to_close);

        let mut running = worker("profile-a-video");
        running["current_job"] = json!({"jobId": "job-1"});
        assert!(!close_safety(&view(vec![running]), &ids).safe_to_close);

        let mut held = worker("profile-a-video");
        held["held_jobs"] = json!([{"jobId": "job-2"}]);
        assert!(!close_safety(&view(vec![held]), &ids).safe_to_close);

        let mut durable = worker("profile-a-video");
        durable["durableBusy"] = json!(true);
        assert!(!close_safety(&view(vec![durable]), &ids).safe_to_close);

        let mut not_paused = worker("profile-a-video");
        not_paused["state"]["paused"] = json!(false);
        assert!(!close_safety(&view(vec![not_paused]), &ids).safe_to_close);

        let mut wrong_revision = worker("profile-a-video");
        wrong_revision["state"]["control_revision"] = json!("revision-0");
        assert!(!close_safety(&view(vec![wrong_revision]), &ids).safe_to_close);

        let mut pause_only = worker("profile-a-video");
        pause_only["control"]["closeRequested"] = json!(false);
        assert!(!close_safety(&view(vec![pause_only]), &ids).safe_to_close);

        assert!(
            !close_safety(&view(vec![worker("profile-a-video")]), &["missing".into()])
                .safe_to_close
        );
    }

    #[test]
    fn control_validation_rejects_unsafe_combinations() {
        assert_eq!(validate_worker_ids(&[]), Err(ManagerError::InvalidControl));
        assert_eq!(
            validate_worker_ids(&["a".into(), "a".into()]),
            Err(ManagerError::InvalidControl)
        );
        assert!(validate_worker_ids(&["a".into(), "b".into()]).is_ok());
        assert_eq!(
            validate_worker_ids(&["bad\nid".into()]),
            Err(ManagerError::InvalidControl)
        );
        assert_eq!(
            Pacing {
                min_seconds: -1.0,
                max_seconds: 15.0
            }
            .validate(),
            Err(ManagerError::InvalidPacing)
        );
        assert_eq!(
            Pacing {
                min_seconds: 16.0,
                max_seconds: 15.0
            }
            .validate(),
            Err(ManagerError::InvalidPacing)
        );
        assert_eq!(
            Pacing {
                min_seconds: f64::NAN,
                max_seconds: 15.0
            }
            .validate(),
            Err(ManagerError::InvalidPacing)
        );
        assert!(Pacing {
            min_seconds: 0.0,
            max_seconds: 0.0
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn pause_confirmation_requires_every_requested_worker() {
        let ids = vec!["a".into(), "b".into()];
        let both = view(vec![worker("a"), worker("b")]);
        assert!(pause_confirmed(&both, &ids));
        assert!(!pause_confirmed(&view(vec![worker("a")]), &ids));
        let mut enabled = worker("b");
        enabled["control"]["dispatchEnabled"] = json!(true);
        assert!(!pause_confirmed(&view(vec![worker("a"), enabled]), &ids));
    }

    #[test]
    fn tokens_and_ids_reject_unsafe_values() {
        assert!(valid_token("abc.DEF-123"));
        assert!(!valid_token(""));
        assert!(!valid_token("two words"));
        assert!(!valid_token("non-ascii-\u{e9}"));
        assert!(!valid_token(&"a".repeat(MAX_TOKEN_BYTES + 1)));
        assert!(valid_id("worker-manager"));
        assert!(!valid_id(""));
    }

    #[test]
    fn account_validation_keeps_zero_distinct_from_unknown() {
        let document = json!({
            "schema": 1,
            "receivedAt": 1.0,
            "maxAgeSeconds": 90,
            "workers": [],
            "accounts": [{
                "provider": "flow",
                "account_id": "opaque",
                "balance": {
                    "value": 0.0,
                    "unit": "credits",
                    "source": "flow.account",
                    "observedAt": "2026-11-11T01:00:00Z"
                },
                "reset_at": null,
                "plan": null,
                "credit_buckets": null,
                "source": "flow.account",
                "observed_at": "2026-11-11T01:00:00Z",
                "received_at": 1.0,
                "fresh": true,
                "gateway_enabled": false,
                "reserved": null,
                "available": 0.0,
                "reservation_states": null,
                "needs_reconciliation": false,
                "workers": []
            }]
        });
        assert_eq!(
            parse_view(document).unwrap().accounts[0]
                .balance
                .unwrap()
                .value,
            0.0
        );

        let bad = json!({
            "schema": 1,
            "receivedAt": 1.0,
            "maxAgeSeconds": 90,
            "workers": [],
            "accounts": [{
                "provider": "flow",
                "account_id": "opaque",
                "balance": {
                    "value": -1.0,
                    "unit": "credits",
                    "source": "flow.account",
                    "observedAt": "2026-11-11T01:00:00Z"
                },
                "reset_at": null,
                "plan": null,
                "credit_buckets": null,
                "source": null,
                "observed_at": null,
                "received_at": null,
                "fresh": false,
                "gateway_enabled": false,
                "reserved": null,
                "available": null,
                "reservation_states": null,
                "needs_reconciliation": null,
                "workers": []
            }]
        });
        assert_eq!(parse_view(bad).err(), Some(ManagerError::InvalidView));
    }
}
