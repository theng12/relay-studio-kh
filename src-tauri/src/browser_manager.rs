//! Relay-owned Chrome profile lifecycle.
//!
//! Real Chrome processes, each with its own Relay app-support user-data-dir. Relay never copies
//! Chrome credentials and never passes security-weakening flags. Process identity is the pid plus
//! the process start time observed at launch, so a reused pid is never treated as ours.
//!
//! Gateway authentication and worker readback are supplied by the parent-owned manager_gateway
//! module through `ManagerGateway`. Until it is installed every Gateway field is unknown, pacing
//! is a preference, and closing waits instead of stopping Chrome. No endpoint or credential is
//! implemented here.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State, WebviewWindow};

const STATE_SCHEMA: u8 = 1;
const RECONCILE_SECONDS: u64 = 15;
const MAX_OPEN_ATTEMPTS: u32 = 3;
const PROFILE_LABEL_LIMIT: usize = 120;
const WORKER_ID_LIMIT: usize = 128;

// ---------------------------------------------------------------------------
// Gateway authority seam (implemented and installed by the parent-owned adapter)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct GatewayProfile {
    pub id: String,
    pub image_worker_id: Option<String>,
    pub video_worker_id: Option<String>,
}

/// Authenticated per-profile readback. Missing fields stay unknown in the snapshot.
#[derive(Clone, Default)]
pub(crate) struct GatewayReading {
    pub connection: Option<String>,
    pub sign_in: Option<String>,
    pub readiness: Option<String>,
    pub activity: Option<String>,
    pub provider: Option<String>,
    pub account_id: Option<String>,
    pub observed_at: Option<String>,
    pub credits: Option<serde_json::Value>,
    pub next_eligible_at: Option<String>,
}

#[derive(Clone)]
pub(crate) struct GatewaySupport {
    pub telemetry_authorized: bool,
    pub telemetry_error: Option<String>,
    pub manager_paired: bool,
    pub pacing_applied: bool,
    pub pacing_error: Option<String>,
}

impl GatewaySupport {
    fn unavailable() -> Self {
        Self {
            telemetry_authorized: false,
            telemetry_error: Some("Gateway manager readback is not connected yet.".into()),
            manager_paired: false,
            pacing_applied: false,
            pacing_error: Some(
                "Pacing is saved as a preference until Gateway and the worker acknowledge it."
                    .into(),
            ),
        }
    }
}

/// A close is authorized only by the authenticated adapter, never by a UI caller.
pub(crate) enum CloseAuthorization {
    NotAcknowledged(String),
    Safe,
}

/// Safety is decided inside the authenticated adapter, never by a UI caller.
pub(crate) trait ManagerGateway: Send + Sync {
    /// Cached authenticated readback for the listed profiles. Must return promptly; the adapter
    /// refreshes asynchronously and leaves fields unknown while a read is outstanding.
    fn read(&self, profiles: &[GatewayProfile]) -> HashMap<String, GatewayReading>;
    fn support(&self) -> GatewaySupport;
    /// Called on explicit open. The adapter may request `dispatchEnabled: true` through its
    /// authenticated manager control only for an enabled profile whose bound workers have no
    /// held or durably uncertain job. Gateway's account enable stays a separate state and never
    /// starts spending. The default does nothing.
    fn open_requested(&self, _profile: &GatewayProfile) {}
    /// Called while moving a profile to close: the adapter issues control for both paired worker
    /// ids with `dispatchEnabled: false` and `closeRequested: true`, then returns `Safe` only
    /// after a fresh paused acknowledgement plus `safeToClose`. Missing worker bindings must
    /// return `NotAcknowledged`; no local flag may override that. Must return promptly.
    fn close_authorization(&self, profile: &GatewayProfile) -> CloseAuthorization;
}

static AUTHORITY: OnceLock<Arc<dyn ManagerGateway>> = OnceLock::new();

/// Parent-owned integration point; no network, token or account code lives here.
#[allow(dead_code)]
pub(crate) fn install_gateway(authority: Arc<dyn ManagerGateway>) -> Result<(), String> {
    AUTHORITY
        .set(authority)
        .map_err(|_| "A Gateway manager authority is already installed.".to_string())
}

fn authority() -> Option<&'static Arc<dyn ManagerGateway>> {
    AUTHORITY.get()
}

