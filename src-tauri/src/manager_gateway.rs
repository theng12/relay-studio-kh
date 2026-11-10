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
// Small allowance for Gateway/Relay clock skew; larger gaps fail closed.
const CLOCK_SKEW_SECONDS: f64 = 5.0;
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
    FutureView,
    StaleView,
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
            Self::FutureView => "Gateway's manager view is dated in the future.",
            Self::StaleView => "Gateway's manager view is older than its freshness limit.",
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
    // Gateway manager responses use normalized snake_case epoch seconds.
    pub observed_at: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CreditBucket {
    pub id: String,
    pub value: f64,
    pub unit: String,
    pub source: String,
    pub observed_at: f64,
    pub reset_at: Option<f64>,
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
    pub observed_at: Option<f64>,
    pub next_eligible_at: Option<f64>,
    pub control_revision: Option<String>,
    pub pacing: Option<Pacing>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkerControl {
    #[serde(rename = "dispatchEnabled")]
    pub dispatch_enabled: bool,
    #[serde(rename = "closeRequested")]
    pub close_requested: Option<bool>,
    #[serde(rename = "credentialGeneration")]
    pub credential_generation: Option<String>,
    pub revision: Option<String>,
    #[serde(rename = "requestedAt")]
    pub requested_at: Option<f64>,
    pub pacing: Option<Pacing>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ManagerWorker {
    pub worker_id: String,
    pub kind: Option<String>,
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
    pub reset_at: Option<f64>,
    pub plan: Option<Value>,
    pub credit_buckets: Option<Vec<CreditBucket>>,
    pub source: Option<String>,
    pub observed_at: Option<f64>,
    pub received_at: Option<f64>,
    pub fresh: bool,
    pub gateway_enabled: bool,
    pub reserved: Option<f64>,
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
            if !valid_id(&worker.worker_id) || !worker_ids.insert(worker.worker_id.as_str()) {
                return Err(ManagerError::InvalidView);
            }
            validate_optional_epoch(worker.received_at)?;
            validate_optional_epoch(worker.control.requested_at)?;
            if worker
                .control
                .credential_generation
                .as_deref()
                .is_some_and(|generation| !valid_id(generation))
            {
                return Err(ManagerError::InvalidView);
            }
            if worker.kind.as_deref().is_some_and(|kind| !valid_id(kind)) {
                return Err(ManagerError::InvalidView);
            }
            if let Some(pacing) = &worker.control.pacing {
                pacing.validate()?;
            }
            if let Some(state) = &worker.state {
                validate_optional_epoch(state.observed_at)?;
                validate_optional_epoch(state.next_eligible_at)?;
                if let Some(pacing) = &state.pacing {
                    pacing.validate()?;
                }
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
            validate_optional_epoch(account.observed_at)?;
            validate_optional_epoch(account.reset_at)?;
            validate_optional_epoch(account.received_at)?;
            if let Some(balance) = &account.balance {
                validate_balance(
                    balance.value,
                    &balance.unit,
                    &balance.source,
                    balance.observed_at,
                )?;
            }
            if let Some(reserved) = account.reserved {
                if !reserved.is_finite() || reserved < 0.0 {
                    return Err(ManagerError::InvalidView);
                }
            }
            // Negative available is an honest reserve deficit; preserve it, never clamp.
            if account
                .available
                .is_some_and(|available| !available.is_finite())
            {
                return Err(ManagerError::InvalidView);
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
                        bucket.observed_at,
                    )?;
                    validate_optional_epoch(bucket.reset_at)?;
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

    /// Reject future-dated or stale response timestamps before trusting a view.
    pub(crate) fn validate_times(&self, now_epoch: f64) -> Result<(), ManagerError> {
        if !now_epoch.is_finite() {
            return Err(ManagerError::InvalidView);
        }
        check_epoch_fresh(self.received_at, now_epoch, self.max_age_seconds)?;
        for worker in &self.workers {
            if let Some(received_at) = worker.received_at {
                // A stale worker is valid data with fresh:false; close safety rejects it.
                check_epoch_not_future(received_at, now_epoch)?;
            }
        }
        for account in &self.accounts {
            if let Some(received_at) = account.received_at {
                // Stale accounts remain displayable; only impossible future times fail.
                check_epoch_not_future(received_at, now_epoch)?;
            }
        }
        Ok(())
    }
}

fn check_epoch_not_future(value: f64, now_epoch: f64) -> Result<(), ManagerError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ManagerError::InvalidView);
    }
    if value > now_epoch + CLOCK_SKEW_SECONDS {
        return Err(ManagerError::FutureView);
    }
    Ok(())
}

