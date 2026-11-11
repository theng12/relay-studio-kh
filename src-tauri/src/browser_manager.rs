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

use crate::manager_gateway;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
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
const IDENTITY_HOLD: &str =
    "Chrome process identity could not be verified; it was not stopped or restarted.";

// ---------------------------------------------------------------------------
// Gateway authority seam (implemented and installed by the parent-owned adapter)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct GatewayProfile {
    pub id: String,
    pub image_worker_id: Option<String>,
    pub video_worker_id: Option<String>,
    /// Exact normalized Gateway origin the worker generations were captured under.
    pub binding_origin: Option<String>,
    pub image_worker_generation: Option<String>,
    pub video_worker_generation: Option<String>,
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
    /// Authenticated adapter only. Presence and spend fields are reported at the snapshot root so
    /// the UI never has to dig them out of `credits`; conflicting account rows stay unknown.
    pub credits: Option<serde_json::Value>,
    pub next_eligible_at: Option<String>,
    pub gateway_enabled: Option<bool>,
    pub reserved: Option<f64>,
    pub available: Option<f64>,
    /// Worker/account readings disagree or are stale; the profile needs attention.
    pub attention: bool,
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
    fn support(
        &self,
        profiles: &[GatewayProfile],
        pacing: &manager_gateway::Pacing,
    ) -> GatewaySupport;
    /// Called while moving a profile to close: issues ONE control for the profile's explicit
    /// image and video workers with `dispatchEnabled: false` and `closeRequested: true`, binds
    /// that attempt to the exact saved origin, kinds and credential generations, and returns
    /// `Safe` only after a fresh paused acknowledgement carrying THIS attempt's revision plus
    /// `safeToClose`. Missing bindings must return `NotAcknowledged`; a cached pre-attempt
    /// acknowledgement is never enough. Must return promptly.
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
    pacing_update: tauri::async_runtime::Mutex<()>,
}

#[derive(Default)]
struct GatewayCache {
    /// Origin this cached view was fetched for. A view is never shown or used for close
    /// authorization under a different saved origin.
    origin: Option<String>,
    view: Option<manager_gateway::ManagerView>,
    error: Option<String>,
    pacing_error: Option<String>,
    refreshing: bool,
    last_view: Option<Instant>,
    closes: HashMap<String, CloseState>,
    next_attempt: u64,
}

#[derive(Default)]
struct CloseState {
    in_flight: bool,
    last_attempt: Option<Instant>,
    /// Only an attempt's own response may authorize this profile's close. A cached view from
    /// before the attempt is never enough.
    attempt: Option<CloseAttempt>,
    failure: Option<String>,
}

/// One close control sent for the exact origin and worker bindings saved at that moment.
struct CloseAttempt {
    id: u64,
    origin: String,
    bindings: Vec<manager_gateway::ExpectedWorkerBinding>,
    /// Revision each worker was issued in THIS control response.
    revisions: HashMap<String, String>,
    response_received_at: Option<f64>,
}

impl GatewayCache {
    /// Returns the cached view only for the origin it was fetched from. Changing the saved
    /// origin invalidates the view, error and close bookkeeping before anything can use them.
    fn view_for(&mut self, origin: Option<&str>) -> Option<manager_gateway::ManagerView> {
        if origin.is_none() || self.origin.as_deref() != origin {
            self.origin = origin.map(ToString::to_string);
            self.view = None;
            self.error = None;
            self.pacing_error = None;
            self.last_view = None;
            self.closes.clear();
            return None;
        }
        self.view.clone()
    }

    /// Delayed or equally dated replies cannot replace a newer control snapshot.
    fn publish_view(&mut self, view: manager_gateway::ManagerView) -> bool {
        if !view.received_at.is_finite()
            || view.received_at <= 0.0
            || self
                .view
                .as_ref()
                .is_some_and(|current| view.received_at <= current.received_at)
        {
            return false;
        }
        self.view = Some(view);
        self.error = None;
        self.last_view = Some(Instant::now());
        true
    }
}

impl LiveGateway {
    fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            cache: Arc::new(Mutex::new(GatewayCache::default())),
            pacing_update: tauri::async_runtime::Mutex::new(()),
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
        // Validate against the saved origin before anything else can consume the cache.
        let _ = cache.view_for(origin.as_deref());
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
            // A view fetched for an older origin is discarded, never displayed.
            if cache.origin.as_deref() != Some(origin.as_str()) {
                return;
            }
            match result {
                Ok(view) => {
                    cache.publish_view(view);
                }
                Err(error) => cache.error = Some(error.to_string()),
            }
        });
    }

    fn remember(&self, origin: String, view: manager_gateway::ManagerView) {
        let mut cache = self.cache();
        let _ = cache.view_for(Some(origin.as_str()));
        cache.publish_view(view);
        cache.closes.clear();
    }

    fn forget(&self) {
        let mut cache = self.cache();
        cache.origin = None;
        cache.view = None;
        cache.error = None;
        cache.pacing_error = None;
        cache.last_view = None;
        cache.closes.clear();
    }

    /// Cached authenticated readback with the origin it belongs to, for deliberate Save.
    fn current_view(&self) -> Option<(String, manager_gateway::ManagerView)> {
        let origin = gateway_origin(&self.app)?;
        let mut cache = self.cache();
        let view = cache.view_for(Some(origin.as_str()))?;
        Some((origin, view))
    }

    async fn apply_saved_pacing(&self, shared: &Shared) {
        // Serialize writes and read the latest persisted preference after taking the lock.
        let _serial = self.pacing_update.lock().await;
        let origin = gateway_origin(&self.app);
        let result: Result<manager_gateway::ManagerView, String> = async {
            let origin = origin.as_deref().ok_or("Save a Gateway address first.")?;
            let (profiles, pacing) = {
                let inner = locked(shared)?;
                let stored = stored_or_error(&inner)?;
                (gateway_profiles(stored), saved_pacing(&stored.settings))
            };
            let (_, view) = self
                .current_view()
                .ok_or("Read Gateway before applying pacing.")?;
            let ids = pacing_worker_ids(&view, &profiles, origin, epoch_now())?;
            manager_gateway::set_pacing(origin, &ids, pacing)
                .await
                .map_err(|error| error.to_string())
        }
        .await;
        let mut cache = self.cache();
        if gateway_origin(&self.app) != origin || cache.origin != origin {
            return;
        }
        match result {
            Ok(view) => {
                cache.publish_view(view);
                cache.pacing_error = None;
            }
            Err(error) => {
                cache.pacing_error = Some(format!(
                    "Pacing preference saved; Gateway update failed: {error}"
                ))
            }
        }
    }
}

impl ManagerGateway for LiveGateway {
    fn read(&self, profiles: &[GatewayProfile]) -> HashMap<String, GatewayReading> {
        let origin = gateway_origin(&self.app);
        let stale = {
            let cache = self.cache();
            cache.origin.as_deref() != origin.as_deref()
                || cache.view.is_none()
                || cache
                    .last_view
                    .map_or(true, |at| at.elapsed() >= Duration::from_secs(5))
        };
        if stale {
            self.refresh();
        }
        let view = self.cache().view_for(origin.as_deref());
        let Some(view) = view else {
            return HashMap::new();
        };
        profiles
            .iter()
            .map(|profile| (profile.id.clone(), reading_for(&view, profile)))
            .collect()
    }

    fn support(
        &self,
        profiles: &[GatewayProfile],
        pacing: &manager_gateway::Pacing,
    ) -> GatewaySupport {
        let origin = gateway_origin(&self.app);
        let manager_paired = origin
            .as_deref()
            .is_some_and(|origin| manager_gateway::paired(origin).unwrap_or(false));
        let mut cache = self.cache();
        let view = cache.view_for(origin.as_deref());
        let fresh = view.as_ref().is_some_and(|view| view.is_fresh(epoch_now()));
        let authorized = view.is_some();
        let mut telemetry_error = cache.error.clone();
        if telemetry_error.is_none() && authorized && !fresh {
            telemetry_error = Some("Gateway manager readback is stale.".into());
        }
        if telemetry_error.is_none() && !authorized {
            telemetry_error = Some("Waiting for the first Gateway manager readback.".into());
        }
        let pacing_applied = view.as_ref().is_some_and(|view| {
            origin.as_deref().is_some_and(|origin| {
                pacing_is_applied(view, profiles, origin, pacing, epoch_now())
            })
        });
        GatewaySupport {
            telemetry_authorized: authorized,
            telemetry_error,
            manager_paired,
            pacing_applied,
            pacing_error: (!pacing_applied).then(|| {
                cache.pacing_error.clone().unwrap_or_else(|| {
                    "Pacing is saved; waiting for every bound worker to acknowledge the current revision and range. Save again to retry a failed update."
                        .to_string()
                })
            }),
        }
    }