fn close_authorization(profile: &GatewayProfile) -> CloseAuthorization {
    match authority() {
        Some(authority) => authority.close_authorization(profile),
        None => CloseAuthorization::NotAcknowledged(
            "Waiting for Gateway's authenticated manager safety readback; no managed Chrome process was stopped."
                .into(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Live adapter: ManagerView -> GatewayReading, cached so the UI never blocks
// ---------------------------------------------------------------------------

struct LiveGateway {
    app: tauri::AppHandle,
    cache: Arc<Mutex<GatewayCache>>,
}

#[derive(Default)]
struct GatewayCache {
    view: Option<manager_gateway::ManagerView>,
    error: Option<String>,
    refreshing: bool,
    last_view: Option<Instant>,
    closes: HashMap<String, CloseState>,
}

#[derive(Default)]
struct CloseState {
    in_flight: bool,
    last_attempt: Option<Instant>,
}

impl LiveGateway {
    fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            cache: Arc::new(Mutex::new(GatewayCache::default())),
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, GatewayCache> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn refresh(&self) {
        let origin = gateway_origin(&self.app);
        let mut cache = self.cache();
        if cache.refreshing {
            return;
        }
        let Some(origin) = origin else {
            cache.error = Some("Save a Gateway address first.".into());
            return;
        };
        cache.refreshing = true;
        drop(cache);
        let shared_cache = self.cache.clone();
        let _ = tauri::async_runtime::spawn(async move {
            let result = manager_gateway::read(&origin).await;
            let mut cache = shared_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.refreshing = false;
            match result {
                Ok(view) => {
                    cache.view = Some(view);
                    cache.error = None;
                    cache.last_view = Some(Instant::now());
                }
                Err(error) => cache.error = Some(error.to_string()),
            }
        });
    }

    fn remember(&self, view: manager_gateway::ManagerView) {
        let mut cache = self.cache();
        cache.view = Some(view);
        cache.error = None;
        cache.last_view = Some(Instant::now());
    }

    fn forget(&self) {
        let mut cache = self.cache();
        cache.view = None;
        cache.error = None;
        cache.last_view = None;
        cache.closes.clear();
    }
}

impl ManagerGateway for LiveGateway {
    fn read(&self, profiles: &[GatewayProfile]) -> HashMap<String, GatewayReading> {
        let stale = {
            let cache = self.cache();
            cache.view.is_none()
                || cache
                    .last_view
                    .map_or(true, |at| at.elapsed() >= Duration::from_secs(5))
        };
        if stale {
            self.refresh();
        }
        let view = self.cache().view.clone();
        let Some(view) = view else {
            return HashMap::new();
        };
        profiles
            .iter()
            .map(|profile| (profile.id.clone(), reading_for(&view, profile)))
            .collect()
    }

    fn support(&self) -> GatewaySupport {
        let manager_paired = gateway_origin(&self.app)
            .is_some_and(|origin| manager_gateway::paired(&origin).unwrap_or(false));
        let cache = self.cache();
        let fresh = cache
            .view
            .as_ref()
            .is_some_and(|view| view.is_fresh(epoch_now()));
        let authorized = cache.view.is_some();
        let mut telemetry_error = cache.error.clone();
        if telemetry_error.is_none() && authorized && !fresh {
            telemetry_error = Some("Gateway manager readback is stale.".into());
        }
        if telemetry_error.is_none() && !authorized {
            telemetry_error = Some("Waiting for the first Gateway manager readback.".into());
        }
        let pacing_applied = fresh
            && cache.view.as_ref().is_some_and(|view| {
                view.workers.iter().any(|worker| {
                    matches!(
                        (worker.control.pacing.as_ref(), worker.state.as_ref().and_then(|state| state.pacing.as_ref())),
                        (Some(requested), Some(observed))
                            if requested.min_seconds == observed.min_seconds
                                && requested.max_seconds == observed.max_seconds
                    )
                })
            });
        GatewaySupport {
            telemetry_authorized: authorized,
            telemetry_error,
            manager_paired,
            pacing_applied,
            pacing_error: (!pacing_applied)
                .then(|| "Waiting for a managed worker to report matching pacing.".to_string()),
        }
    }

    fn open_requested(&self, profile: &GatewayProfile) {
        let ids = worker_ids(profile);
        if ids.is_empty() {
            return;
        }
        let ready = self
            .cache()
            .view
            .as_ref()
            .is_some_and(|view| view.is_fresh(epoch_now()) && can_dispatch(view, &ids));
        if !ready {
            return;
        }
        let Some(origin) = gateway_origin(&self.app) else {
            return;
        };
        let shared_cache = self.cache.clone();
        let _ = tauri::async_runtime::spawn(async move {
            let result = manager_gateway::control(&origin, &ids, true, None, None).await;
            let mut cache = shared_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match result {
                Ok(view) => {
                    cache.view = Some(view);
                    cache.error = None;
                    cache.last_view = Some(Instant::now());
                }
                Err(error) => cache.error = Some(error.to_string()),
            }
        });
    }

    fn close_authorization(&self, profile: &GatewayProfile) -> CloseAuthorization {
        let ids = worker_ids(profile);
        if ids.is_empty() {
            return CloseAuthorization::NotAcknowledged(
                "No image or video worker is bound to this profile; close waits.".into(),
            );
        }
        let mut cache = self.cache();
        let mut waiting = None;
        if let Some(view) = cache.view.as_ref() {
            let safety = manager_gateway::close_safety(view, &ids);
            if safety.safe_to_close {
                return CloseAuthorization::Safe;
            }
            waiting = Some(close_reason(&safety));
        }
        let state = cache.closes.entry(profile.id.clone()).or_default();
        if state.in_flight
            || state
                .last_attempt
                .is_some_and(|at| at.elapsed() < Duration::from_secs(5))
        {
            return CloseAuthorization::NotAcknowledged(
                waiting
                    .unwrap_or_else(|| "Waiting for Gateway's fresh pause acknowledgement.".into()),
            );
        }
        let Some(origin) = gateway_origin(&self.app) else {
            return CloseAuthorization::NotAcknowledged("Save a Gateway address first.".into());
        };
        state.in_flight = true;
        state.last_attempt = Some(Instant::now());
        drop(cache);
        let shared_cache = self.cache.clone();
        let profile_id = profile.id.clone();
        let _ = tauri::async_runtime::spawn(async move {
            let result = manager_gateway::control(&origin, &ids, false, Some(true), None).await;
            let mut cache = shared_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match result {
                Ok(view) => {
                    cache.view = Some(view);
                    cache.error = None;
                    cache.last_view = Some(Instant::now());
                }
                Err(error) => cache.error = Some(error.to_string()),
            }
            if let Some(state) = cache.closes.get_mut(&profile_id) {
                state.in_flight = false;
            }
        });
        CloseAuthorization::NotAcknowledged(waiting.unwrap_or_else(|| {
            "Waiting for Gateway's fresh pause acknowledgement and safe-to-close readback.".into()
        }))
    }
}

fn gateway_origin(app: &tauri::AppHandle) -> Option<String> {
    super::current(app)
        .ok()
        .and_then(|settings| settings.gateway_url)
}

fn epoch_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn worker_ids(profile: &GatewayProfile) -> Vec<String> {
    let mut ids = Vec::new();
    for id in [
        profile.image_worker_id.clone(),
        profile.video_worker_id.clone(),
    ]
    .into_iter()
    .flatten()
    {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn close_reason(safety: &manager_gateway::CloseSafety) -> String {
    if let Some(reason) = &safety.reason {
        return reason.clone();
    }
    if safety.waiting_on.is_empty() {
        "Waiting for Gateway's safe-to-close readback.".into()
    } else {
        format!(
            "Waiting for Gateway safety: {}.",
            safety.waiting_on.join(", ")
        )
    }
}

fn can_dispatch(view: &manager_gateway::ManagerView, ids: &[String]) -> bool {
    ids.iter().all(|id| {
        view.workers
            .iter()
            .find(|worker| &worker.worker_id == id)
            .is_some_and(|worker| {
                worker.fresh
                    && worker.current_job.is_none()
                    && worker.held_jobs.is_empty()
                    && !worker.durable_busy
            })
    })
}

fn reading_for(view: &manager_gateway::ManagerView, profile: &GatewayProfile) -> GatewayReading {
    let ids = worker_ids(profile);
    let state = view
        .workers
        .iter()
        .find(|worker| ids.iter().any(|id| id == &worker.worker_id))
        .and_then(|worker| worker.state.as_ref());
    let account = view
        .accounts
        .iter()
        .find(|account| account.workers.iter().any(|id| ids.contains(id)));
    GatewayReading {
        connection: state.and_then(|state| state.connection.clone()),
        sign_in: state.and_then(|state| state.sign_in.clone()),
        readiness: state.and_then(|state| state.readiness.clone()),
        activity: state.and_then(|state| state.activity.clone()),
        provider: account.map(|account| account.provider.clone()),
        account_id: account.map(|account| account.account_id.clone()),
        observed_at: state
            .and_then(|state| state.observed_at)
            .map(|value| value.to_string()),
        credits: account
            .filter(|account| account.balance.is_some() || account.credit_buckets.is_some())
            .map(|account| {
                serde_json::json!({
                    "balance": account.balance.clone(),
                    "fresh": account.fresh,
                    "reset_at": account.reset_at.clone(),
                    "credit_buckets": account.credit_buckets.clone(),
                    "gateway_enabled": account.gateway_enabled,
                    "reserved": account.reserved.clone(),
                    "available": account.available,
                })
            }),
        next_eligible_at: state
            .and_then(|state| state.next_eligible_at)
            .map(|value| value.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Persisted state
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
struct StoredProfile {
    id: String,
    number: u32,
    label: String,
    enabled: bool,
    desired_open: bool,
    image_worker_id: Option<String>,
    video_worker_id: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    pid_started: Option<String>,
    #[serde(default)]
    binary: Option<String>,
    #[serde(default)]
    close_pending: bool,
    #[serde(default)]
    open_failures: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredSettings {
    #[serde(default)]
    memory_budget_mb: Option<u64>,
    #[serde(default = "default_reserve_mb")]
    reserve_mb: u64,
    #[serde(default)]
    max_active: Option<u32>,
    #[serde(default = "default_pause_min_seconds")]
    pause_min_seconds: u64,
    #[serde(default = "default_pause_max_seconds")]
    pause_max_seconds: u64,
}

fn default_reserve_mb() -> u64 {
    2048
}
fn default_pause_min_seconds() -> u64 {
    5
}
fn default_pause_max_seconds() -> u64 {
    15
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            memory_budget_mb: None,
            reserve_mb: default_reserve_mb(),
            max_active: None,
            pause_min_seconds: default_pause_min_seconds(),
            pause_max_seconds: default_pause_max_seconds(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    #[serde(default = "default_schema")]
    schema: u8,
    #[serde(default = "default_next_number")]
    next_number: u32,
    #[serde(default)]
    profiles: Vec<StoredProfile>,
    #[serde(default)]
    settings: StoredSettings,
}

fn default_schema() -> u8 {
    STATE_SCHEMA
}
fn default_next_number() -> u32 {
    1
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            schema: STATE_SCHEMA,
            next_number: 1,
            profiles: Vec::new(),
            settings: StoredSettings::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot (exact crossing names are emitted as snake_case fields)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct Snapshot {
    profiles: Vec<ProfileSnapshot>,
    memory: MemorySnapshot,
    settings: SettingsSnapshot,
    support: SupportSnapshot,
}

#[derive(Serialize)]
struct ProfileSnapshot {
    id: String,
    number: u32,
    label: String,
    enabled: bool,
    desired_open: bool,
    process_state: &'static str,
    process_id: Option<u32>,
    memory_mb: Option<u64>,
    close_pending: bool,
    error: Option<String>,
    attention: bool,
    gateway: GatewaySnapshot,
}

#[derive(Serialize)]
struct GatewaySnapshot {
    connection: String,
    sign_in: String,
    readiness: String,
    activity: String,
    provider: Option<String>,
    account_id: Option<String>,
    image_worker_id: Option<String>,
    video_worker_id: Option<String>,
    observed_at: Option<String>,
    credits: Option<serde_json::Value>,
    next_eligible_at: Option<String>,
}

#[derive(Serialize)]
struct MemorySnapshot {
    physical_mb: Option<u64>,
    available_mb: Option<u64>,
    budget_mb: Option<u64>,
    used_mb: Option<u64>,
    per_profile_mb: Option<u64>,
    queued: u32,
    reason: Option<String>,
}

#[derive(Serialize)]
struct SettingsSnapshot {
    memory_budget_mb: Option<u64>,
    reserve_mb: u64,
    max_active: Option<u32>,
    pause_min_seconds: u64,
    pause_max_seconds: u64,
}

#[derive(Serialize)]
struct SupportSnapshot {
    native: bool,
    chrome_available: bool,
    telemetry_authorized: bool,
    telemetry_error: Option<String>,
    manager_paired: bool,
    pacing_applied: bool,
    pacing_error: Option<String>,
}

// ---------------------------------------------------------------------------
// Native manager
// ---------------------------------------------------------------------------

pub(crate) struct BrowserManager {
    shared: Arc<Shared>,
}

struct Shared {
    state_path: PathBuf,
    profiles_dir: PathBuf,
    chrome: Option<PathBuf>,
    gateway: Arc<LiveGateway>,
    inner: Mutex<Inner>,
}

struct Inner {
    stored: Option<Stored>,
    load_error: Option<String>,
    errors: HashMap<String, String>,
    attention: HashSet<String>,
    measurement: Measurement,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            stored: None,
            load_error: None,
            errors: HashMap::new(),
            attention: HashSet::new(),
            measurement: Measurement::default(),
        }
    }
}

pub(crate) fn init(app: &tauri::AppHandle) -> Result<BrowserManager, String> {
    let state_path = app
        .path()
        .app_config_dir()
        .map_err(|_| "Relay's settings folder is unavailable.")?
        .join("browser-manager.json");
    let profiles_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Relay's support folder is unavailable.")?
        .join("browser-profiles");
    let chrome = chrome_binary();
    let (stored, load_error) = match load_state(&state_path) {
        Ok(Some(stored)) => (Some(stored), None),
        Ok(None) => {
            let fresh = Stored::default();
            let error = save_state(&state_path, &fresh).err();
            (Some(fresh), error)
        }
        Err(error) => (None, Some(error)),
    };
    let gateway = Arc::new(LiveGateway::new(app.clone()));
    let authority: Arc<dyn ManagerGateway> = gateway.clone();
    let _ = install_gateway(authority);
    let shared = Arc::new(Shared {
        state_path,
        profiles_dir,
        chrome,
        gateway,
        inner: Mutex::new(Inner {
            stored,
            load_error,
            ..Inner::default()
        }),
    });
    let reconciler = shared.clone();
    thread::Builder::new()
        .name("relay-browser-reconcile".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(RECONCILE_SECONDS));
            let _ = reconcile(&reconciler);
        })
        .map_err(|_| "Relay could not start its browser reconciliation.")?;
    Ok(BrowserManager { shared })
}

fn locked(shared: &Shared) -> Result<std::sync::MutexGuard<'_, Inner>, String> {
    shared
        .inner
        .lock()
        .map_err(|_| "Browser manager state is busy.".to_string())
}

fn stored_or_error(inner: &Inner) -> Result<&Stored, String> {
    match (&inner.stored, &inner.load_error) {
        (Some(stored), _) => Ok(stored),
        (None, Some(error)) => Err(error.clone()),
        (None, None) => Err("Saved browser profiles are unavailable.".into()),
    }
}

fn mutate(
    shared: &Shared,
    change: impl FnOnce(&mut Stored) -> Result<(), String>,
) -> Result<(), String> {
    let mut inner = locked(shared)?;
    let mut next = stored_or_error(&inner)?.clone();
    change(&mut next)?;
    validate_state(&next)?;
    save_state(&shared.state_path, &next)?;
    inner.stored = Some(next);
    Ok(())
}

fn finish(shared: &Shared) -> Result<Snapshot, String> {
    reconcile(shared)?;
    build_snapshot(shared)
}

// ---------------------------------------------------------------------------
// Tauri commands (local window only; remote Gateway webview cannot call these)
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) fn manager_snapshot(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    finish(&manager.shared)
}

/// Pairing is a local-window action; the raw token is handed to the auth module and never
/// stored, logged or placed in the saved snapshot. Returns the normal manager snapshot.
#[tauri::command]
pub(crate) async fn manager_pair_gateway(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    origin: String,
    token: String,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    let shared = manager.shared.clone();
    drop(manager);
    let view = manager_gateway::pair(&origin, &token)
        .await
        .map_err(|error| error.to_string())?;
    shared.gateway.remember(view);
    build_snapshot(&shared)
}

#[tauri::command]
pub(crate) fn manager_clear_gateway(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    origin: String,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    manager_gateway::manager_clear_gateway(window.clone(), origin)?;
    manager.shared.gateway.forget();
    build_snapshot(&manager.shared)
}

#[tauri::command]
pub(crate) fn manager_add_profiles(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    labels: Vec<String>,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    if labels.is_empty() {
        return Err("Add at least one browser profile label.".into());
    }
    mutate(&manager.shared, |stored| {
        for label in &labels {
            append_profile(stored, label)?;
        }
        Ok(())
    })?;
    finish(&manager.shared)
}

#[tauri::command]
pub(crate) fn manager_update_profile(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    profile_id: String,
    label: Option<String>,
    enabled: Option<bool>,
    image_worker_id: Option<String>,
    video_worker_id: Option<String>,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    mutate(&manager.shared, |stored| {
        let profile = stored
            .profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or("Unknown browser profile.")?;
        if let Some(label) = &label {
            profile.label = validate_label(label)?;
        }
        if let Some(enabled) = enabled {
            profile.enabled = enabled;
            if !enabled && profile.desired_open {
                profile.desired_open = false;
                if profile.pid.is_some() {
                    profile.close_pending = true;
                }
            }
        }
        if let Some(worker) = image_worker_id {
            profile.image_worker_id = normalize_worker(worker)?;
        }
        if let Some(worker) = video_worker_id {
            profile.video_worker_id = normalize_worker(worker)?;
        }
        Ok(())
    })?;
    finish(&manager.shared)
}

#[tauri::command]
pub(crate) fn manager_open_profiles(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    profile_ids: Vec<String>,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    let mut rejected: Vec<(String, String)> = Vec::new();
    let mut opened: Vec<GatewayProfile> = Vec::new();
    mutate(&manager.shared, |stored| {
        for id in &profile_ids {
            let profile = stored
                .profiles
                .iter_mut()
                .find(|profile| &profile.id == id)
                .ok_or("Unknown browser profile.")?;
            if !profile.enabled {
                rejected.push((profile.id.clone(), "This profile is disabled.".into()));
                continue;
            }
            if profile.close_pending {
                rejected.push((
                    profile.id.clone(),
                    "Waiting for Gateway safety before this profile can reopen.".into(),
                ));
                continue;
            }
            // Open intent is saved before any process starts, so a restart restores it.
            profile.desired_open = true;
            profile.open_failures = 0;
            opened.push(GatewayProfile {
                id: profile.id.clone(),
                image_worker_id: profile.image_worker_id.clone(),
                video_worker_id: profile.video_worker_id.clone(),
            });
        }
        Ok(())
    })?;
    {
        let mut inner = locked(&manager.shared)?;
        for (id, reason) in rejected {
            inner.errors.insert(id.clone(), reason);
            inner.attention.remove(&id);
        }
    }
    let snapshot = finish(&manager.shared)?;
    if let Some(authority) = authority() {
        for profile in &opened {
            // Only a profile whose real Chrome process actually started may ask for dispatch.
            let running = snapshot
                .profiles
                .iter()
                .find(|entry| entry.id == profile.id)
                .is_some_and(|entry| matches!(entry.process_state, "open" | "opening"));
            if running {
                authority.open_requested(profile);
            }
        }
    }
    Ok(snapshot)
}

#[tauri::command]
pub(crate) fn manager_close_profiles(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    profile_ids: Vec<String>,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    mutate(&manager.shared, |stored| {
        for id in &profile_ids {
            let profile = stored
                .profiles
                .iter_mut()
                .find(|profile| &profile.id == id)
                .ok_or("Unknown browser profile.")?;
            profile.desired_open = false;
            profile.open_failures = 0;
            profile.close_pending = profile.pid.is_some();
        }
        Ok(())
    })?;
    finish(&manager.shared)
}

#[tauri::command]
pub(crate) fn manager_save_settings(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    memory_budget_mb: Option<u64>,
    reserve_mb: u64,
    max_active: Option<u32>,
    pause_min_seconds: u64,
    pause_max_seconds: u64,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    mutate(&manager.shared, |stored| {
        stored.settings = StoredSettings {
            memory_budget_mb,
            reserve_mb,
            max_active,
            pause_min_seconds,
            pause_max_seconds,
        };
        Ok(())
    })?;
    finish(&manager.shared)
}

// ---------------------------------------------------------------------------
// Snapshot assembly
// ---------------------------------------------------------------------------

fn build_snapshot(shared: &Shared) -> Result<Snapshot, String> {
    let (stored, errors, attention, measurement) = {
        let inner = locked(shared)?;
        let stored = stored_or_error(&inner)?.clone();
        (
            stored,
            inner.errors.clone(),
            inner.attention.clone(),
            inner.measurement.clone(),
        )
    };
    let gateway_profiles: Vec<GatewayProfile> = stored
        .profiles
        .iter()
        .map(|profile| GatewayProfile {
            id: profile.id.clone(),
            image_worker_id: profile.image_worker_id.clone(),
            video_worker_id: profile.video_worker_id.clone(),
        })
        .collect();
    let (readings, support) = match authority() {
        Some(authority) => (authority.read(&gateway_profiles), authority.support()),
        None => (HashMap::new(), GatewaySupport::unavailable()),
    };
    let mut queued = 0u32;
    let profiles = stored
        .profiles
        .iter()
        .map(|profile| {
            let alive = measurement.alive.contains(&profile.id);
            let state = process_state(profile, alive);
            if state == "queued" {
                queued = queued.saturating_add(1);
            }
            ProfileSnapshot {
                id: profile.id.clone(),
                number: profile.number,
                label: profile.label.clone(),
                enabled: profile.enabled,
                desired_open: profile.desired_open,
                process_state: state,
                process_id: if alive { profile.pid } else { None },
                memory_mb: measurement.profile_mb.get(&profile.id).copied(),
                close_pending: profile.close_pending,
                error: errors.get(&profile.id).cloned(),
                attention: attention.contains(&profile.id),
                gateway: gateway_snapshot(profile, readings.get(&profile.id)),
            }
        })
        .collect();
    let reason = if !measurement.processes_ok {
        Some("Process memory cannot be measured, so no profile was started.".into())
    } else if measurement.available_mb.is_none() {
        Some("Available memory cannot be measured, so no profile was started.".into())
    } else if queued > 0 {
        Some(format!(
            "{queued} profile(s) queued for measured memory headroom."
        ))
    } else {
        None
    };
    Ok(Snapshot {
        profiles,
        memory: MemorySnapshot {
            physical_mb: measurement.physical_mb,
            available_mb: measurement.available_mb,
            budget_mb: stored.settings.memory_budget_mb,
            used_mb: measurement.used_mb,
            per_profile_mb: measurement.per_profile_mb,
            queued,
            reason,
        },
        settings: SettingsSnapshot {
            memory_budget_mb: stored.settings.memory_budget_mb,
            reserve_mb: stored.settings.reserve_mb,
            max_active: stored.settings.max_active,
            pause_min_seconds: stored.settings.pause_min_seconds,
            pause_max_seconds: stored.settings.pause_max_seconds,
        },
        support: SupportSnapshot {
            native: true,
            chrome_available: shared.chrome.is_some(),
            telemetry_authorized: support.telemetry_authorized,
            telemetry_error: support.telemetry_error,
            manager_paired: support.manager_paired,
            pacing_applied: support.pacing_applied,
            pacing_error: support.pacing_error,
        },
    })
}

fn process_state(profile: &StoredProfile, alive: bool) -> &'static str {
    if profile.pid.is_some() {
        if !alive {
            if profile.pid_started.is_none() {
                "opening"
            } else {
                "error"
            }
        } else if profile.close_pending && !profile.desired_open {
            "closing"
        } else if profile.pid_started.is_none() {
            "opening"
        } else {
            "open"
        }
    } else if profile.desired_open {
        if profile.open_failures >= MAX_OPEN_ATTEMPTS {
            "error"
        } else {
            "queued"
        }
    } else {
        "closed"
    }
}

fn gateway_snapshot(profile: &StoredProfile, reading: Option<&GatewayReading>) -> GatewaySnapshot {
    let reading = reading.cloned().unwrap_or_default();
    GatewaySnapshot {
        connection: reading.connection.unwrap_or_else(|| "unknown".into()),
        sign_in: reading.sign_in.unwrap_or_else(|| "unknown".into()),
        readiness: reading.readiness.unwrap_or_else(|| "unknown".into()),
        activity: reading.activity.unwrap_or_else(|| "unknown".into()),
        provider: reading.provider,
        account_id: reading.account_id,
        image_worker_id: profile.image_worker_id.clone(),
        video_worker_id: profile.video_worker_id.clone(),
        observed_at: reading.observed_at,
        credits: reading.credits,
        next_eligible_at: reading.next_eligible_at,
    }
}

// ---------------------------------------------------------------------------
// Reconciliation: ownership, admission, graceful close
// ---------------------------------------------------------------------------

fn reconcile(shared: &Shared) -> Result<(), String> {
    let mut guard = locked(shared)?;
    let load_error = guard.load_error.clone();
    let Some(mut stored) = guard.stored.clone() else {
        return Err(load_error.unwrap_or_else(|| "Saved browser profiles are unavailable.".into()));
    };
    let chrome = shared.chrome.clone();
    let mut measurement = measure(&stored, chrome.as_deref());
    let mut errors = std::mem::take(&mut guard.errors);
    let mut attention = std::mem::take(&mut guard.attention);
    let mut changed = false;

    for profile in stored.profiles.iter_mut() {
        let row = profile
            .pid
            .and_then(|pid| measurement.rows.get(&pid).cloned());
        let owned = row
            .as_ref()
            .is_some_and(|row| owned_process(row, profile, chrome.as_deref()));
        if profile.pid.is_some() && !owned {
            let wanted = profile.desired_open;
            profile.pid = None;
            profile.pid_started = None;
            profile.binary = None;
            changed = true;
            if wanted && !profile.close_pending {
                profile.open_failures = profile.open_failures.saturating_add(1);
                errors.insert(profile.id.clone(), exit_message(profile.open_failures));
                if profile.open_failures >= MAX_OPEN_ATTEMPTS {
                    attention.insert(profile.id.clone());
                }
            } else {
                profile.close_pending = false;
                errors.remove(&profile.id);
                attention.remove(&profile.id);
            }
        } else if let Some(row) = row {
            if profile.pid_started.is_none() {
                profile.pid_started = Some(row.started.clone());
                changed = true;
            }
            if profile.open_failures != 0 {
                profile.open_failures = 0;
                changed = true;
                errors.remove(&profile.id);
                attention.remove(&profile.id);
            }
        }
        if profile.close_pending {
            if profile.pid.is_none() {
                profile.close_pending = false;
                changed = true;
                errors.remove(&profile.id);
            } else if owned {
                let input = GatewayProfile {
                    id: profile.id.clone(),
                    image_worker_id: profile.image_worker_id.clone(),
                    video_worker_id: profile.video_worker_id.clone(),
                };
                match close_action(true, close_authorization(&input)) {
                    CloseAction::Terminate => match terminate(profile.pid.unwrap_or_default()) {
                        Ok(()) => {
                            errors.insert(
                                profile.id.clone(),
                                "Waiting for Chrome to finish its graceful close.".into(),
                            );
                        }
                        Err(reason) => {
                            errors.insert(profile.id.clone(), reason);
                        }
                    },
                    CloseAction::Wait(reason) => {
                        errors.insert(profile.id.clone(), reason);
                    }
                }
            }
        }
    }

    let settings = stored.settings.clone();
    let mut resident = measurement.alive.len() as u32;
    let mut used = measurement.used_mb;
    let mut estimate = measurement.per_profile_mb;
    let mut probe_opened = false;
    let mut launched = false;
    if let Some(chrome_path) = chrome.as_deref() {
        for index in 0..stored.profiles.len() {
            if probe_opened {
                break;
            }
            let profile = &stored.profiles[index];
            if !profile.desired_open || profile.pid.is_some() || profile.close_pending {
                continue;
            }
            if profile.open_failures >= MAX_OPEN_ATTEMPTS {
                errors.insert(profile.id.clone(), exit_message(profile.open_failures));
                attention.insert(profile.id.clone());
                continue;
            }
            let admission = Admission {
                max_active: settings.max_active,
                available_mb: measurement.available_mb,
                reserve_mb: settings.reserve_mb,
                budget_mb: settings.memory_budget_mb,
                used_mb: used,
                per_profile_mb: estimate,
            };
            match admission_reason(&admission, resident) {
                Err(reason) => {
                    errors.insert(profile.id.clone(), reason);
                    attention.remove(&profile.id);
                }
                Ok(()) => {
                    let id = profile.id.clone();
                    match launch(shared, &mut stored.profiles[index], chrome_path) {
                        Ok(()) => {
                            changed = true;
                            launched = true;
                            errors.remove(&id);
                            attention.remove(&id);
                            resident = resident.saturating_add(1);
                            match estimate {
                                Some(value) => {
                                    used = used.map(|current| current.saturating_add(value));
                                }
                                None => probe_opened = true,
                            }
                        }
                        Err(reason) => {
                            errors.insert(id.clone(), reason);
                            attention.insert(id);
                        }
                    }
                }
            }
        }
    } else {
        for profile in stored.profiles.iter_mut() {
            if profile.desired_open && profile.pid.is_none() && !profile.close_pending {
                errors.insert(
                    profile.id.clone(),
                    "Google Chrome was not found in Applications; no profile was started.".into(),
                );
            }
        }
    }

    if launched {
        measurement = measure(&stored, chrome.as_deref());
        // Capture the exact start time observed for each freshly started process.
        for profile in stored.profiles.iter_mut() {
            let Some(pid) = profile.pid else { continue };
            if profile.pid_started.is_some() {
                continue;
            }
            if let Some(row) = measurement.rows.get(&pid) {
                let expected = profile
                    .binary
                    .as_deref()
                    .is_some_and(|binary| binary == row.comm);
                if expected {
                    profile.pid_started = Some(row.started.clone());
                    changed = true;
                }
            }
        }
    }
    let saved = if changed {
        save_state(&shared.state_path, &stored)
    } else {
        Ok(())
    };
    guard.stored = Some(stored);
    guard.measurement = measurement;
    guard.errors = errors;
    guard.attention = attention;
    saved
}

enum CloseAction {
    Wait(String),
    Terminate,
}

fn close_action(owned_process: bool, authorization: CloseAuthorization) -> CloseAction {
    if !owned_process {
        return CloseAction::Wait(
            "The recorded Chrome process could not be verified as Relay's own; it was not stopped."
                .into(),
        );
    }
    match authorization {
        CloseAuthorization::Safe => CloseAction::Terminate,
        CloseAuthorization::NotAcknowledged(reason) => CloseAction::Wait(reason),
    }
}

fn terminate(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("Managed Chrome process identity is incomplete; it was not stopped.".into());
    }
    // Graceful only. Relay never sends SIGKILL or replays work.
    let target = pid.to_string();
    let status = Command::new("/bin/kill")
        .args(["-TERM", target.as_str()])
        .status()
        .map_err(|_| {
            "Chrome did not accept a graceful close request; it was not forced.".to_string()
        })?;
    if status.success() {
        Ok(())
    } else {
        Err("Chrome did not accept a graceful close request; it was not forced.".into())
    }
}

fn exit_message(attempts: u32) -> String {
    if attempts >= MAX_OPEN_ATTEMPTS {
        "Chrome exited repeatedly; open this profile again deliberately.".into()
    } else {
        "Chrome exited unexpectedly; Relay will reopen it when measured headroom allows.".into()
    }
}

// ---------------------------------------------------------------------------
// Measured capacity
// ---------------------------------------------------------------------------

struct Admission {
    max_active: Option<u32>,
    available_mb: Option<u64>,
    reserve_mb: u64,
    budget_mb: Option<u64>,
    used_mb: Option<u64>,
    per_profile_mb: Option<u64>,
}

fn admission_reason(admission: &Admission, resident: u32) -> Result<(), String> {
    if let Some(max) = admission.max_active {
        if resident >= max {
            return Err(format!(
                "Waiting for a free resident slot ({resident}/{max} open)."
            ));
        }
    }
    let Some(available) = admission.available_mb else {
        return Err("Available memory cannot be measured, so no profile was started.".into());
    };
    let Some(used) = admission.used_mb else {
        return Err("Process memory cannot be measured, so no profile was started.".into());
    };
    let mut room = available.saturating_sub(admission.reserve_mb);
    if let Some(budget) = admission.budget_mb {
        if used >= budget {
            return Err("Relay's memory budget is full.".into());
        }
        room = room.min(budget - used);
    }
    match admission.per_profile_mb {
        Some(estimate) if room < estimate => Err(format!(
            "Waiting for measured memory headroom: {room} MB free for Relay, {estimate} MB needed."
        )),
        Some(_) => Ok(()),
        None if room == 0 => Err("Waiting for memory headroom above the reserve.".into()),
        // No profile has been measured yet: one probe is admitted, then its measurement governs.
        None => Ok(()),
    }
}

#[derive(Clone, Default)]
struct Measurement {
    physical_mb: Option<u64>,
    available_mb: Option<u64>,
    used_mb: Option<u64>,
    per_profile_mb: Option<u64>,
    profile_mb: HashMap<String, u64>,
    rows: HashMap<u32, ProcessRow>,
    alive: HashSet<String>,
    processes_ok: bool,
}

#[derive(Clone)]
struct ProcessRow {
    ppid: u32,
    rss_kb: u64,
    started: String,
    comm: String,
}

#[derive(Default)]
struct ProcessTable {
    rows: HashMap<u32, ProcessRow>,
    children: HashMap<u32, Vec<u32>>,
}

fn measure(stored: &Stored, chrome: Option<&Path>) -> Measurement {
    let physical_mb = Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map(|bytes| bytes / (1024 * 1024));
    let available_mb = Command::new("/usr/bin/vm_stat")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| parse_vm_stat(&String::from_utf8_lossy(&output.stdout)));
    let table = process_table();
    let processes_ok = table.is_ok();
    let table = table.unwrap_or_default();
    let mut alive = HashSet::new();
    let mut profile_mb = HashMap::new();
    let mut used = 0u64;
    for profile in &stored.profiles {
        let Some(pid) = profile.pid else { continue };
        let Some(row) = table.rows.get(&pid) else {
            continue;
        };
        if !owned_process(row, profile, chrome) {
            continue;
        }
        alive.insert(profile.id.clone());
        let mb = subtree_mb(&table, pid);
        profile_mb.insert(profile.id.clone(), mb);
        used = used.saturating_add(mb);
    }
    let per_profile_mb = if alive.is_empty() {
        None
    } else {
        Some(used / alive.len() as u64)
    };
    Measurement {
        physical_mb,
        available_mb,
        used_mb: processes_ok.then_some(used),
        per_profile_mb,
        profile_mb,
        rows: table.rows,
        alive,
        processes_ok,
    }
}

fn parse_vm_stat(text: &str) -> Option<u64> {
    let page_size = text
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    let mut pages = 0u64;
    let mut found = false;
    for line in text.lines() {
        for key in ["Pages free:", "Pages inactive:", "Pages speculative:"] {
            if let Some(value) = line.strip_prefix(key) {
                if let Ok(value) = value.trim().trim_end_matches('.').parse::<u64>() {
                    pages = pages.saturating_add(value);
                    found = true;
                }
            }
        }
    }
    if !found {
        return None;
    }
    Some(pages.saturating_mul(page_size) / (1024 * 1024))
}

fn process_table() -> Result<ProcessTable, String> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,rss=,lstart=,comm="])
        .output()
        .map_err(|_| "Process table is unavailable.".to_string())?;
    if !output.status.success() {
        return Err("Process table is unavailable.".into());
    }
    Ok(parse_ps(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_ps(text: &str) -> ProcessTable {
    let mut rows = HashMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 9 {
            continue;
        }
        let (Ok(pid), Ok(ppid), Ok(rss_kb)) = (
            fields[0].parse::<u32>(),
            fields[1].parse::<u32>(),
            fields[2].parse::<u64>(),
        ) else {
            continue;
        };
        rows.insert(
            pid,
            ProcessRow {
                ppid,
                rss_kb,
                started: fields[3..8].join(" "),
                comm: fields[8..].join(" "),
            },
        );
    }
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, row) in &rows {
        children.entry(row.ppid).or_default().push(*pid);
    }
    ProcessTable { rows, children }
}

fn subtree_mb(table: &ProcessTable, root: u32) -> u64 {
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    let mut rss_kb = 0u64;
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(row) = table.rows.get(&pid) {
            rss_kb = rss_kb.saturating_add(row.rss_kb);
        }
        if let Some(children) = table.children.get(&pid) {
            stack.extend(children.iter().copied());
        }
    }
    rss_kb / 1024
}

fn owned_process(row: &ProcessRow, profile: &StoredProfile, chrome: Option<&Path>) -> bool {
    let expected = profile
        .binary
        .clone()
        .or_else(|| chrome.map(|path| path.to_string_lossy().into_owned()));
    if expected.as_deref() != Some(row.comm.as_str()) {
        return false;
    }
    match profile.pid_started.as_deref() {
        Some(started) => started == row.started,
        None => true,
    }
}

fn launch(shared: &Shared, profile: &mut StoredProfile, chrome: &Path) -> Result<(), String> {
    let directory = profile_dir(shared, &profile.id)?;
    let child = Command::new(chrome)
        .arg(format!("--user-data-dir={}", directory.display()))
        .spawn()
        .map_err(|_| "Could not start Google Chrome for this profile.".to_string())?;
    profile.pid = Some(child.id());
    profile.binary = Some(chrome.to_string_lossy().into_owned());
    profile.pid_started = None;
    // Reap the child so an exited Chrome does not linger as a zombie; state comes from the table.
    thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

fn profile_dir(shared: &Shared, id: &str) -> Result<PathBuf, String> {
    ensure_plain_directory(&shared.profiles_dir)?;
    let directory = shared.profiles_dir.join(id);
    ensure_plain_directory(&directory)?;
    Ok(directory)
}

fn ensure_plain_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(info) if info.file_type().is_symlink() || !info.is_dir() => Err(
            "Relay's browser profile folder is not a plain directory; it was not changed.".into(),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)
            .map_err(|_| "Could not create Relay's browser profile folder.".into()),
        Err(_) => {
            Err("Relay's browser profile folder could not be read; it was not changed.".into())
        }
    }
}

fn chrome_binary() -> Option<PathBuf> {
    let relative = Path::new("Google Chrome.app/Contents/MacOS/Google Chrome");
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    for root in roots {
        let candidate = root.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // Spotlight supplements the standard folders, matching the existing browser detection.
    let output = Command::new("/usr/bin/mdfind")
        .arg("kMDItemCFBundleIdentifier == 'com.google.Chrome'")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| Path::new(line).join("Contents/MacOS/Google Chrome"))
        .find(|path| path.is_file())
}

// ---------------------------------------------------------------------------
// State validation and persistence
// ---------------------------------------------------------------------------

fn append_profile(stored: &mut Stored, raw_label: &str) -> Result<String, String> {
    let label = validate_label(raw_label)?;
    let number = stored.next_number;
    stored.next_number = stored
        .next_number
        .checked_add(1)
        .ok_or("No more stable browser profile numbers are available.")?;
    let id = new_id(&stored.profiles, number);
    stored.profiles.push(StoredProfile {
        id: id.clone(),
        number,
        label,
        enabled: true,
        desired_open: false,
        image_worker_id: None,
        video_worker_id: None,
        pid: None,
        pid_started: None,
        binary: None,
        close_pending: false,
        open_failures: 0,
    });
    Ok(id)
}

fn new_id(existing: &[StoredProfile], number: u32) -> String {
    let mut counter = 0u64;
    loop {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let seed = format!("{nanos}:{}:{number}:{counter}", std::process::id());
        let digest = format!("{:x}", Sha256::digest(seed.as_bytes()));
        let id = digest[..24].to_string();
        if !existing.iter().any(|profile| profile.id == id) {
            return id;
        }
        counter = counter.saturating_add(1);
    }
}

fn validate_label(value: &str) -> Result<String, String> {
    let label = value.trim();
    if label.is_empty() || label.chars().count() > PROFILE_LABEL_LIMIT {
        return Err(format!(
            "Use a profile label of 1 to {PROFILE_LABEL_LIMIT} characters."
        ));
    }
    if label.chars().any(char::is_control) {
        return Err("Profile labels cannot contain control characters.".into());
    }
    Ok(label.to_string())
}

fn normalize_worker(value: String) -> Result<Option<String>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    validate_worker(Some(value))?;
    Ok(Some(value.to_string()))
}