fn check_epoch_fresh(value: f64, now_epoch: f64, max_age_seconds: f64) -> Result<(), ManagerError> {
    check_epoch_not_future(value, now_epoch)?;
    if now_epoch - value > max_age_seconds + CLOCK_SKEW_SECONDS {
        return Err(ManagerError::StaleView);
    }
    Ok(())
}

fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn view_time_ok(view: &ManagerView, now_epoch: f64) -> bool {
    now_epoch.is_finite()
        && view.received_at <= now_epoch + CLOCK_SKEW_SECONDS
        && now_epoch - view.received_at <= view.max_age_seconds + CLOCK_SKEW_SECONDS
}

fn validate_optional_epoch(value: Option<f64>) -> Result<(), ManagerError> {
    match value {
        Some(value) if !value.is_finite() || value <= 0.0 => Err(ManagerError::InvalidView),
        _ => Ok(()),
    }
}

fn validate_balance(
    value: f64,
    unit: &str,
    source: &str,
    observed_at: f64,
) -> Result<(), ManagerError> {
    if value.is_finite()
        && value >= 0.0
        && !unit.is_empty()
        && !source.is_empty()
        && observed_at.is_finite()
        && observed_at > 0.0
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

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ExpectedWorkerBinding {
    pub worker_id: String,
    pub kind: String,
    pub credential_generation: String,
}

/// Bind every worker sharing the profile to the exact expected kind and
/// credential generation. A legacy or re-paired worker without that evidence
/// stays pending until the caller reconciles the profile binding.
pub(crate) fn close_safety_for_bindings(
    view: &ManagerView,
    bindings: &[ExpectedWorkerBinding],
) -> CloseSafety {
    close_safety_for_bindings_at(view, bindings, now_epoch())
}

pub(crate) fn close_safety_for_bindings_at(
    view: &ManagerView,
    bindings: &[ExpectedWorkerBinding],
    now_epoch: f64,
) -> CloseSafety {
    let waiting_on: Vec<String> = bindings
        .iter()
        .map(|binding| binding.worker_id.clone())
        .collect();
    if !view_time_ok(view, now_epoch) {
        return CloseSafety {
            safe_to_close: false,
            waiting_on,
            reason: Some("Manager view is stale or dated in the future.".into()),
        };
    }
    if bindings.is_empty() || bindings.len() > MAX_WORKERS {
        return CloseSafety {
            safe_to_close: false,
            waiting_on,
            reason: Some("No valid generation-bound worker bindings were supplied.".into()),
        };
    }
    let mut seen = HashSet::new();
    for binding in bindings {
        if !valid_id(&binding.worker_id)
            || !valid_id(&binding.kind)
            || !valid_id(&binding.credential_generation)
            || !seen.insert(binding.worker_id.as_str())
        {
            return CloseSafety {
                safe_to_close: false,
                waiting_on,
                reason: Some(
                    "Worker bindings must have one nonempty, unique id, kind and credential generation."
                        .into(),
                ),
            };
        }
    }
    let mut waiting_on = Vec::new();
    let mut reasons = Vec::new();
    for binding in bindings {
        let id = &binding.worker_id;
        let Some(worker) = view.workers.iter().find(|worker| &worker.worker_id == id) else {
            waiting_on.push(id.clone());
            reasons.push(format!("{id}: not present in the manager view"));
            continue;
        };
        let mut problems = Vec::new();
        if !worker.has_live_token {
            problems.push("worker has no live credential");
        }
        if worker.kind.as_deref() != Some(binding.kind.as_str()) {
            problems.push("worker kind does not match the profile binding");
        }
        match worker.control.credential_generation.as_deref() {
            None | Some("") => problems.push("credential generation is not reported"),
            Some(generation) if generation != binding.credential_generation => {
                problems.push("credential generation does not match the profile binding")
            }
            Some(_) => {}
        }
        match worker.received_at {
            None => problems.push("worker receive time is not reported"),
            Some(received_at) if !received_at.is_finite() || received_at <= 0.0 => {
                problems.push("worker receive time is invalid")
            }
            Some(received_at) if received_at > now_epoch + CLOCK_SKEW_SECONDS => {
                problems.push("worker receive time is in the future")
            }
            Some(received_at)
                if now_epoch - received_at > view.max_age_seconds + CLOCK_SKEW_SECONDS =>
            {
                problems.push("worker telemetry is stale")
            }
            Some(_) => {}
        }
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
                match state.observed_at {
                    None => problems.push("worker source observation time is not reported"),
                    Some(observed_at) if !observed_at.is_finite() || observed_at <= 0.0 => {
                        problems.push("worker source observation time is invalid")
                    }
                    Some(observed_at) if observed_at > now_epoch => {
                        problems.push("worker source observation time is in the future")
                    }
                    Some(observed_at) if now_epoch - observed_at > view.max_age_seconds => {
                        problems.push("worker source observation is stale")
                    }
                    Some(_) => {}
                }
                if state.paused != Some(true) {
                    problems.push("worker has not acknowledged pause");
                }
                match worker.control.revision.as_deref() {
                    None => problems.push("control revision has not been issued"),
                    Some(revision) if state.control_revision.as_deref() != Some(revision) => {
                        problems.push("pause acknowledgement revision does not match")
                    }
                    Some(_) => {}
                }
                match state.activity.as_deref() {
                    Some("idle" | "cooling" | "resting" | "waiting") => {}
                    Some(_) => problems.push("worker activity is not a known safe state"),
                    None => problems.push("worker activity has not been observed"),
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

fn generation_unbound(worker_ids: &[String]) -> CloseSafety {
    CloseSafety {
        safe_to_close: false,
        waiting_on: worker_ids.to_vec(),
        reason: Some("Generation-bound worker bindings are required for close.".into()),
    }
}

/// Legacy unbound callers never authorize close; use `close_safety_for_bindings`.
pub(crate) fn close_safety(_view: &ManagerView, worker_ids: &[String]) -> CloseSafety {
    generation_unbound(worker_ids)
}

pub(crate) fn close_safety_at(
    _view: &ManagerView,
    worker_ids: &[String],
    _now_epoch: f64,
) -> CloseSafety {
    generation_unbound(worker_ids)
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
    view.validate_times(now_epoch())?;
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

/// Local Keychain presence only; live auth still requires a successful read.
pub(crate) fn paired(origin: &str) -> Result<bool, ManagerError> {
    let origin = normalize_origin(origin)?;
    Ok(load_token(&origin)?.is_some())
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
pub(crate) fn manager_paired_gateway(
    window: WebviewWindow,
    origin: String,
) -> Result<bool, String> {
    super::local_only(&window)?;
    paired(&origin).map_err(|error| error.to_string())
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

    const NOW: f64 = 1794350001.0;

    fn worker(id: &str) -> Value {
        json!({
            "worker_id": id,
            "kind": "video",
            "has_live_token": true,
            "control": {
                "dispatchEnabled": false,
                "closeRequested": true,
                "credentialGeneration": "generation-1",
                "revision": "revision-1",
                "requestedAt": 1794350000.0,
                "pacing": {"minSeconds": 5, "maxSeconds": 15}
            },
            "state": {
                "connection": "connected",
                "sign_in": "signed_in",
                "readiness": "ready",
                "activity": "idle",
                "attention": false,
                "enabled": true,
                "paused": true,
                "source": "flow.account",
                "observed_at": 1794350000.0,
                "next_eligible_at": null,
                "control_revision": "revision-1",
                "pacing": {"minSeconds": 5, "maxSeconds": 15}
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

    fn bindings(ids: &[&str]) -> Vec<ExpectedWorkerBinding> {
        ids.iter()
            .map(|id| ExpectedWorkerBinding {
                worker_id: (*id).into(),
                kind: "video".into(),
                credential_generation: "generation-1".into(),
            })
            .collect()
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
        let mut empty_generation = worker("a");
        empty_generation["control"]["credentialGeneration"] = json!("");
        assert_eq!(
            parse_view(json!({"schema": 1, "receivedAt": 1.0, "maxAgeSeconds": 90, "workers": [empty_generation], "accounts": []})).err(),
            Some(ManagerError::InvalidView)
        );
    }

    #[test]
    fn close_is_safe_only_with_fresh_pause_ack_revision_and_no_work() {
        let ids = bindings(&["profile-a-video"]);
        let safe = close_safety_for_bindings_at(&view(vec![worker("profile-a-video")]), &ids, NOW);
        assert!(safe.safe_to_close);
        assert!(safe.waiting_on.is_empty());

        let mut stale = worker("profile-a-video");
        stale["fresh"] = json!(false);
        assert!(!close_safety_for_bindings_at(&view(vec![stale]), &ids, NOW).safe_to_close);

        let mut running = worker("profile-a-video");
        running["current_job"] = json!({"jobId": "job-1"});
        assert!(!close_safety_for_bindings_at(&view(vec![running]), &ids, NOW).safe_to_close);

        let mut held = worker("profile-a-video");
        held["held_jobs"] = json!([{"jobId": "job-2"}]);
        assert!(!close_safety_for_bindings_at(&view(vec![held]), &ids, NOW).safe_to_close);

        let mut durable = worker("profile-a-video");
        durable["durableBusy"] = json!(true);
        assert!(!close_safety_for_bindings_at(&view(vec![durable]), &ids, NOW).safe_to_close);

        let mut not_paused = worker("profile-a-video");
        not_paused["state"]["paused"] = json!(false);
        assert!(!close_safety_for_bindings_at(&view(vec![not_paused]), &ids, NOW).safe_to_close);

        let mut wrong_revision = worker("profile-a-video");
        wrong_revision["state"]["control_revision"] = json!("revision-0");
        assert!(
            !close_safety_for_bindings_at(&view(vec![wrong_revision]), &ids, NOW).safe_to_close
        );

        let mut pause_only = worker("profile-a-video");
        pause_only["control"]["closeRequested"] = json!(false);
        assert!(!close_safety_for_bindings_at(&view(vec![pause_only]), &ids, NOW).safe_to_close);

        assert!(
            !close_safety_for_bindings_at(
                &view(vec![worker("profile-a-video")]),
                &bindings(&["missing"]),
                NOW
            )
            .safe_to_close
        );
    }

    #[test]
    fn close_requires_generation_and_kind_binding() {
        let ids = bindings(&["profile-a-video"]);
        let safe_view = view(vec![worker("profile-a-video")]);
        assert!(close_safety_for_bindings_at(&safe_view, &ids, NOW).safe_to_close);
        assert!(!close_safety_for_bindings_at(&safe_view, &[], NOW).safe_to_close);
        assert!(
            !close_safety_for_bindings_at(
                &safe_view,
                &bindings(&["profile-a-video", "profile-a-video"]),
                NOW
            )
            .safe_to_close
        );
        assert!(!close_safety(&safe_view, &["profile-a-video".into()]).safe_to_close);
        assert!(!close_safety_at(&safe_view, &["profile-a-video".into()], NOW).safe_to_close);

        let mut missing = worker("profile-a-video");
        missing["control"]
            .as_object_mut()
            .unwrap()
            .remove("credentialGeneration");
        assert!(!close_safety_for_bindings_at(&view(vec![missing]), &ids, NOW).safe_to_close);

        let mut legacy = worker("profile-a-video");
        legacy["control"]["credentialGeneration"] = json!(null);
        assert!(!close_safety_for_bindings_at(&view(vec![legacy]), &ids, NOW).safe_to_close);

        let mut changed = worker("profile-a-video");
        changed["control"]["credentialGeneration"] = json!("generation-2");
        assert!(!close_safety_for_bindings_at(&view(vec![changed]), &ids, NOW).safe_to_close);

        let mut wrong_kind = worker("profile-a-video");
        wrong_kind["kind"] = json!("image");
        assert!(!close_safety_for_bindings_at(&view(vec![wrong_kind]), &ids, NOW).safe_to_close);

        let mut revoked = worker("profile-a-video");
        revoked["has_live_token"] = json!(false);
        assert!(!close_safety_for_bindings_at(&view(vec![revoked]), &ids, NOW).safe_to_close);

        let wrong_binding = vec![ExpectedWorkerBinding {
            worker_id: "profile-a-video".into(),
            kind: "image".into(),
            credential_generation: "generation-1".into(),
        }];
        assert!(!close_safety_for_bindings_at(&safe_view, &wrong_binding, NOW).safe_to_close);
    }

    #[test]
    fn close_requires_observed_safe_activity() {
        let ids = bindings(&["profile-a-video"]);
        for activity in ["idle", "cooling", "resting", "waiting"] {
            let mut observed = worker("profile-a-video");
            observed["state"]["activity"] = json!(activity);
            assert!(
                close_safety_for_bindings_at(&view(vec![observed]), &ids, NOW).safe_to_close,
                "activity {activity}"
            );
        }
        for activity in [
            json!("generating"),
            json!("recovering"),
            json!("unknown"),
            json!(null),
        ] {
            let mut observed = worker("profile-a-video");
            observed["state"]["activity"] = activity;
            assert!(!close_safety_for_bindings_at(&view(vec![observed]), &ids, NOW).safe_to_close);
        }
        let mut missing = worker("profile-a-video");
        if let Some(state) = missing["state"].as_object_mut() {
            state.remove("activity");
        }
        assert!(!close_safety_for_bindings_at(&view(vec![missing]), &ids, NOW).safe_to_close);
    }

    #[test]
    fn close_rejects_future_or_stale_times() {
        let ids = bindings(&["profile-a-video"]);
        let mut future_worker = worker("profile-a-video");
        future_worker["receivedAt"] = json!(NOW + 100.0);
        assert!(!close_safety_for_bindings_at(&view(vec![future_worker]), &ids, NOW).safe_to_close);

        let mut stale_worker = worker("profile-a-video");
        stale_worker["receivedAt"] = json!(NOW - 1000.0);
        assert!(!close_safety_for_bindings_at(&view(vec![stale_worker]), &ids, NOW).safe_to_close);

        let mut missing_time = worker("profile-a-video");
        missing_time["receivedAt"] = json!(null);
        assert!(!close_safety_for_bindings_at(&view(vec![missing_time]), &ids, NOW).safe_to_close);

        let mut future_view = view(vec![worker("profile-a-video")]);
        future_view.received_at = NOW + 100.0;
        assert!(!close_safety_for_bindings_at(&future_view, &ids, NOW).safe_to_close);

        let mut stale_view = view(vec![worker("profile-a-video")]);
        stale_view.received_at = NOW - 1000.0;
        assert!(!close_safety_for_bindings_at(&stale_view, &ids, NOW).safe_to_close);

        assert!(view(vec![worker("profile-a-video")])
            .validate_times(NOW)
            .is_ok());
        let mut future = view(vec![worker("profile-a-video")]);
        future.received_at = NOW + 100.0;
        assert_eq!(future.validate_times(NOW), Err(ManagerError::FutureView));
        let mut stale = view(vec![worker("profile-a-video")]);
        stale.received_at = NOW - 1000.0;
        assert_eq!(stale.validate_times(NOW), Err(ManagerError::StaleView));
        let mut stale_worker_time = view(vec![worker("profile-a-video")]);
        stale_worker_time.workers[0].received_at = Some(NOW - 1000.0);
        // Stale worker rows are valid data; close safety rejects them separately.
        assert!(stale_worker_time.validate_times(NOW).is_ok());
        let mut future_worker_time = view(vec![worker("profile-a-video")]);
        future_worker_time.workers[0].received_at = Some(NOW + 100.0);
        assert_eq!(
            future_worker_time.validate_times(NOW),
            Err(ManagerError::FutureView)
        );
        assert_eq!(
            view(vec![worker("profile-a-video")]).validate_times(f64::NAN),
            Err(ManagerError::InvalidView)
        );
    }

    #[test]
    fn close_rechecks_source_freshness_even_when_cached_fresh_is_true() {
        let expected = bindings(&["profile-a-video"]);
        let fresh = view(vec![worker("profile-a-video")]);
        assert!(close_safety_for_bindings_at(&fresh, &expected, NOW).safe_to_close);
        for observed_at in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(0.0),
            Some(NOW + 0.1),
            Some(NOW - fresh.max_age_seconds - 0.1),
        ] {
            let mut cached = fresh.clone();
            cached.workers[0].state.as_mut().unwrap().observed_at = observed_at;
            assert!(cached.workers[0].fresh);
            assert!(!close_safety_for_bindings_at(&cached, &expected, NOW).safe_to_close);
        }
    }

    #[test]
    fn close_waits_for_each_physical_sibling_binding() {
        let mut image = worker("profile-a-image");
        image["kind"] = json!("image");
        let video = worker("profile-a-video");
        let mut expected = bindings(&["profile-a-image", "profile-a-video"]);
        expected[0].kind = "image".into();
        assert!(
            close_safety_for_bindings_at(&view(vec![image.clone(), video]), &expected, NOW)
                .safe_to_close
        );
        assert!(!close_safety_for_bindings_at(&view(vec![image]), &expected, NOW).safe_to_close);
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

    fn account() -> Value {
        json!({
            "provider": "flow",
            "account_id": "opaque-account",
            "connection": "connected",
            "sign_in": "signed_in",
            "readiness": "ready",
            "activity": "idle",
            "attention": false,
            "enabled": true,
            "paused": false,
            "next_eligible_at": null,
            "balance": {
                "value": 0.0,
                "unit": "credits",
                "source": "flow.account",
                "observed_at": 1794350000.0,
                "received_at": 1794350000.0,
                "fresh": true
            },
            "reset_at": 1794353600.0,
            "plan": null,
            "credit_buckets": [{
                "id": "daily",
                "value": 0.0,
                "unit": "credits",
                "source": "flow.account",
                "observed_at": 1794350000.0,
                "reset_at": 1794353600.0
            }],
            "source": "flow.account",
            "observed_at": 1794350000.0,
            "received_at": 1794350000.0,
            "fresh": true,
            "gateway_enabled": false,
            "reserved": 0.0,
            "available": 0.0,
            "reservation_states": {"reserved": 0, "held": 0, "debited": 0, "reconciled": 0},
            "needs_reconciliation": false,
            "workers": []
        })
    }

    fn parse_account(account: Value) -> Result<ManagerView, ManagerError> {
        parse_view(json!({
            "schema": 1,
            "receivedAt": 1794350000.0,
            "maxAgeSeconds": 90,
            "workers": [],
            "accounts": [account]
        }))
    }

    fn account_view(account: Value) -> ManagerView {
        parse_account(account).unwrap()
    }

    #[test]
    fn account_validation_keeps_zero_distinct_from_unknown() {
        let view = account_view(account());
        assert_eq!(view.accounts[0].balance.as_ref().unwrap().value, 0.0);
        assert_eq!(view.accounts[0].available, Some(0.0));
        let mut bad = account();
        bad["balance"]["value"] = json!(-1.0);
        assert_eq!(parse_account(bad).err(), Some(ManagerError::InvalidView));
    }

    #[test]
    fn observed_credits_decode_snake_case_epochs() {
        let view = account_view(account());
        let account = &view.accounts[0];
        assert_eq!(account.observed_at, Some(1794350000.0));
        assert_eq!(account.reset_at, Some(1794353600.0));
        assert_eq!(account.balance.as_ref().unwrap().observed_at, 1794350000.0);
        let bucket = &account.credit_buckets.as_ref().unwrap()[0];
        assert_eq!(bucket.observed_at, 1794350000.0);
        assert_eq!(bucket.reset_at, Some(1794353600.0));
        assert_eq!(account.reserved, Some(0.0));
        assert_eq!(account.available, Some(0.0));
    }

    #[test]
    fn negative_available_deficit_is_preserved() {
        let mut account = account();
        account["balance"]["value"] = json!(5.0);
        account["reserved"] = json!(10.0);
        account["available"] = json!(-5.0);
        let view = account_view(account);
        assert_eq!(view.accounts[0].reserved, Some(10.0));
        assert_eq!(view.accounts[0].available, Some(-5.0));
    }

    #[test]
    fn unknown_incomparable_units_keep_available_null() {
        let mut account = account();
        account["reserved"] = json!(null);
        account["available"] = json!(null);
        let view = account_view(account);
        assert_eq!(view.accounts[0].reserved, None);
        assert_eq!(view.accounts[0].available, None);
    }

    #[test]
    fn first_pairing_with_null_revision_is_valid_but_never_close_safe() {
        let ids = bindings(&["profile-a-video"]);
        let mut first = worker("profile-a-video");
        first["control"]["revision"] = json!(null);
        first["control"]["requestedAt"] = json!(null);
        first["control"]["closeRequested"] = json!(false);
        first["state"] = json!(null);
        first["receivedAt"] = json!(null);
        first["fresh"] = json!(false);
        first["safeToClose"] = json!(false);
        let view = view(vec![first]);
        assert_eq!(view.workers[0].control.revision, None);
        assert!(!close_safety_for_bindings_at(&view, &ids, NOW).safe_to_close);
    }

    #[test]
    fn revoked_worker_null_kind_does_not_invalidate_other_workers() {
        let mut revoked = worker("profile-b-video");
        revoked["kind"] = json!(null);
        revoked["has_live_token"] = json!(false);
        revoked["control"]["revision"] = json!(null);
        revoked["control"]["closeRequested"] = json!(false);
        revoked["state"] = json!(null);
        revoked["receivedAt"] = json!(null);
        revoked["fresh"] = json!(false);
        revoked["safeToClose"] = json!(false);
        let view = view(vec![worker("profile-a-video"), revoked]);
        assert_eq!(view.workers.len(), 2);
        assert_eq!(view.workers[1].kind, None);
        assert!(
            !close_safety_for_bindings_at(&view, &bindings(&["profile-b-video"]), NOW)
                .safe_to_close
        );
        assert!(
            close_safety_for_bindings_at(&view, &bindings(&["profile-a-video"]), NOW).safe_to_close
        );
    }
}