    fn close_authorization(&self, profile: &GatewayProfile) -> CloseAuthorization {
        let Some(origin) = gateway_origin(&self.app) else {
            return CloseAuthorization::NotAcknowledged("Save a Gateway address first.".into());
        };
        let expected = match expected_bindings(profile, &origin) {
            Ok(bindings) => bindings,
            Err(reason) => return CloseAuthorization::NotAcknowledged(reason),
        };
        let mut cache = self.cache();
        // A view from another origin can never authorize stopping this profile's Chrome.
        let view = cache.view_for(Some(origin.as_str()));
        // Reserve this attempt's identity before the per-profile entry borrow exists.
        cache.next_attempt = cache.next_attempt.saturating_add(1);
        let attempt_id = cache.next_attempt;
        let state = cache.closes.entry(profile.id.clone()).or_default();
        // An origin or binding change makes an earlier attempt unusable, including one still in
        // flight: its response must not publish a view or mark a later attempt complete.
        if state
            .attempt
            .as_ref()
            .is_some_and(|attempt| !attempt_is_current(attempt, &origin, &expected))
        {
            state.attempt = None;
            state.failure = None;
        }
        if let Some(attempt) = state.attempt.take() {
            if attempt.revisions.is_empty() {
                state.attempt = Some(attempt);
                return CloseAuthorization::NotAcknowledged(
                    "Waiting for this close attempt's authenticated control response.".into(),
                );
            }
            let Some(view) = view.as_ref() else {
                state.attempt = Some(attempt);
                return CloseAuthorization::NotAcknowledged(
                    "Waiting for this close attempt's fresh authenticated readback.".into(),
                );
            };
            if let Some(reason) = attempt_revision_mismatch(view, &attempt) {
                // A fresh view without this attempt's revision means a later authorized
                // control replaced it. Drop the attempt; the rate limit spaces the next one.
                state.failure = Some(reason);
            } else {
                let safety = manager_gateway::close_safety_for_bindings(view, &expected);
                if safety.safe_to_close {
                    // Keep the completed attempt until reconcile observes the close finish.
                    state.attempt = Some(attempt);
                    return CloseAuthorization::Safe;
                }
                let reason = close_reason(&safety);
                state.attempt = Some(attempt);
                return CloseAuthorization::NotAcknowledged(reason);
            }
        }
        let waiting = state.failure.clone().unwrap_or_else(|| {
            "Waiting for a generation-bound close attempt for this profile.".into()
        });
        if state.in_flight
            || state
                .last_attempt
                .is_some_and(|at| at.elapsed() < Duration::from_secs(5))
        {
            return CloseAuthorization::NotAcknowledged(waiting);
        }
        let ids: Vec<String> = expected
            .iter()
            .map(|binding| binding.worker_id.clone())
            .collect();
        state.attempt = Some(CloseAttempt {
            id: attempt_id,
            origin: origin.clone(),
            bindings: expected.clone(),
            revisions: HashMap::new(),
            response_received_at: None,
        });
        state.failure = None;
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
            let cache = &mut *cache;
            // Only this attempt's origin and identity may publish a result.
            if cache.origin.as_deref() != Some(origin.as_str()) {
                return;
            }
            let current_id = cache
                .closes
                .get(&profile_id)
                .and_then(|state| state.attempt.as_ref())
                .map(|attempt| attempt.id);
            if current_id != Some(attempt_id) {
                return;
            }
            match result {
                Ok(view) => {
                    let bindings = cache
                        .closes
                        .get(&profile_id)
                        .and_then(|state| state.attempt.as_ref())
                        .map(|attempt| attempt.bindings.clone())
                        .unwrap_or_default();
                    let revisions = response_revisions(&view, &bindings);
                    let response_received_at = view.received_at;
                    if !cache.publish_view(view) {
                        if let Some(state) = cache.closes.get_mut(&profile_id) {
                            state.in_flight = false;
                            state.attempt = None;
                            state.failure = Some(
                                "A newer or equally dated Gateway view is already cached; this delayed close response cannot authorize close.".into(),
                            );
                        }
                        return;
                    }
                    if let Some(state) = cache.closes.get_mut(&profile_id) {
                        state.in_flight = false;
                        match revisions {
                            Ok(revisions) => {
                                if let Some(attempt) = state.attempt.as_mut() {
                                    attempt.revisions = revisions;
                                    attempt.response_received_at = Some(response_received_at);
                                }
                                state.failure = None;
                            }
                            Err(reason) => {
                                state.attempt = None;
                                state.failure = Some(reason);
                            }
                        }
                    }
                }
                Err(error) => {
                    if let Some(state) = cache.closes.get_mut(&profile_id) {
                        state.in_flight = false;
                        state.attempt = None;
                        state.failure = Some(format!(
                            "Gateway did not accept the close control; close waits. {error}"
                        ));
                    }
                }
            }
        });
        CloseAuthorization::NotAcknowledged(
            "Requested one generation-bound pause for this close; waiting for this attempt's fresh paused acknowledgement and safe-to-close readback."
                .into(),
        )
    }
}

fn gateway_origin(app: &tauri::AppHandle) -> Option<String> {
    super::current(app)
        .ok()
        .and_then(|settings| settings.gateway_url)
}

/// Pairing uses the saved, already-normalized Gateway origin; the caller never supplies one.
fn required_origin(origin: Option<String>) -> Result<String, String> {
    origin.ok_or_else(|| "Save a Gateway address first.".to_string())
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

fn saved_pacing(settings: &StoredSettings) -> manager_gateway::Pacing {
    manager_gateway::Pacing {
        min_seconds: settings.pause_min_seconds,
        max_seconds: settings.pause_max_seconds,
    }
}

fn gateway_profiles(stored: &Stored) -> Vec<GatewayProfile> {
    stored
        .profiles
        .iter()
        .map(|profile| GatewayProfile {
            id: profile.id.clone(),
            image_worker_id: profile.image_worker_id.clone(),
            video_worker_id: profile.video_worker_id.clone(),
            binding_origin: profile.binding_origin.clone(),
            image_worker_generation: profile.image_worker_generation.clone(),
            video_worker_generation: profile.video_worker_generation.clone(),
        })
        .collect()
}

fn pacing_worker_ids(
    view: &manager_gateway::ManagerView,
    profiles: &[GatewayProfile],
    origin: &str,
    now: f64,
) -> Result<Vec<String>, String> {
    if !view.is_fresh(now) {
        return Err("Gateway readback is stale; pacing was not sent.".into());
    }
    let mut ids = Vec::new();
    for profile in profiles
        .iter()
        .filter(|profile| !worker_ids(profile).is_empty())
    {
        let bindings = expected_bindings(profile, origin)?;
        for binding in bindings {
            let worker = view
                .workers
                .iter()
                .find(|worker| worker.worker_id == binding.worker_id)
                .ok_or("A bound worker is outside the current manager readback.")?;
            if !worker.has_live_token
                || worker.kind.as_deref() != Some(binding.kind.as_str())
                || worker.control.credential_generation.as_deref()
                    != Some(binding.credential_generation.as_str())
            {
                return Err(
                    "A worker binding changed; save the account binding before applying pacing."
                        .into(),
                );
            }
            if ids.contains(&binding.worker_id) {
                return Err("A worker is assigned to more than one saved profile.".into());
            }
            ids.push(binding.worker_id);
        }
    }
    if ids.is_empty() {
        return Err("Bind image and video workers before applying pacing.".into());
    }
    Ok(ids)
}

fn pacing_is_applied(
    view: &manager_gateway::ManagerView,
    profiles: &[GatewayProfile],
    origin: &str,
    pacing: &manager_gateway::Pacing,
    now: f64,
) -> bool {
    let Ok(ids) = pacing_worker_ids(view, profiles, origin, now) else {
        return false;
    };
    ids.iter().all(|id| {
        let Some(worker) = view.workers.iter().find(|worker| &worker.worker_id == id) else {
            return false;
        };
        let Some(state) = &worker.state else {
            return false;
        };
        let matches = |actual: Option<&manager_gateway::Pacing>| {
            actual.is_some_and(|actual| {
                actual.min_seconds == pacing.min_seconds && actual.max_seconds == pacing.max_seconds
            })
        };
        worker.fresh
            && worker.received_at.is_some_and(|at| {
                at.is_finite() && at > 0.0 && at <= now && now - at <= view.max_age_seconds
            })
            && state.observed_at.is_some_and(|at| {
                at.is_finite() && at > 0.0 && at <= now && now - at <= view.max_age_seconds
            })
            && worker.control.revision.as_deref().is_some_and(|revision| {
                !revision.is_empty() && state.control_revision.as_deref() == Some(revision)
            })
            && matches(worker.control.pacing.as_ref())
            && matches(state.pacing.as_ref())
    })
}

/// Both physical siblings must be explicitly bound with the exact generation captured for this
/// origin. One worker, one reused id for both kinds, or an unknown generation all hold close.
fn expected_bindings(
    profile: &GatewayProfile,
    origin: &str,
) -> Result<Vec<manager_gateway::ExpectedWorkerBinding>, String> {
    let image = profile
        .image_worker_id
        .as_deref()
        .filter(|id| !id.is_empty());
    let video = profile
        .video_worker_id
        .as_deref()
        .filter(|id| !id.is_empty());
    let (Some(image), Some(video)) = (image, video) else {
        return Err(
            "Both an image and a video worker must be bound before this profile can close.".into(),
        );
    };
    if image == video {
        return Err(
            "The image and video bindings must be two different workers; close waits.".into(),
        );
    }
    let Some(binding_origin) = profile.binding_origin.as_deref() else {
        return Err(
            "This profile has no saved Gateway worker binding; save the account binding while Gateway is connected."
                .into(),
        );
    };
    if binding_origin != origin {
        return Err(
            "This profile's worker binding belongs to a different Gateway address; save the account binding again."
                .into(),
        );
    }
    let (Some(image_generation), Some(video_generation)) = (
        profile.image_worker_generation.as_deref(),
        profile.video_worker_generation.as_deref(),
    ) else {
        return Err(
            "The saved credential generation for an image or video worker is unknown; save the account binding while Gateway is connected."
                .into(),
        );
    };
    Ok(vec![
        manager_gateway::ExpectedWorkerBinding {
            worker_id: image.to_string(),
            kind: "image".into(),
            credential_generation: image_generation.to_string(),
        },
        manager_gateway::ExpectedWorkerBinding {
            worker_id: video.to_string(),
            kind: "video".into(),
            credential_generation: video_generation.to_string(),
        },
    ])
}

fn same_bindings(
    left: &[manager_gateway::ExpectedWorkerBinding],
    right: &[manager_gateway::ExpectedWorkerBinding],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.worker_id == right.worker_id
                && left.kind == right.kind
                && left.credential_generation == right.credential_generation
        })
}