fn validate_worker(value: Option<&str>) -> Result<(), String> {
    let Some(value) = value else { return Ok(()) };
    if value.is_empty() || value.chars().count() > WORKER_ID_LIMIT {
        return Err(format!(
            "Worker identifiers must be 1 to {WORKER_ID_LIMIT} characters."
        ));
    }
    if value.chars().any(char::is_control) {
        return Err("Worker identifiers cannot contain control characters.".into());
    }
    Ok(())
}

fn validate_state(stored: &Stored) -> Result<(), String> {
    if stored.schema != STATE_SCHEMA {
        return Err(
            "Saved browser profile state uses an unsupported version; it was not changed.".into(),
        );
    }
    let mut ids = HashSet::new();
    let mut numbers = HashSet::new();
    let mut highest = 0u32;
    for profile in &stored.profiles {
        if profile.id.len() != 24
            || !profile.id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !ids.insert(profile.id.clone())
        {
            return Err(
                "A saved browser profile identifier is invalid; the file was not changed.".into(),
            );
        }
        if profile.number == 0 || !numbers.insert(profile.number) {
            return Err(
                "Saved browser profile numbers are invalid; the file was not changed.".into(),
            );
        }
        highest = highest.max(profile.number);
        validate_label(&profile.label)?;
        validate_worker(profile.image_worker_id.as_deref())?;
        validate_worker(profile.video_worker_id.as_deref())?;
        if profile.pid == Some(0) {
            return Err(
                "A saved browser process identifier is invalid; the file was not changed.".into(),
            );
        }
    }
    if stored.next_number == 0 || stored.next_number <= highest {
        return Err("Saved browser profile numbering is invalid; the file was not changed.".into());
    }
    if stored.settings.pause_min_seconds > stored.settings.pause_max_seconds {
        return Err("Saved pause range is invalid; the file was not changed.".into());
    }
    Ok(())
}