fn attempt_is_current(
    attempt: &CloseAttempt,
    origin: &str,
    expected: &[manager_gateway::ExpectedWorkerBinding],
) -> bool {
    attempt.origin == origin && same_bindings(&attempt.bindings, expected)
}

/// The revision a worker currently reports must be the one issued to it by THIS close attempt's
/// response. A later control's revision is not inherited.
fn attempt_revision_mismatch(
    view: &manager_gateway::ManagerView,
    attempt: &CloseAttempt,
) -> Option<String> {
    if !attempt.response_received_at.is_some_and(|received_at| {
        received_at.is_finite()
            && received_at > 0.0
            && view.received_at.is_finite()
            && view.received_at >= received_at
    }) {
        return Some("Waiting for readback at or after this close attempt's response.".into());
    }
    for binding in &attempt.bindings {
        let Some(expected) = attempt.revisions.get(&binding.worker_id) else {
            return Some("Waiting for this close attempt's authenticated revision.".into());
        };
        let Some(worker) = view
            .workers
            .iter()
            .find(|worker| worker.worker_id == binding.worker_id)
        else {
            return Some(format!(
                "Worker {} is absent from the fresh readback; close waits.",
                binding.worker_id
            ));
        };
        if worker.control.revision.as_deref() != Some(expected.as_str()) {
            return Some(format!(
                "Worker {} reports a different control revision than this close attempt issued; close waits.",
                binding.worker_id
            ));
        }
    }
    None
}

/// A control response is usable only when every bound worker is present, live, still the expected
/// kind and generation, has close applied, and carries the revision this attempt issued.
fn response_revisions(
    view: &manager_gateway::ManagerView,
    bindings: &[manager_gateway::ExpectedWorkerBinding],
) -> Result<HashMap<String, String>, String> {
    let mut revisions = HashMap::new();
    for binding in bindings {
        let Some(worker) = view
            .workers
            .iter()
            .find(|worker| worker.worker_id == binding.worker_id)
        else {
            return Err(format!(
                "Gateway's close response omitted worker {}; close waits.",
                binding.worker_id
            ));
        };
        if !worker.has_live_token {
            return Err(format!(
                "Worker {} has no live credential; close waits.",
                binding.worker_id
            ));
        }
        if worker.kind.as_deref() != Some(binding.kind.as_str()) {
            return Err(format!(
                "Worker {} no longer matches its saved kind; close waits.",
                binding.worker_id
            ));
        }
        if worker.control.credential_generation.as_deref()
            != Some(binding.credential_generation.as_str())
        {
            return Err(format!(
                "Worker {} credential generation changed; reconcile the account binding.",
                binding.worker_id
            ));
        }
        if worker.control.dispatch_enabled || worker.control.close_requested != Some(true) {
            return Err(format!(
                "Gateway did not apply dispatch off and close for worker {}; close waits.",
                binding.worker_id
            ));
        }
        let revision = worker
            .control
            .revision
            .as_deref()
            .filter(|revision| !revision.is_empty())
            .ok_or_else(|| {
                format!(
                    "Gateway's close response has no revision for worker {}; close waits.",
                    binding.worker_id
                )
            })?;
        revisions.insert(binding.worker_id.clone(), revision.to_string());
    }
    Ok(revisions)
}

/// Deliberate pairing or Save captures generations only from a fresh authenticated response that
/// shows both physical siblings live, of the matching kind, with a nonempty generation.
fn verified_generations(
    view: &manager_gateway::ManagerView,
    image_worker_id: Option<&str>,
    video_worker_id: Option<&str>,
    now: f64,
) -> Option<(String, String)> {
    if !view.is_fresh(now) {
        return None;
    }
    let image = image_worker_id.filter(|id| !id.is_empty())?;
    let video = video_worker_id.filter(|id| !id.is_empty())?;
    if image == video {
        return None;
    }
    let generation = |id: &str, kind: &str| {
        let worker = view.workers.iter().find(|worker| worker.worker_id == id)?;
        if !worker.has_live_token || worker.kind.as_deref() != Some(kind) {
            return None;
        }
        let received_at = worker.received_at?;
        if !worker.fresh
            || !received_at.is_finite()
            || received_at <= 0.0
            || received_at > now + 5.0
            || now - received_at > view.max_age_seconds
        {
            return None;
        }
        worker
            .control
            .credential_generation
            .as_deref()
            .filter(|generation| !generation.is_empty())
            .map(ToString::to_string)
    };
    Some((generation(image, "image")?, generation(video, "video")?))
}

fn complete_binding(profile: &StoredProfile) -> bool {
    profile.binding_origin.is_some()
        && profile.image_worker_generation.is_some()
        && profile.video_worker_generation.is_some()
}

/// Pairing is a reconciliation point only for a missing initial binding. A recorded binding,
/// including a generation a newer response disagrees with, is left for an explicit account Save.
fn fill_initial_bindings(
    stored: &mut Stored,
    origin: &str,
    view: &manager_gateway::ManagerView,
    now: f64,
) {
    for profile in &mut stored.profiles {
        if complete_binding(profile) {
            continue;
        }
        let Some((image, video)) = verified_generations(
            view,
            profile.image_worker_id.as_deref(),
            profile.video_worker_id.as_deref(),
            now,
        ) else {
            continue;
        };
        profile.image_worker_generation = Some(image);
        profile.video_worker_generation = Some(video);
        profile.binding_origin = Some(origin.to_string());
    }
}

fn bind_after_pair(
    shared: &Shared,
    origin: &str,
    view: &manager_gateway::ManagerView,
) -> Result<(), String> {
    mutate(shared, |stored| {
        fill_initial_bindings(stored, origin, view, epoch_now());
        Ok(())
    })
}

/// Worker state is current only while its telemetry is fresh and inside the view's age limit.
fn live_state<'a>(
    view: &'a manager_gateway::ManagerView,
    id: &str,
    now: f64,
) -> Option<&'a manager_gateway::WorkerState> {
    let worker = view.workers.iter().find(|worker| worker.worker_id == id)?;
    if !worker.fresh {
        return None;
    }
    let received_at = worker.received_at?;
    if !received_at.is_finite() || received_at <= 0.0 || now - received_at > view.max_age_seconds {
        return None;
    }
    worker.state.as_ref()
}

/// Every bound worker must report the same value; missing, stale or differing readings stay unknown.
fn agreed_state(
    states: &[Option<&manager_gateway::WorkerState>],
    field: fn(&manager_gateway::WorkerState) -> Option<String>,
) -> Option<String> {
    let values: Option<Vec<String>> = states
        .iter()
        .map(|state| (*state).and_then(|state| field(state)))
        .collect();
    let values = values?;
    let first = values.first()?.clone();
    values.iter().all(|value| *value == first).then_some(first)
}

fn latest_observed(states: &[Option<&manager_gateway::WorkerState>]) -> Option<String> {
    let values: Option<Vec<f64>> = states
        .iter()
        .map(|state| (*state).and_then(|state| state.observed_at))
        .collect();
    values?
        .into_iter()
        .reduce(f64::max)
        .map(|value| value.to_string())
}

fn reading_for(view: &manager_gateway::ManagerView, profile: &GatewayProfile) -> GatewayReading {
    reading_for_at(view, profile, epoch_now())
}