fn parse_state(bytes: &[u8]) -> Result<Stored, String> {
    let stored: Stored = serde_json::from_slice(bytes).map_err(|_| {
        "Saved browser profiles are unreadable. The file has not been reset.".to_string()
    })?;
    validate_state(&stored)?;
    Ok(stored)
}

fn load_state(path: &Path) -> Result<Option<Stored>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(parse_state(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(
            "Saved browser profiles could not be read. Existing choices have not been reset."
                .into(),
        ),
    }
}

fn save_state(path: &Path, stored: &Stored) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Relay's settings folder is unavailable.")?;
    fs::create_dir_all(parent).map_err(|_| "Could not create Relay's settings folder.")?;
    let bytes = serde_json::to_vec_pretty(stored)
        .map_err(|_| "Could not encode browser profile settings.")?;
    let temporary = path.with_extension("pending");
    fs::write(&temporary, bytes).map_err(|_| {
        "Could not save browser profile settings; existing choices are retained.".to_string()
    })?;
    fs::rename(&temporary, path).map_err(|_| {
        "Could not replace browser profile settings; existing choices are retained.".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_profile() -> StoredProfile {
        StoredProfile {
            id: "0123456789abcdef01234567".into(),
            number: 1,
            label: "Profile 1".into(),
            enabled: true,
            desired_open: false,
            image_worker_id: Some("image-worker-1".into()),
            video_worker_id: None,
            pid: None,
            pid_started: None,
            binary: None,
            close_pending: false,
            open_failures: 0,
        }
    }

    fn admission() -> Admission {
        Admission {
            max_active: None,
            available_mb: Some(4096),
            reserve_mb: 1024,
            budget_mb: None,
            used_mb: Some(512),
            per_profile_mb: Some(512),
        }
    }

    #[test]
    fn vm_stat_counts_free_inactive_and_speculative_with_page_size() {
        let text = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                    Pages free: 100.\n\
                    Pages inactive: 200.\n\
                    Pages speculative: 50.\n";
        assert_eq!(parse_vm_stat(text), Some(350 * 16384 / (1024 * 1024)));
        assert_eq!(parse_vm_stat("no page statistics here"), None);
    }

    #[test]
    fn ps_parser_keeps_chrome_start_time_and_spaced_command() {
        let text = "   42     1   2048 Wed Nov 11 00:42:11 2026 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome\n\
                       43    42   1024 Wed Nov 11 00:42:12 2026 /Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Helper (Renderer)\n";
        let table = parse_ps(text);
        let root = table.rows.get(&42).expect("root process");
        assert_eq!(root.started, "Wed Nov 11 00:42:11 2026");
        assert_eq!(
            root.comm,
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        );
        assert!(table.rows[&43u32]
            .comm
            .ends_with("Google Chrome Helper (Renderer)"));
        assert_eq!(subtree_mb(&table, 42), 3);
    }

    #[test]
    fn ownership_requires_exact_binary_and_recorded_start_time() {
        let row = ProcessRow {
            ppid: 1,
            rss_kb: 1024,
            started: "Wed Nov 11 00:42:11 2026".into(),
            comm: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
        };
        let mut profile = sample_profile();
        profile.binary = Some(row.comm.clone());
        profile.pid_started = Some(row.started.clone());
        assert!(owned_process(&row, &profile, None));

        profile.pid_started = Some("Thu Nov 12 00:42:11 2026".into());
        assert!(
            !owned_process(&row, &profile, None),
            "a reused pid must not match"
        );

        profile.pid_started = Some(row.started.clone());
        profile.binary = Some("/Applications/Other.app/Contents/MacOS/Other".into());
        assert!(!owned_process(&row, &profile, None));
    }

    #[test]
    fn closing_never_terminates_without_owned_process_and_fresh_safety() {
        assert!(matches!(
            close_action(true, CloseAuthorization::NotAcknowledged("waiting".into())),
            CloseAction::Wait(_)
        ));
        assert!(matches!(
            close_action(false, CloseAuthorization::Safe),
            CloseAction::Wait(_)
        ));
        assert!(matches!(
            close_action(true, CloseAuthorization::Safe),
            CloseAction::Terminate
        ));
    }

    #[test]
    fn admission_queues_on_ceiling_budget_unknown_measurement_and_headroom() {
        let mut policy = admission();
        policy.max_active = Some(1);
        assert!(admission_reason(&policy, 1).is_err());

        let mut policy = admission();
        policy.budget_mb = Some(512);
        policy.used_mb = Some(512);
        assert!(admission_reason(&policy, 0).is_err());

        let mut policy = admission();
        policy.available_mb = None;
        assert!(admission_reason(&policy, 0).is_err());

        let mut policy = admission();
        policy.used_mb = None;
        assert!(admission_reason(&policy, 0).is_err());

        let mut policy = admission();
        policy.per_profile_mb = Some(4000);
        policy.available_mb = Some(4500);
        assert!(admission_reason(&policy, 0).is_err());

        assert!(admission_reason(&admission(), 0).is_ok());

        // No measured profile yet: one probe is allowed with any positive headroom.
        let mut policy = admission();
        policy.per_profile_mb = None;
        policy.available_mb = Some(1025);
        policy.reserve_mb = 1024;
        assert!(admission_reason(&policy, 0).is_ok());
        policy.available_mb = Some(1024);
        assert!(admission_reason(&policy, 0).is_err());
    }

    #[test]
    fn new_profiles_keep_stable_ordinals_and_start_disabled_for_opening() {
        let mut stored = Stored::default();
        let first = append_profile(&mut stored, "First").expect("first profile");
        let second = append_profile(&mut stored, "Second").expect("second profile");
        assert_ne!(first, second);
        assert_eq!(stored.profiles[0].number, 1);
        assert_eq!(stored.profiles[1].number, 2);
        assert_eq!(stored.next_number, 3);
        assert!(stored.profiles.iter().all(|profile| profile.enabled));
        assert!(stored
            .profiles
            .iter()
            .all(|profile| !profile.desired_open && !profile.close_pending));
        assert!(validate_state(&stored).is_ok());
    }

    #[test]
    fn unknown_gateway_stays_unknown_and_never_infers_account() {
        let profile = sample_profile();
        let gateway = gateway_snapshot(&profile, None);
        assert_eq!(gateway.connection, "unknown");
        assert_eq!(gateway.sign_in, "unknown");
        assert_eq!(gateway.readiness, "unknown");
        assert_eq!(gateway.activity, "unknown");
        assert!(gateway.account_id.is_none());
        assert!(gateway.credits.is_none());
        assert!(gateway.observed_at.is_none());
        assert_eq!(gateway.image_worker_id.as_deref(), Some("image-worker-1"));
    }

    #[test]
    fn process_state_reports_queue_crash_limits_and_closed_intent() {
        let mut profile = sample_profile();
        profile.desired_open = true;
        assert_eq!(process_state(&profile, false), "queued");
        profile.open_failures = MAX_OPEN_ATTEMPTS;
        assert_eq!(process_state(&profile, false), "error");
        profile.desired_open = false;
        assert_eq!(process_state(&profile, false), "closed");
        profile.pid = Some(42);
        profile.pid_started = Some("Wed Nov 11 00:42:11 2026".into());
        assert_eq!(process_state(&profile, false), "error");
        profile.pid_started = None;
        assert_eq!(process_state(&profile, false), "opening");
        profile.pid_started = Some("Wed Nov 11 00:42:11 2026".into());
        assert_eq!(process_state(&profile, true), "open");
        profile.desired_open = false;
        profile.close_pending = true;
        assert_eq!(process_state(&profile, true), "closing");
    }

    #[test]
    fn corrupt_or_duplicate_state_is_rejected_without_reset() {
        let duplicate = r#"{
            "schema": 1,
            "next_number": 3,
            "profiles": [
                {"id":"0123456789abcdef01234567","number":1,"label":"A","enabled":true,"desired_open":false},
                {"id":"0123456789abcdef01234567","number":2,"label":"B","enabled":true,"desired_open":false}
            ],
            "settings": {}
        }"#;
        assert!(parse_state(duplicate.as_bytes()).is_err());
        assert!(parse_state(b"{not json").is_err());

        let mut stored = Stored::default();
        stored.settings.pause_min_seconds = 20;
        stored.settings.pause_max_seconds = 5;
        assert!(validate_state(&stored).is_err());
    }

    #[test]
    fn generated_identifier_is_opaque_and_stable_width() {
        let id = new_id(&[], 7);
        assert_eq!(id.len(), 24);
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn manager_view_maps_explicit_worker_and_single_account_credits() {
        let view: manager_gateway::ManagerView = serde_json::from_value(serde_json::json!({
            "schema": 1,
            "receivedAt": 1.0,
            "maxAgeSeconds": 30.0,
            "workers": [{
                "worker_id": "image-1",
                "kind": "image",
                "has_live_token": true,
                "control": {
                    "dispatchEnabled": false,
                    "revision": "revision-1",
                    "pacing": {"minSeconds": 5, "maxSeconds": 15}
                },
                "state": {
                    "connection": "online",
                    "sign_in": "signed_in",
                    "readiness": "ready",
                    "activity": "idle",
                    "observed_at": 1794350000.0,
                    "pacing": {"minSeconds": 5, "maxSeconds": 15}
                },
                "fresh": true,
                "held_jobs": [],
                "durableBusy": false,
                "safeToClose": true
            }],
            "accounts": [{
                "provider": "openai",
                "account_id": "account-1",
                "balance": {
                    "value": 10.0,
                    "unit": "credits",
                    "source": "gateway",
                    "observed_at": 1794350000.0
                },
                "fresh": true,
                "gateway_enabled": false,
                "workers": ["image-1"]
            }]
        }))
        .expect("manager view fixture");
        let profile = GatewayProfile {
            id: "profile-1".into(),
            image_worker_id: Some("image-1".into()),
            video_worker_id: None,
        };
        assert!(can_dispatch(&view, &worker_ids(&profile)));
        let reading = reading_for(&view, &profile);
        assert_eq!(reading.connection.as_deref(), Some("online"));
        assert_eq!(reading.activity.as_deref(), Some("idle"));
        assert_eq!(reading.account_id.as_deref(), Some("account-1"));
        let credits = reading.credits.expect("credits");
        assert_eq!(credits["balance"]["value"], 10.0);
        assert_eq!(credits["gateway_enabled"], false);
        assert_eq!(credits["credit_buckets"], serde_json::Value::Null);

        let unbound = GatewayProfile {
            id: "profile-2".into(),
            image_worker_id: None,
            video_worker_id: None,
        };
        assert!(reading_for(&view, &unbound).account_id.is_none());
        assert!(reading_for(&view, &unbound).credits.is_none());
        assert!(!can_dispatch(&view, &["missing".into()]));
    }

    #[test]
    fn worker_ids_drop_duplicates() {
        let profile = GatewayProfile {
            id: "profile-1".into(),
            image_worker_id: Some("same".into()),
            video_worker_id: Some("same".into()),
        };
        assert_eq!(worker_ids(&profile), vec!["same".to_string()]);
    }
}