fn reading_for_at(
    view: &manager_gateway::ManagerView,
    profile: &GatewayProfile,
    now: f64,
) -> GatewayReading {
    let ids = worker_ids(profile);
    // A stale view or an unbound profile is fully unknown, never last-known state shown as now.
    if ids.is_empty() || !view.is_fresh(now) {
        return GatewayReading::default();
    }
    let states: Vec<Option<&manager_gateway::WorkerState>> =
        ids.iter().map(|id| live_state(view, id, now)).collect();
    let rows: Vec<&manager_gateway::ManagerAccount> = view
        .accounts
        .iter()
        .filter(|account| account.workers.iter().any(|id| ids.contains(id)))
        .collect();
    // Two different accounts on one physical profile must not share credits as if they matched.
    let mismatched = rows.first().is_some_and(|first| {
        rows.iter()
            .any(|row| row.provider != first.provider || row.account_id != first.account_id)
    });
    let account = rows.first().copied().filter(|_| !mismatched);
    GatewayReading {
        connection: agreed_state(&states, |state| state.connection.clone()),
        sign_in: agreed_state(&states, |state| state.sign_in.clone()),
        readiness: agreed_state(&states, |state| state.readiness.clone()),
        activity: agreed_state(&states, |state| state.activity.clone()),
        provider: account.map(|account| account.provider.clone()),
        account_id: account.map(|account| account.account_id.clone()),
        observed_at: latest_observed(&states),
        credits: account
            .filter(|account| account.balance.is_some() || account.credit_buckets.is_some())
            .map(|account| {
                serde_json::json!({
                    "balance": account.balance.clone(),
                    "fresh": account.fresh,
                    "reset_at": account.reset_at,
                    "credit_buckets": account.credit_buckets.clone(),
                    "gateway_enabled": account.gateway_enabled,
                    "reserved": account.reserved,
                    "available": account.available,
                })
            }),
        next_eligible_at: agreed_state(&states, |state| {
            state.next_eligible_at.map(|value| value.to_string())
        }),
        gateway_enabled: account.map(|account| account.gateway_enabled),
        reserved: account.and_then(|account| account.reserved),
        available: account.and_then(|account| account.available),
        attention: mismatched
            || states
                .iter()
                .any(|state| (*state).and_then(|state| state.attention).unwrap_or(false)),
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
    /// Exact normalized Gateway origin and per-worker credential generations captured by a
    /// deliberate pairing or explicit account Save. Label-only saves never change them.
    #[serde(default)]
    binding_origin: Option<String>,
    #[serde(default)]
    image_worker_generation: Option<String>,
    #[serde(default)]
    video_worker_generation: Option<String>,
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
    pause_min_seconds: f64,
    #[serde(default = "default_pause_max_seconds")]
    pause_max_seconds: f64,
}

fn default_reserve_mb() -> u64 {
    2048
}
fn default_pause_min_seconds() -> f64 {
    5.0
}
fn default_pause_max_seconds() -> f64 {
    15.0
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
    // Reported at the root to match the UI; `credits` may repeat them but they are never summed.
    gateway_enabled: Option<bool>,
    reserved: Option<f64>,
    available: Option<f64>,
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
    pause_min_seconds: f64,
    pause_max_seconds: f64,
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
    // Retained spawn leases: a live Child proves this exact pid is still our process, so its
    // start time may be captured safely and a reused pid is never adopted.
    children: Mutex<HashMap<u32, Child>>,
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
        children: Mutex::new(HashMap::new()),
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

/// Show stops launching processes, so its errors are transient. A verified process or a
/// successful activation resolves them; ownership and close holds are left untouched.
fn is_show_error(message: &str) -> bool {
    message.starts_with("Chrome is no longer running for this profile")
        || message.starts_with("macOS did not activate")
        || message.starts_with("Showing a managed Chrome profile is available only on macOS")
        || message.starts_with("This profile's Chrome process is not verified open")
}

fn clear_resolved_errors(
    errors: &mut HashMap<String, String>,
    attention: &mut HashSet<String>,
    id: &str,
) {
    let resolved = errors
        .get(id)
        .is_some_and(|message| message == IDENTITY_HOLD || is_show_error(message));
    if resolved {
        errors.remove(id);
        attention.remove(id);
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
/// stored, logged or placed in the saved snapshot. The origin is the saved, validated Gateway
/// address, so the UI passes only the token. Returns the normal manager snapshot.
#[tauri::command]
pub(crate) async fn manager_pair_gateway(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    token: String,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    let origin = required_origin(gateway_origin(window.app_handle()))?;
    let shared = manager.shared.clone();
    drop(manager);
    let view = manager_gateway::pair(&origin, &token)
        .await
        .map_err(|error| error.to_string())?;
    shared.gateway.remember(origin.clone(), view.clone());
    // Deliberate pairing may fill a missing initial generation binding. It never replaces a
    // recorded one, and never clears work or state that is already held.
    bind_after_pair(&shared, &origin, &view)?;
    shared.gateway.apply_saved_pacing(&shared).await;
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
    // A label-only update passes no worker fields and never rebinds. Passing a worker field is
    // the explicit account Save that reconciles both generations from a fresh authenticated view.
    let rebind = image_worker_id.is_some() || video_worker_id.is_some();
    let binding_view = if rebind {
        manager.shared.gateway.current_view()
    } else {
        None
    };
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
        // Bindings are frozen while this physical profile is resident or closing, so close
        // authorization always evaluates the workers that were bound when close was requested.
        // Filling a missing binding stays allowed; replacing or clearing a recorded one does not.
        let frozen = profile.pid.is_some() || profile.close_pending;
        if let Some(worker) = image_worker_id {
            let next = normalize_worker(worker)?;
            profile.image_worker_id = updated_binding(&profile.image_worker_id, next, frozen)?;
        }
        if let Some(worker) = video_worker_id {
            let next = normalize_worker(worker)?;
            profile.video_worker_id = updated_binding(&profile.video_worker_id, next, frozen)?;
        }
        if rebind {
            // The mapping is written as one atom: either both exact generations for this saved
            // origin, or nothing, which leaves close unknown/held until a successful Save.
            let captured = binding_view.as_ref().and_then(|(origin, view)| {
                verified_generations(
                    view,
                    profile.image_worker_id.as_deref(),
                    profile.video_worker_id.as_deref(),
                    epoch_now(),
                )
                .map(|generations| (origin.clone(), generations))
            });
            match captured {
                Some((origin, (image, video))) => {
                    profile.image_worker_generation = Some(image);
                    profile.video_worker_generation = Some(video);
                    profile.binding_origin = Some(origin);
                }
                None => {
                    profile.image_worker_generation = None;
                    profile.video_worker_generation = None;
                    profile.binding_origin = None;
                }
            }
        }
        Ok(())
    })?;
    if rebind {
        // A deliberate rebind invalidates any earlier attempt, including one still in flight.
        manager.shared.gateway.cache().closes.remove(&profile_id);
        let shared = manager.shared.clone();
        let _ = tauri::async_runtime::spawn(async move {
            shared.gateway.apply_saved_pacing(&shared).await;
        });
    }
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
    // (profile, whether its Chrome was already resident when Open was requested)
    let mut opened: Vec<(GatewayProfile, bool)> = Vec::new();
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
            // Only local Chrome intent is saved; Gateway dispatch stays Gateway's decision.
            let resident = profile.pid.is_some();
            profile.desired_open = true;
            profile.open_failures = 0;
            opened.push((
                GatewayProfile {
                    id: profile.id.clone(),
                    image_worker_id: profile.image_worker_id.clone(),
                    video_worker_id: profile.video_worker_id.clone(),
                    binding_origin: profile.binding_origin.clone(),
                    image_worker_generation: profile.image_worker_generation.clone(),
                    video_worker_generation: profile.video_worker_generation.clone(),
                },
                resident,
            ));
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
    {
        // A new open intent drops any earlier close attempt so a later close cannot inherit it.
        let mut cache = manager.shared.gateway.cache();
        for (profile, _) in &opened {
            cache.closes.remove(&profile.id);
        }
    }
    let snapshot = finish(&manager.shared)?;
    // Opening never authorizes jobs. A profile that was already resident is activated by pid,
    // which brings its own windows forward and cannot resume paused or held work.
    let targets = {
        let inner = locked(&manager.shared)?;
        let stored = stored_or_error(&inner)?;
        opened
            .iter()
            .filter(|(profile, resident)| {
                *resident && inner.measurement.alive.contains(&profile.id)
            })
            .filter_map(|(profile, _)| {
                stored
                    .profiles
                    .iter()
                    .find(|entry| entry.id == profile.id)
                    .and_then(|entry| entry.pid)
                    .map(|pid| (profile.id.clone(), pid))
            })
            .collect::<Vec<_>>()
    };
    let mut shown = Vec::new();
    let mut failures: Vec<(String, String)> = Vec::new();
    for (id, pid) in targets {
        match show(pid) {
            Ok(()) => shown.push(id),
            Err(reason) => failures.push((id, reason)),
        }
    }
    {
        let mut inner = locked(&manager.shared)?;
        let inner = &mut *inner;
        for id in &shown {
            clear_resolved_errors(&mut inner.errors, &mut inner.attention, id);
        }
        for (id, reason) in failures {
            inner.errors.insert(id, reason);
        }
    }
    Ok(snapshot)
}

/// Show activates the exact managed profile's own running process. It cannot launch Chrome,
/// creates no managed process, submits no job and never touches dispatch.
#[tauri::command]
pub(crate) fn manager_show_profiles(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    profile_ids: Vec<String>,
) -> Result<Snapshot, String> {
    super::local_only(&window)?;
    reconcile(&manager.shared)?;
    let mut shows: Vec<(String, u32)> = Vec::new();
    let mut failures: Vec<(String, String)> = Vec::new();
    {
        let inner = locked(&manager.shared)?;
        let stored = stored_or_error(&inner)?;
        for id in &profile_ids {
            let Some(profile) = stored.profiles.iter().find(|profile| &profile.id == id) else {
                return Err("Unknown browser profile.".into());
            };
            match profile.pid.filter(|_| inner.measurement.alive.contains(id)) {
                Some(pid) => shows.push((id.clone(), pid)),
                None => failures.push((
                    id.clone(),
                    "This profile's Chrome process is not verified open, so nothing was activated."
                        .into(),
                )),
            }
        }
    }
    let mut shown = Vec::new();
    for (id, pid) in shows {
        match show(pid) {
            Ok(()) => shown.push(id),
            Err(reason) => failures.push((id, reason)),
        }
    }
    {
        let mut inner = locked(&manager.shared)?;
        let inner = &mut *inner;
        for id in &shown {
            clear_resolved_errors(&mut inner.errors, &mut inner.attention, id);
        }
        for (id, reason) in failures {
            inner.errors.insert(id, reason);
        }
    }
    build_snapshot(&manager.shared)
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
pub(crate) async fn manager_save_settings(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    memory_budget_mb: Option<u64>,
    reserve_mb: u64,
    max_active: Option<u32>,
    pause_min_seconds: f64,
    pause_max_seconds: f64,
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
    let shared = manager.shared.clone();
    drop(manager);
    shared.gateway.apply_saved_pacing(&shared).await;
    finish(&shared)
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
    let gateway_profiles = gateway_profiles(&stored);
    let (readings, support) = match authority() {
        Some(authority) => (
            authority.read(&gateway_profiles),
            authority.support(&gateway_profiles, &saved_pacing(&stored.settings)),
        ),
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
                attention: attention.contains(&profile.id)
                    || readings
                        .get(&profile.id)
                        .is_some_and(|reading| reading.attention),
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
        gateway_enabled: reading.gateway_enabled,
        reserved: reading.reserved,
        available: reading.available,
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
    let mut changed = false;
    // Reap exited spawn leases, then retry identity for launches whose first ps lookup missed.
    // The retained Child proves this exact pid is still ours, so no persisted pid is ever adopted.
    {
        let mut children = shared
            .children
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        children.retain(|_, child| child.try_wait().is_ok_and(|status| status.is_none()));
    }
    for profile in stored.profiles.iter_mut() {
        if profile.pid_started.is_some() {
            continue;
        }
        if let (Some(pid), Some(binary)) = (profile.pid, profile.binary.as_deref()) {
            if let Some(started) = leased_identity(shared, pid, binary) {
                profile.pid_started = Some(started);
                changed = true;
            }
        }
    }
    let mut measurement = measure(&stored, chrome.as_deref());
    let mut errors = std::mem::take(&mut guard.errors);
    let mut attention = std::mem::take(&mut guard.attention);

    for profile in stored.profiles.iter_mut() {
        let ownership = profile.pid.map(|pid| {
            let recorded = measurement
                .rows
                .get(&pid)
                .map(|row| ownership(row, profile, chrome.as_deref()));
            if recorded == Some(Ownership::Verified) {
                Ownership::Verified
            } else if leased_running(shared, pid) {
                // The retained child proves this exact process is ours but ps has not confirmed
                // it yet: hold, never clear the pid or relaunch while Chrome may be live.
                Ownership::Unverified
            } else {
                recorded.unwrap_or(Ownership::Reused)
            }
        });
        let mut owned = false;
        match ownership {
            Some(Ownership::Verified) => {
                owned = true;
                clear_resolved_errors(&mut errors, &mut attention, &profile.id);
                if profile.open_failures != 0 {
                    profile.open_failures = 0;
                    changed = true;
                    errors.remove(&profile.id);
                    attention.remove(&profile.id);
                }
            }
            Some(Ownership::Unverified) => {
                // Our binary but no recorded launch time: hold, never adopt, stop or restart it.
                errors.insert(profile.id.clone(), IDENTITY_HOLD.into());
                attention.insert(profile.id.clone());
            }
            Some(Ownership::Reused) | None if profile.pid.is_some() => {
                let wanted = profile.desired_open;
                forget_process(profile);
                changed = true;
                if wanted && !profile.close_pending {
                    profile.open_failures = profile.open_failures.saturating_add(1);
                    errors.insert(profile.id.clone(), exit_message(profile.open_failures));
                    if profile.open_failures >= MAX_OPEN_ATTEMPTS {
                        attention.insert(profile.id.clone());
                    }
                } else {
                    profile.close_pending = false;
                    // The close finished or was abandoned here; its attempt must not be inherited.
                    shared.gateway.cache().closes.remove(&profile.id);
                    errors.remove(&profile.id);
                    attention.remove(&profile.id);
                }
            }
            _ => {}
        }
        if profile.close_pending {
            if profile.pid.is_none() {
                profile.close_pending = false;
                changed = true;
                shared.gateway.cache().closes.remove(&profile.id);
                errors.remove(&profile.id);
            } else if owned {
                let input = GatewayProfile {
                    id: profile.id.clone(),
                    image_worker_id: profile.image_worker_id.clone(),
                    video_worker_id: profile.video_worker_id.clone(),
                    binding_origin: profile.binding_origin.clone(),
                    image_worker_generation: profile.image_worker_generation.clone(),
                    video_worker_generation: profile.video_worker_generation.clone(),
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
        // Re-measure so admission and the snapshot see the new processes. Identity was recorded
        // at launch; it is never adopted later from a possibly reused pid.
        measurement = measure(&stored, chrome.as_deref());
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

/// Why a recorded pid may or may not be Relay's own Chrome. The recorded launch time is the only
/// identity proof; a matching pid alone is never enough because pids are reused.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ownership {
    Verified,
    Reused,
    Unverified,
}

fn ownership(row: &ProcessRow, profile: &StoredProfile, chrome: Option<&Path>) -> Ownership {
    let expected = profile
        .binary
        .clone()
        .or_else(|| chrome.map(|path| path.to_string_lossy().into_owned()));
    if expected.as_deref() != Some(row.comm.as_str()) {
        return Ownership::Reused;
    }
    match profile.pid_started.as_deref() {
        Some(started) if started == row.started => Ownership::Verified,
        Some(_) => Ownership::Reused,
        None => Ownership::Unverified,
    }
}

fn owned_process(row: &ProcessRow, profile: &StoredProfile, chrome: Option<&Path>) -> bool {
    ownership(row, profile, chrome) == Ownership::Verified
}

fn forget_process(profile: &mut StoredProfile) {
    profile.pid = None;
    profile.pid_started = None;
    profile.binary = None;
}

fn launch(shared: &Shared, profile: &mut StoredProfile, chrome: &Path) -> Result<(), String> {
    let directory = profile_dir(shared, &profile.id)?;
    let child = Command::new(chrome)
        .arg(user_data_arg(&directory))
        .spawn()
        .map_err(|_| "Could not start Google Chrome for this profile.".to_string())?;
    let pid = child.id();
    let binary = chrome.to_string_lossy().into_owned();
    profile.pid = Some(pid);
    profile.binary = Some(binary.clone());
    {
        // Retain the spawn lease before any lookup. While this Child is alive, the pid cannot be
        // reused, so its start time is provably the process we just started.
        let mut children = shared
            .children
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        children.insert(pid, child);
    }
    profile.pid_started = capture_identity(shared, pid, &binary);
    Ok(())
}

/// Exact start time of the retained spawn, confirmed by the live Child and our binary. A failed
/// capture is retried on each reconcile while the lease is alive, so a ps miss cannot strand it.
fn leased_identity(shared: &Shared, pid: u32, binary: &str) -> Option<String> {
    let mut children = shared
        .children
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let alive = match children.get_mut(&pid) {
        Some(child) => child.try_wait().is_ok_and(|status| status.is_none()),
        None => return None,
    };
    if !alive {
        // Reap the exited child and drop the lease; no start time can be adopted afterwards.
        children.remove(&pid);
        return None;
    }
    let table = process_table().ok()?;
    let row = table.rows.get(&pid)?;
    (row.comm == binary).then(|| row.started.clone())
}

fn capture_identity(shared: &Shared, pid: u32, binary: &str) -> Option<String> {
    for _ in 0..10 {
        if let Some(started) = leased_identity(shared, pid, binary) {
            return Some(started);
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

/// True only while the retained spawn lease for this exact pid is still running.
fn leased_running(shared: &Shared, pid: u32) -> bool {
    let mut children = shared
        .children
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let alive = match children.get_mut(&pid) {
        Some(child) => child.try_wait().is_ok_and(|status| status.is_none()),
        None => return false,
    };
    if !alive {
        children.remove(&pid);
    }
    alive
}

/// Args are exactly Chrome's own user-data-dir selection; no credential or security flag is ever
/// added.
fn user_data_arg(directory: &Path) -> String {
    format!("--user-data-dir={}", directory.display())
}

/// Focus the exact running profile by activating its own process through AppKit. This cannot
/// launch Chrome: a gone process returns None. No Apple Events or privacy prompt is involved.
#[cfg(target_os = "macos")]
fn show(pid: u32) -> Result<(), String> {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    let application = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
        .ok_or(
            "Chrome is no longer running for this profile; nothing was activated.".to_string(),
        )?;
    if application.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows) {
        Ok(())
    } else {
        Err("macOS did not activate this Chrome profile; open it from Chrome.".into())
    }
}

#[cfg(not(target_os = "macos"))]
fn show(_pid: u32) -> Result<(), String> {
    Err("Showing a managed Chrome profile is available only on macOS.".into())
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
        binding_origin: None,
        image_worker_generation: None,
        video_worker_generation: None,
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

/// A resident or closing profile may keep its recorded binding or fill a missing one; it may not
/// swap or clear a recorded binding while its Chrome process could still hold a job.
fn updated_binding(
    current: &Option<String>,
    next: Option<String>,
    frozen: bool,
) -> Result<Option<String>, String> {
    if frozen && current.is_some() && *current != next {
        return Err(
            "Worker bindings stay fixed while this Chrome profile is open or closing.".into(),
        );
    }
    Ok(next)
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

fn validate_generation(value: Option<&str>) -> Result<(), String> {
    let Some(value) = value else { return Ok(()) };
    if value.is_empty() || value.chars().count() > WORKER_ID_LIMIT {
        return Err(
            "Saved worker credential generations are invalid; the file was not changed.".into(),
        );
    }
    if value.chars().any(char::is_control) {
        return Err(
            "Saved worker credential generations cannot contain control characters.".into(),
        );
    }
    Ok(())
}

/// The binding origin is the already-normalized saved Gateway origin: scheme, host, optional
/// port, and no path, credentials, query or fragment.
fn validate_binding_origin(value: &str) -> Result<(), String> {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    let valid = rest.is_some_and(|rest| {
        !rest.is_empty()
            && !rest
                .chars()
                .any(|c| matches!(c, '/' | '?' | '#' | '@') || c.is_control() || c.is_whitespace())
    });
    if !valid || value != value.trim() {
        return Err("A saved worker binding origin is invalid; the file was not changed.".into());
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
        validate_generation(profile.image_worker_generation.as_deref())?;
        validate_generation(profile.video_worker_generation.as_deref())?;
        match (
            &profile.binding_origin,
            &profile.image_worker_generation,
            &profile.video_worker_generation,
        ) {
            (None, None, None) => {}
            (Some(origin), Some(_), Some(_)) => {
                validate_binding_origin(origin)?;
                let image = profile
                    .image_worker_id
                    .as_deref()
                    .filter(|id| !id.is_empty());
                let video = profile
                    .video_worker_id
                    .as_deref()
                    .filter(|id| !id.is_empty());
                if image.is_none() || video.is_none() || image == video {
                    return Err(
                        "A saved worker binding needs distinct image and video workers; the file was not changed."
                            .into(),
                    );
                }
            }
            _ => {
                return Err(
                    "A saved worker credential-generation mapping is incomplete; the file was not changed."
                        .into(),
                );
            }
        }
        if profile.pid == Some(0) {
            return Err(
                "A saved browser process identifier is invalid; the file was not changed.".into(),
            );
        }
    }
    if stored.next_number == 0 || stored.next_number <= highest {
        return Err("Saved browser profile numbering is invalid; the file was not changed.".into());
    }
    let pause = &stored.settings;
    if !pause.pause_min_seconds.is_finite()
        || !pause.pause_max_seconds.is_finite()
        || pause.pause_min_seconds < 0.0
        || pause.pause_max_seconds < 0.0
        || pause.pause_min_seconds > pause.pause_max_seconds
    {
        return Err("Saved pause range is invalid; the file was not changed.".into());
    }
    Ok(())
}

fn parse_state(bytes: &[u8]) -> Result<Stored, String> {
    let mut stored: Stored = serde_json::from_slice(bytes).map_err(|_| {
        "Saved browser profiles are unreadable. The file has not been reset.".to_string()
    })?;
    validate_state(&stored)?;
    // A locally disabled profile may never carry open intent, including from older or hand-edited
    // state. Normalize it off in memory; the retained profile is persisted on the next save.
    for profile in &mut stored.profiles {
        if !profile.enabled {
            profile.desired_open = false;
        }
    }
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
            binding_origin: None,
            image_worker_generation: None,
            video_worker_generation: None,
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
        assert_eq!(ownership(&row, &profile, None), Ownership::Verified);

        profile.pid_started = None;
        assert!(
            !owned_process(&row, &profile, None),
            "a missing start time is never adopted as ownership"
        );
        assert_eq!(ownership(&row, &profile, None), Ownership::Unverified);

        profile.pid_started = Some("Thu Nov 12 00:42:11 2026".into());
        assert!(
            !owned_process(&row, &profile, None),
            "a reused pid must not match"
        );
        assert_eq!(ownership(&row, &profile, None), Ownership::Reused);

        profile.pid_started = Some(row.started.clone());
        profile.binary = Some("/Applications/Other.app/Contents/MacOS/Other".into());
        assert!(!owned_process(&row, &profile, None));
        assert_eq!(ownership(&row, &profile, None), Ownership::Reused);
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
        stored.settings.pause_min_seconds = 20.0;
        stored.settings.pause_max_seconds = 5.0;
        assert!(validate_state(&stored).is_err());
    }

    #[test]
    fn fractional_pause_range_roundtrips_and_keeps_integer_json_readable() {
        let state = r#"{"schema":1,"next_number":1,"profiles":[],"settings":{"pause_min_seconds":1.5,"pause_max_seconds":2.75}}"#;
        let stored = parse_state(state.as_bytes()).expect("fractional state loads");
        assert_eq!(stored.settings.pause_min_seconds, 1.5);
        let saved = serde_json::to_vec(&stored).expect("state encodes");
        let reloaded = parse_state(&saved).expect("state reloads");
        assert_eq!(reloaded.settings.pause_min_seconds, 1.5);
        assert_eq!(reloaded.settings.pause_max_seconds, 2.75);

        let integer = r#"{"schema":1,"next_number":1,"profiles":[],"settings":{"pause_min_seconds":5,"pause_max_seconds":15}}"#;
        let stored = parse_state(integer.as_bytes()).expect("integer state loads");
        assert_eq!(stored.settings.pause_min_seconds, 5.0);
        assert_eq!(stored.settings.pause_max_seconds, 15.0);

        let snapshot = SettingsSnapshot {
            memory_budget_mb: None,
            reserve_mb: 2048,
            max_active: None,
            pause_min_seconds: 1.5,
            pause_max_seconds: 2.75,
        };
        let json = serde_json::to_value(&snapshot).expect("snapshot encodes");
        assert_eq!(json["pause_min_seconds"], 1.5);
        assert_eq!(json["pause_max_seconds"], 2.75);
    }

    #[test]
    fn pause_range_rejects_negative_non_finite_and_inverted_values_but_keeps_zero() {
        let mut stored = Stored::default();
        stored.settings.pause_min_seconds = -0.5;
        stored.settings.pause_max_seconds = 2.0;
        assert!(validate_state(&stored).is_err());

        stored.settings.pause_min_seconds = f64::NAN;
        assert!(validate_state(&stored).is_err());

        stored.settings.pause_min_seconds = 1.5;
        stored.settings.pause_max_seconds = 1.25;
        assert!(validate_state(&stored).is_err());

        stored.settings.pause_max_seconds = 2.75;
        assert!(validate_state(&stored).is_ok());
        stored.settings.pause_min_seconds = 0.0;
        stored.settings.pause_max_seconds = 0.0;
        assert!(
            validate_state(&stored).is_ok(),
            "explicit zero is preserved"
        );
    }

    #[test]
    fn generated_identifier_is_opaque_and_stable_width() {
        let id = new_id(&[], 7);
        assert_eq!(id.len(), 24);
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    const NOW: f64 = 1794350060.0;

    fn worker(id: &str) -> serde_json::Value {
        serde_json::json!({
            "worker_id": id,
            "kind": "image",
            "has_live_token": true,
            "control": {"dispatchEnabled": false},
            "state": {
                "connection": "connected",
                "sign_in": "signed_in",
                "readiness": "ready",
                "activity": "idle",
                "observed_at": NOW - 5.0,
                "next_eligible_at": NOW + 30.0
            },
            "receivedAt": NOW - 2.0,
            "fresh": true,
            "held_jobs": [],
            "durableBusy": false,
            "safeToClose": false
        })
    }

    fn account(provider: &str, id: &str, workers: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "provider": provider,
            "account_id": id,
            "balance": {"value": 10.0, "unit": "credits", "source": "gateway", "observed_at": NOW - 5.0},
            "fresh": true,
            "gateway_enabled": true,
            "reserved": 4.0,
            "available": -3.0,
            "workers": workers
        })
    }

    fn manager_view(
        workers: Vec<serde_json::Value>,
        accounts: Vec<serde_json::Value>,
    ) -> manager_gateway::ManagerView {
        serde_json::from_value(serde_json::json!({
            "schema": 1,
            "receivedAt": NOW,
            "maxAgeSeconds": 90.0,
            "workers": workers,
            "accounts": accounts
        }))
        .expect("manager view fixture")
    }

    fn profile(image: Option<&str>, video: Option<&str>) -> GatewayProfile {
        GatewayProfile {
            id: "profile-1".into(),
            image_worker_id: image.map(ToString::to_string),
            video_worker_id: video.map(ToString::to_string),
            binding_origin: None,
            image_worker_generation: None,
            video_worker_generation: None,
        }
    }

    fn bound_profile() -> GatewayProfile {
        GatewayProfile {
            id: "profile-1".into(),
            image_worker_id: Some("image-1".into()),
            video_worker_id: Some("video-1".into()),
            binding_origin: Some("https://gateway.example".into()),
            image_worker_generation: Some("gen-image".into()),
            video_worker_generation: Some("gen-video".into()),
        }
    }

    #[test]
    fn manager_view_aggregates_both_workers_and_exposes_gateway_spend_at_root() {
        let view = manager_view(
            vec![worker("image-1"), worker("video-1")],
            vec![account("flow", "account-1", &["image-1", "video-1"])],
        );
        let reading = reading_for_at(&view, &profile(Some("image-1"), Some("video-1")), NOW);
        assert_eq!(reading.connection.as_deref(), Some("connected"));
        assert_eq!(reading.account_id.as_deref(), Some("account-1"));
        assert_eq!(reading.observed_at, Some((NOW - 5.0).to_string()));
        assert_eq!(reading.next_eligible_at, Some((NOW + 30.0).to_string()));
        assert!(!reading.attention);

        let root = gateway_snapshot(&sample_profile(), Some(&reading));
        assert_eq!(root.connection, "connected");
        assert_eq!(root.gateway_enabled, Some(true));
        assert_eq!(root.reserved, Some(4.0));
        assert_eq!(
            root.available,
            Some(-3.0),
            "an observed available deficit stays negative"
        );
        let credits = reading.credits.expect("credits");
        assert_eq!(credits["balance"]["value"], 10.0);
        assert_eq!(credits["gateway_enabled"], true);
    }

    #[test]
    fn stale_or_absent_worker_telemetry_stays_unknown() {
        let mut stale = worker("image-1");
        stale["fresh"] = serde_json::json!(false);
        let view = manager_view(
            vec![stale],
            vec![account("flow", "account-1", &["image-1"])],
        );
        let reading = reading_for_at(&view, &profile(Some("image-1"), None), NOW);
        assert!(reading.connection.is_none(), "stale state is not current");
        assert!(reading.activity.is_none());

        // A two-worker profile cannot claim aggregate state from only one worker.
        let view = manager_view(
            vec![worker("image-1")],
            vec![account("flow", "account-1", &["image-1"])],
        );
        let reading = reading_for_at(&view, &profile(Some("image-1"), Some("video-1")), NOW);
        assert!(reading.connection.is_none());
        assert!(reading.next_eligible_at.is_none());

        // A stale view is entirely unknown, including its accounts.
        let mut view = manager_view(
            vec![worker("image-1")],
            vec![account("flow", "account-1", &["image-1"])],
        );
        view.received_at = NOW - 1000.0;
        let reading = reading_for_at(&view, &profile(Some("image-1"), None), NOW);
        assert!(reading.connection.is_none());
        assert!(reading.account_id.is_none());
        assert!(reading.credits.is_none());

        let unbound = reading_for_at(&view, &profile(None, None), NOW);
        assert!(unbound.account_id.is_none());
        assert!(unbound.credits.is_none());
    }

    #[test]
    fn mismatched_worker_accounts_are_unknown_and_need_attention() {
        let view = manager_view(
            vec![worker("image-1"), worker("video-1")],
            vec![
                account("flow", "account-1", &["image-1"]),
                account("flow", "account-2", &["video-1"]),
            ],
        );
        let reading = reading_for_at(&view, &profile(Some("image-1"), Some("video-1")), NOW);
        assert!(reading.account_id.is_none());
        assert!(reading.credits.is_none());
        assert!(reading.gateway_enabled.is_none());
        assert!(reading.reserved.is_none());
        assert!(reading.available.is_none());
        assert!(
            reading.attention,
            "different accounts must not share credits"
        );
    }

    #[test]
    fn disabled_profile_with_saved_open_intent_loads_closed_and_keeps_its_data() {
        let state = r#"{
            "schema": 1,
            "next_number": 2,
            "profiles": [
                {"id":"0123456789abcdef01234567","number":1,"label":"A","enabled":false,"desired_open":true}
            ],
            "settings": {}
        }"#;
        let stored = parse_state(state.as_bytes()).expect("state loads");
        assert!(!stored.profiles[0].desired_open);
        assert_eq!(stored.profiles[0].label, "A");
        assert_eq!(stored.profiles[0].number, 1);
    }

    #[test]
    fn worker_bindings_freeze_only_for_a_resident_or_closing_profile() {
        let recorded = Some("worker-a".to_string());
        assert!(updated_binding(&recorded, Some("worker-b".into()), true).is_err());
        assert!(updated_binding(&recorded, None, true).is_err());
        assert_eq!(
            updated_binding(&recorded, recorded.clone(), true).unwrap(),
            recorded
        );
        // A missing initial binding may still be filled so a close is not deadlocked.
        assert_eq!(
            updated_binding(&None, Some("worker-a".into()), true).unwrap(),
            Some("worker-a".to_string())
        );
        assert_eq!(
            updated_binding(&recorded, Some("worker-b".into()), false).unwrap(),
            Some("worker-b".to_string())
        );
    }

    #[test]
    fn delayed_close_response_cannot_restore_an_earlier_safe_revision() {
        let bindings = expected_bindings(&bound_profile(), "https://gateway.example").unwrap();
        let earlier = pair_view();
        let mut later = earlier.clone();
        later.received_at += 1.0;
        for worker in &mut later.workers {
            worker.control.revision = Some("rev-2".into());
            worker.state.as_mut().unwrap().control_revision = Some("rev-2".into());
        }
        let mut cache = GatewayCache::default();
        assert!(cache.publish_view(later.clone()));
        assert!(!cache.publish_view(earlier.clone()));
        let mut equally_dated = earlier.clone();
        equally_dated.received_at = later.received_at;
        assert!(!cache.publish_view(equally_dated));
        let mut attempt = CloseAttempt {
            id: 1,
            origin: "https://gateway.example".into(),
            bindings: bindings.clone(),
            revisions: response_revisions(&earlier, &bindings).unwrap(),
            response_received_at: Some(earlier.received_at),
        };
        assert!(
            manager_gateway::close_safety_for_bindings_at(&earlier, &bindings, NOW).safe_to_close
        );
        assert!(attempt_revision_mismatch(cache.view.as_ref().unwrap(), &attempt).is_some());
        assert!(attempt_revision_mismatch(&earlier, &attempt).is_none());
        attempt.response_received_at = Some(earlier.received_at + 1.0);
        assert!(attempt_revision_mismatch(&earlier, &attempt).is_some());
        attempt.response_received_at = None;
        assert!(attempt_revision_mismatch(&later, &attempt).is_some());
    }

    #[test]
    fn cache_view_is_discarded_when_the_saved_origin_changes() {
        let view = manager_view(vec![worker("image-1")], vec![]);
        let mut cache = GatewayCache {
            origin: Some("https://old.example".into()),
            view: Some(view.clone()),
            last_view: Some(Instant::now()),
            ..GatewayCache::default()
        };
        assert!(cache.view_for(Some("https://new.example")).is_none());
        assert!(cache.view.is_none(), "the old-origin view was invalidated");

        cache.origin = Some("https://old.example".into());
        cache.view = Some(view);
        assert!(cache.view_for(Some("https://old.example")).is_some());
        assert!(cache.view_for(None).is_none());
        assert!(cache.view.is_none(), "no view survives an unset origin");
    }

    #[test]
    fn launch_targets_the_profiles_own_user_data_directory_without_extra_flags() {
        let directory = Path::new(
            "/Users/x/Library/Application Support/relay/browser-profiles/0123456789abcdef01234567",
        );
        let arg = user_data_arg(directory);
        assert_eq!(
            arg,
            "--user-data-dir=/Users/x/Library/Application Support/relay/browser-profiles/0123456789abcdef01234567"
        );
        assert!(!arg.contains("disable"));
        assert!(!arg.contains("original"));
    }

    #[test]
    fn resolved_holds_clear_without_clearing_ownership_or_safety_errors() {
        let mut errors = HashMap::new();
        let mut attention = HashSet::new();
        errors.insert("p".to_string(), IDENTITY_HOLD.to_string());
        attention.insert("p".to_string());
        clear_resolved_errors(&mut errors, &mut attention, "p");
        assert!(
            errors.is_empty() && attention.is_empty(),
            "verified opens the hold"
        );

        errors.insert(
            "p".to_string(),
            "This profile's Chrome process is not verified open, so nothing was activated.".into(),
        );
        clear_resolved_errors(&mut errors, &mut attention, "p");
        assert!(errors.is_empty(), "a successful show clears its own error");

        errors.insert("p".to_string(), "Waiting for Gateway safety: video.".into());
        clear_resolved_errors(&mut errors, &mut attention, "p");
        assert!(
            errors.contains_key("p"),
            "a close-safety hold is not a transient show error"
        );
    }

    #[test]
    fn pairing_requires_the_saved_gateway_origin() {
        assert!(required_origin(None).is_err());
        assert_eq!(
            required_origin(Some("https://gateway.example".into())).unwrap(),
            "https://gateway.example"
        );
    }

    #[test]
    fn worker_ids_aggregate_each_explicit_binding_once() {
        let same = GatewayProfile {
            id: "profile-1".into(),
            image_worker_id: Some("same".into()),
            video_worker_id: Some("same".into()),
            binding_origin: None,
            image_worker_generation: None,
            video_worker_generation: None,
        };
        assert_eq!(worker_ids(&same), vec!["same".to_string()]);
        let different = profile(Some("image-1"), Some("video-1"));
        assert_eq!(
            worker_ids(&different),
            vec!["image-1".to_string(), "video-1".to_string()]
        );
    }

    fn bound_worker(id: &str, kind: &str, generation: &str, revision: &str) -> serde_json::Value {
        serde_json::json!({
            "worker_id": id,
            "kind": kind,
            "has_live_token": true,
            "control": {
                "dispatchEnabled": false,
                "closeRequested": true,
                "credentialGeneration": generation,
                "revision": revision
            },
            "state": {
                "activity": "idle",
                "paused": true,
                "observed_at": NOW - 5.0,
                "control_revision": revision
            },
            "receivedAt": NOW - 2.0,
            "fresh": true,
            "current_job": null,
            "held_jobs": [],
            "durableBusy": false,
            "safeToClose": true
        })
    }

    fn pair_view() -> manager_gateway::ManagerView {
        manager_view(
            vec![
                bound_worker("image-1", "image", "gen-image", "rev-1"),
                bound_worker("video-1", "video", "gen-video", "rev-1"),
            ],
            vec![account("flow", "account-1", &["image-1", "video-1"])],
        )
    }

    #[test]
    fn worker_generation_mapping_roundtrips_and_rejects_partial_or_invalid_data() {
        let mut stored = Stored::default();
        append_profile(&mut stored, "Bound").expect("profile");
        let profile = &mut stored.profiles[0];
        profile.image_worker_id = Some("image-1".into());
        profile.video_worker_id = Some("video-1".into());
        profile.binding_origin = Some("https://gateway.example:8787".into());
        profile.image_worker_generation = Some("gen-image".into());
        profile.video_worker_generation = Some("gen-video".into());
        assert!(validate_state(&stored).is_ok());

        let saved = serde_json::to_vec(&stored).expect("state encodes");
        let loaded = parse_state(&saved).expect("state reloads");
        assert_eq!(
            loaded.profiles[0].binding_origin.as_deref(),
            Some("https://gateway.example:8787")
        );
        assert_eq!(
            loaded.profiles[0].video_worker_generation.as_deref(),
            Some("gen-video")
        );
        // Older state without the new optional fields still loads as unknown/held.
        assert!(
            parse_state(br#"{"schema":1,"next_number":1,"profiles":[],"settings":{}}"#).is_ok()
        );

        let mut partial = stored.clone();
        partial.profiles[0].image_worker_generation = None;
        assert!(validate_state(&partial).is_err());

        let mut same_worker = stored.clone();
        same_worker.profiles[0].image_worker_id = Some("video-1".into());
        assert!(validate_state(&same_worker).is_err());

        let mut bad_origin = stored.clone();
        bad_origin.profiles[0].binding_origin = Some("https://gateway.example/path".into());
        assert!(validate_state(&bad_origin).is_err());
    }

    #[test]
    fn only_a_complete_matching_live_pair_captures_generations() {
        let view = pair_view();
        assert_eq!(
            verified_generations(&view, Some("image-1"), Some("video-1"), NOW),
            Some(("gen-image".into(), "gen-video".into()))
        );
        assert!(
            verified_generations(&view, Some("image-1"), Some("image-1"), NOW).is_none(),
            "one worker cannot fill both kinds"
        );
        assert!(
            verified_generations(&view, Some("image-1"), None, NOW).is_none(),
            "a missing sibling stays unknown"
        );

        let wrong_kind = manager_view(
            vec![
                bound_worker("image-1", "video", "gen-image", "rev-1"),
                bound_worker("video-1", "video", "gen-video", "rev-1"),
            ],
            vec![],
        );
        assert!(verified_generations(&wrong_kind, Some("image-1"), Some("video-1"), NOW).is_none());

        let without_generation = manager_view(
            vec![
                worker("image-1"),
                bound_worker("video-1", "video", "gen-video", "rev-1"),
            ],
            vec![],
        );
        assert!(
            verified_generations(&without_generation, Some("image-1"), Some("video-1"), NOW)
                .is_none(),
            "a missing generation is never invented"
        );

        let mut stale = pair_view();
        stale.received_at = NOW - 1000.0;
        assert!(verified_generations(&stale, Some("image-1"), Some("video-1"), NOW).is_none());
    }

    #[test]
    fn pacing_application_requires_every_bound_current_worker_and_saved_range() {
        let profiles = vec![bound_profile()];
        let requested = manager_gateway::Pacing {
            min_seconds: 1.5,
            max_seconds: 2.75,
        };
        let mut view = pair_view();
        for worker in &mut view.workers {
            worker.control.pacing = Some(requested.clone());
            worker.state.as_mut().unwrap().pacing = Some(requested.clone());
        }
        let applied = |view: &manager_gateway::ManagerView| {
            pacing_is_applied(view, &profiles, "https://gateway.example", &requested, NOW)
        };
        assert!(applied(&view));
        let mut missing = view.clone();
        missing.workers.pop();
        assert!(!applied(&missing));
        let mut different = view.clone();
        different.workers[1]
            .state
            .as_mut()
            .unwrap()
            .pacing
            .as_mut()
            .unwrap()
            .max_seconds = 15.0;
        assert!(!applied(&different), "one matching worker is insufficient");
        let mut old_revision = view.clone();
        old_revision.workers[1]
            .state
            .as_mut()
            .unwrap()
            .control_revision = Some("older".into());
        assert!(!applied(&old_revision));
        let mut old_source = view.clone();
        old_source.workers[1].state.as_mut().unwrap().observed_at =
            Some(NOW - view.max_age_seconds - 1.0);
        assert!(!applied(&old_source));
        let mut changed_generation = view.clone();
        changed_generation.workers[1].control.credential_generation = Some("replacement".into());
        assert!(!applied(&changed_generation));
        assert!(!pacing_is_applied(
            &view,
            &[],
            "https://gateway.example",
            &requested,
            NOW
        ));
        assert!(!pacing_is_applied(
            &view,
            &profiles,
            "https://other.example",
            &requested,
            NOW
        ));
        assert!(pacing_worker_ids(
            &view,
            &[profiles[0].clone(), profiles[0].clone()],
            "https://gateway.example",
            NOW
        )
        .is_err());
    }

    #[test]
    fn expected_bindings_require_two_kinds_generations_and_the_saved_origin() {
        let profile = bound_profile();
        let bindings = expected_bindings(&profile, "https://gateway.example").expect("bindings");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].worker_id, "image-1");
        assert_eq!(bindings[0].kind, "image");
        assert_eq!(bindings[0].credential_generation, "gen-image");
        assert_eq!(bindings[1].worker_id, "video-1");
        assert_eq!(bindings[1].kind, "video");
        assert_eq!(bindings[1].credential_generation, "gen-video");

        let mut same_worker = profile.clone();
        same_worker.video_worker_id = same_worker.image_worker_id.clone();
        assert!(expected_bindings(&same_worker, "https://gateway.example").is_err());

        let mut only_image = profile.clone();
        only_image.video_worker_id = None;
        assert!(
            expected_bindings(&only_image, "https://gateway.example").is_err(),
            "one physical sibling keeps close pending"
        );

        let mut unknown_generation = profile.clone();
        unknown_generation.video_worker_generation = None;
        assert!(expected_bindings(&unknown_generation, "https://gateway.example").is_err());

        assert!(
            expected_bindings(&profile, "https://other.example").is_err(),
            "another origin invalidates the binding"
        );
    }

    #[test]
    fn pairing_fills_only_a_missing_initial_binding() {
        let mut stored = Stored::default();
        append_profile(&mut stored, "Pair").expect("profile");
        stored.profiles[0].image_worker_id = Some("image-1".into());
        stored.profiles[0].video_worker_id = Some("video-1".into());

        fill_initial_bindings(&mut stored, "https://gateway.example", &pair_view(), NOW);
        assert_eq!(
            stored.profiles[0].image_worker_generation.as_deref(),
            Some("gen-image")
        );
        assert_eq!(
            stored.profiles[0].binding_origin.as_deref(),
            Some("https://gateway.example")
        );

        // A recorded binding is never silently replaced, including when the view disagrees.
        stored.profiles[0].image_worker_generation = Some("gen-old".into());
        stored.profiles[0].video_worker_generation = Some("gen-old-video".into());
        fill_initial_bindings(&mut stored, "https://gateway.example", &pair_view(), NOW);
        assert_eq!(
            stored.profiles[0].image_worker_generation.as_deref(),
            Some("gen-old")
        );
        assert_eq!(
            stored.profiles[0].video_worker_generation.as_deref(),
            Some("gen-old-video")
        );
    }

    #[test]
    fn control_response_must_cover_both_live_workers_with_this_attempt_revision() {
        let bindings =
            expected_bindings(&bound_profile(), "https://gateway.example").expect("bindings");
        let view = pair_view();
        let revisions = response_revisions(&view, &bindings).expect("both workers acknowledged");
        assert_eq!(revisions.get("image-1").map(String::as_str), Some("rev-1"));
        assert_eq!(revisions.get("video-1").map(String::as_str), Some("rev-1"));

        let partial = manager_view(
            vec![bound_worker("image-1", "image", "gen-image", "rev-1")],
            vec![],
        );
        assert!(
            response_revisions(&partial, &bindings).is_err(),
            "a partial response holds close"
        );

        let wrong_generation = manager_view(
            vec![
                bound_worker("image-1", "image", "gen-new", "rev-1"),
                bound_worker("video-1", "video", "gen-video", "rev-1"),
            ],
            vec![],
        );
        assert!(response_revisions(&wrong_generation, &bindings).is_err());

        let missing_revision = manager_view(
            vec![
                bound_worker("image-1", "image", "gen-image", ""),
                bound_worker("video-1", "video", "gen-video", ""),
            ],
            vec![],
        );
        assert!(response_revisions(&missing_revision, &bindings).is_err());
    }

    #[test]
    fn attempt_never_inherits_a_later_control_revision_or_another_binding() {
        let profile = bound_profile();
        let bindings = expected_bindings(&profile, "https://gateway.example").expect("bindings");
        let attempt = CloseAttempt {
            id: 1,
            origin: "https://gateway.example".into(),
            bindings: bindings.clone(),
            revisions: HashMap::from([
                ("image-1".to_string(), "rev-7".to_string()),
                ("video-1".to_string(), "rev-7".to_string()),
            ]),
            response_received_at: Some(NOW),
        };
        let issued = manager_view(
            vec![
                bound_worker("image-1", "image", "gen-image", "rev-7"),
                bound_worker("video-1", "video", "gen-video", "rev-7"),
            ],
            vec![],
        );
        assert!(attempt_revision_mismatch(&issued, &attempt).is_none());
        assert!(attempt_is_current(
            &attempt,
            "https://gateway.example",
            &bindings
        ));
        assert!(
            !attempt_is_current(&attempt, "https://other.example", &bindings),
            "origin change invalidates the attempt"
        );
        let mut rebound = bindings.clone();
        rebound[1].credential_generation = "gen-new".into();
        assert!(
            !attempt_is_current(&attempt, "https://gateway.example", &rebound),
            "binding change invalidates the attempt"
        );

        let later = manager_view(
            vec![
                bound_worker("image-1", "image", "gen-image", "rev-8"),
                bound_worker("video-1", "video", "gen-video", "rev-8"),
            ],
            vec![],
        );
        // The typed safety helper alone would accept the later self-consistent revision; the
        // attempt anchor rejects it because it is not the revision THIS close requested.
        assert!(
            manager_gateway::close_safety_for_bindings_at(&later, &bindings, NOW).safe_to_close
        );
        assert!(attempt_revision_mismatch(&later, &attempt).is_some());
    }
}
