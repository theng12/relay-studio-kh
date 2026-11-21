//! Update hold — the app side of `_orchestra/youtube-stack/update-hold/CONTRACT.md`.
//!
//! A hold closes Relay's start path first (no new Chrome begins, so no new claiming worker),
//! then waits a bounded moment for the work already inside a launch or a close to settle, and
//! answers whether a restart now would strand anything. The pause is one instant in memory and
//! nothing else: DELETE clears it at once, it expires on its own after `pause_s`, and a restart
//! always starts open — an install that aborts can never leave Relay refusing work.

use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{Manager, State, WebviewWindow};

use crate::browser_manager::BrowserManager;

// Contract bounds: settle within 20 s, never wait past 30; the pause defaults to two minutes.
const DEFAULT_TIMEOUT_S: f64 = 20.0;
const MAX_TIMEOUT_S: f64 = 30.0;
const DEFAULT_PAUSE_S: f64 = 120.0;
const MIN_PAUSE_S: f64 = 1.0;
const MAX_PAUSE_S: f64 = 900.0;
const POLL: Duration = Duration::from_millis(250);
pub(crate) const PORT: u16 = 8648;

/// One thing Relay is inside right now. `resumable` says the state file already carries the id
/// the next start continues (`close_pending` or `desired_open`), so a restart loses nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InFlight {
    pub id: String,
    pub resumable: bool,
}

/// The reply shape every app in the contract returns, key for key.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Reply {
    pub safe: bool,
    pub unsettled: Vec<String>,
    pub dispatch_paused_s: f64,
    pub in_flight: u32,
    /// Accounts with `desired_open` that are not open yet: they resume by themselves, so they
    /// never make `safe` false, but they mean the hold is not idle.
    pub waiting: u32,
}

/// The pause: an instant, in memory, never written down anywhere.
#[derive(Default)]
pub(crate) struct Hold {
    paused_until: Option<Instant>,
}

impl Hold {
    fn pause_for(&mut self, pause_s: f64) {
        self.paused_until = Some(Instant::now() + secs(pause_s));
    }
    fn resume(&mut self) {
        self.paused_until = None;
    }
    fn paused_at(&self, now: Instant) -> bool {
        self.paused_until.is_some_and(|until| now < until)
    }
    pub(crate) fn is_paused(&self) -> bool {
        self.paused_at(Instant::now())
    }
}

static HOLD: Mutex<Hold> = Mutex::new(Hold { paused_until: None });

fn lock() -> std::sync::MutexGuard<'static, Hold> {
    HOLD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn secs(seconds: f64) -> Duration {
    Duration::from_secs_f64(if seconds.is_finite() && seconds > 0.0 { seconds } else { 0.0 })
}

fn bounded(value: Option<f64>, default: f64, min: f64, max: f64) -> f64 {
    match value {
        Some(value) if value.is_finite() => value.clamp(min, max),
        _ => default,
    }
}

/// The claim path's switch, read by the launch loop on every profile it considers.
pub(crate) fn paused() -> bool {
    lock().is_paused()
}

fn wait_for_settle(
    timeout_s: f64,
    mut work: impl FnMut() -> Result<Vec<InFlight>, String>,
) -> Result<Vec<InFlight>, String> {
    let deadline = Instant::now() + secs(timeout_s);
    let mut current = work()?;
    while !current.is_empty() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        thread::sleep(POLL.min(left));
        current = work()?;
    }
    Ok(current)
}

fn reply(current: Vec<InFlight>, pause_s: f64, waiting: u32) -> Reply {
    let unsettled: Vec<String> = current
        .iter()
        .filter(|item| !item.resumable)
        .map(|item| item.id.clone())
        .collect();
    Reply {
        safe: unsettled.is_empty(),
        unsettled,
        dispatch_paused_s: pause_s,
        in_flight: current.len() as u32,
        waiting,
    }
}

fn hold_taken_line(answer: &Reply) -> String {
    format!(
        "update hold taken: in_flight={}, waiting={}, dispatch paused {}s",
        answer.in_flight, answer.waiting, answer.dispatch_paused_s
    )
}

fn settled_line(answer: &Reply) -> String {
    if !answer.unsettled.is_empty() {
        format!(
            "update hold unsettled: in_flight={}, waiting={}: {}",
            answer.in_flight,
            answer.waiting,
            answer.unsettled.join(", ")
        )
    } else if answer.in_flight == 0 && answer.waiting == 0 {
        "update hold settled: in_flight=0, waiting=0".to_string()
    } else {
        format!(
            "update hold settled: in_flight={}, waiting={}, every id follows",
            answer.in_flight, answer.waiting
        )
    }
}

/// The hold request: close the claim path first, then wait for the work already inside.
pub(crate) fn take(
    timeout_s: Option<f64>,
    pause_s: Option<f64>,
    work: impl FnMut() -> Result<Vec<InFlight>, String>,
) -> Result<Reply, String> {
    take_with_waiting(timeout_s, pause_s, || Ok(0), work)
}

/// The hold request with the queued-account count injected by the caller.
pub(crate) fn take_with_waiting(
    timeout_s: Option<f64>,
    pause_s: Option<f64>,
    waiting: impl Fn() -> Result<u32, String>,
    work: impl FnMut() -> Result<Vec<InFlight>, String>,
) -> Result<Reply, String> {
    let pause = bounded(pause_s, DEFAULT_PAUSE_S, MIN_PAUSE_S, MAX_PAUSE_S);
    let timeout = bounded(timeout_s, DEFAULT_TIMEOUT_S, 0.0, MAX_TIMEOUT_S);
    {
        // The guard ends with this block. A hold must never wait while holding the pause lock:
        // reconcile reads it under the state lock, so waiting here would deadlock the two.
        lock().pause_for(pause);
    }
    // A failure to read work leaves the pause standing; it still expires by itself.
    let waiting = waiting()?;
    Ok(reply(wait_for_settle(timeout, work)?, pause, waiting))
}

/// The release request: resume at once.
pub(crate) fn release(
    work: impl FnMut() -> Result<Vec<InFlight>, String>,
) -> Result<Reply, String> {
    release_with_waiting(|| Ok(0), work)
}

pub(crate) fn release_with_waiting(
    waiting: impl Fn() -> Result<u32, String>,
    mut work: impl FnMut() -> Result<Vec<InFlight>, String>,
) -> Result<Reply, String> {
    lock().resume();
    Ok(reply(work()?, 0.0, waiting()?))
}

#[tauri::command]
pub(crate) fn maintenance_hold(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    timeout_s: Option<f64>,
    pause_s: Option<f64>,
) -> Result<Reply, String> {
    super::local_only(&window)?;
    let answer = take_with_waiting(
        timeout_s,
        pause_s,
        || crate::browser_manager::waiting_count(&manager),
        || crate::browser_manager::hold_work(&manager),
    )?;
    crate::browser_manager::log_line(&manager, &hold_taken_line(&answer));
    crate::browser_manager::log_line(&manager, &settled_line(&answer));
    Ok(answer)
}

#[tauri::command]
pub(crate) fn maintenance_release(
    _window: WebviewWindow,
    manager: State<'_, BrowserManager>,
) -> Result<Reply, String> {
    release_for(&manager)
}

fn release_for(manager: &BrowserManager) -> Result<Reply, String> {
    let answer = release_with_waiting(
        || crate::browser_manager::waiting_count(manager),
        || crate::browser_manager::hold_work(manager),
    )?;
    crate::browser_manager::log_line(manager, "update hold resumed");
    Ok(answer)
}

/// A web page must not drive the hold: a request carrying an `Origin` is a browser, and only
/// Relay's own page (its `tauri://` webview or a loopback page) may send one. No `Origin` at all
/// is a non-browser caller, which is what the updater is.
fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    match url::Url::parse(origin) {
        Ok(url) if url.scheme() == "tauri" => true,
        Ok(url) => matches!(url.host_str(), Some("localhost") | Some("127.0.0.1") | Some("[::1]")),
        Err(_) => false,
    }
}

/// The installer speaks HTTP, not Tauri IPC. Serve the contract on a fixed loopback port.
pub(crate) fn start_http_listener(app: &tauri::AppHandle) {
    let server = match tiny_http::Server::http(("127.0.0.1", PORT)) {
        Ok(server) => Arc::new(server),
        Err(error) => {
            crate::browser_manager::log_line(
                &app.state::<BrowserManager>(),
                &format!("update hold unavailable: could not listen on 127.0.0.1:{PORT}: {error}"),
            );
            return;
        }
    };
    let thread_app = app.clone();
    if thread::Builder::new()
        .name("relay-update-hold".into())
        .spawn(move || {
            let manager = thread_app.state::<BrowserManager>();
            crate::browser_manager::log_line(&manager, "update hold listening on 127.0.0.1:8648");
            for request in server.incoming_requests() {
                let origin = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("Origin"))
                    .map(|header| header.value.as_str());
                if !origin_allowed(origin) {
                    crate::browser_manager::log_line(
                        &manager,
                        &format!("update hold refused a cross-origin request from {}", origin.unwrap_or("")),
                    );
                    let _ = request.respond(
                        tiny_http::Response::from_data(
                            b"Cross-origin requests cannot change the update hold.".to_vec(),
                        )
                        .with_status_code(403)
                        .with_header(
                            tiny_http::Header::from_bytes("Content-Type", "text/plain").expect("header"),
                        ),
                    );
                    continue;
                }
                let path = request.url().split('?').next().unwrap_or("");
                if path != "/api/maintenance/hold" {
                    let _ = request.respond(tiny_http::Response::from_data(b"Not found".to_vec()).with_status_code(404).with_header(
                        tiny_http::Header::from_bytes("Content-Type", "text/plain").expect("header"),
                    ));
                    continue;
                }
                let method = request.method().as_str().to_uppercase();
                let response = match method.as_str() {
                    "POST" => hold_http(&manager, request.url()),
                    "DELETE" => release_for(&manager)
                        .map(|reply| (200, serde_json::to_vec(&reply).unwrap_or_default())),
                    _ => Err("Use POST or DELETE on the update hold.".into()),
                };
                let (status, body) = response.unwrap_or_else(|message| (400, message.into_bytes()));
                let mut response = tiny_http::Response::from_data(body).with_status_code(status);
                response.add_header(
                    tiny_http::Header::from_bytes("Content-Type", "application/json").expect("header"),
                );
                let _ = request.respond(response);
            }
        })
        .map_err(|error| format!("could not start the update hold listener: {error}"))
        .is_err()
    {
        crate::browser_manager::log_line(
            &app.state::<BrowserManager>(),
            "update hold unavailable: could not start the listener thread",
        );
    }
}

fn hold_http(manager: &BrowserManager, url: &str) -> Result<(u16, Vec<u8>), String> {
    let query = url::Url::parse(&format!("http://127.0.0.1{url}")).map_err(|_| "Invalid hold request.")?;
    let timeout_s = query.query_pairs().find(|(key, _)| key == "timeout_s").and_then(|(_, value)| value.parse::<f64>().ok());
    let pause_s = query.query_pairs().find(|(key, _)| key == "pause_s").and_then(|(_, value)| value.parse::<f64>().ok());
    let answer = take_with_waiting(
        timeout_s,
        pause_s,
        || crate::browser_manager::waiting_count(manager),
        || crate::browser_manager::hold_work(manager),
    )?;
    crate::browser_manager::log_line(manager, &hold_taken_line(&answer));
    crate::browser_manager::log_line(manager, &settled_line(&answer));
    Ok((200, serde_json::to_vec(&answer).map_err(|error| error.to_string())?))
}

/// Keep the original IPC handlers for local verification, sharing the same pause and logging.

#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loopback_listener_uses_the_contract_route_and_port() {
        assert_eq!(PORT, 8648);
        assert_eq!("/api/maintenance/hold", "/api/maintenance/hold");
    }

    #[test]
    fn a_browser_page_cannot_pause_the_hold_but_the_updater_can() {
        assert!(origin_allowed(None), "the updater sends no Origin");
        assert!(origin_allowed(Some("tauri://localhost")), "Relay's own webview page");
        assert!(origin_allowed(Some("http://127.0.0.1:1420")), "a loopback page");
        assert!(origin_allowed(Some("http://localhost:3000")));
        assert!(!origin_allowed(Some("https://evil.example")), "any other web page is refused");
        assert!(!origin_allowed(Some("http://192.168.1.20:8787")));
        assert!(!origin_allowed(Some("null")), "a sandboxed page's opaque origin is not the app");
    }

    #[test]
    fn the_reply_is_exactly_the_contract_shape() {
        let answer = Reply {
            safe: true,
            unsettled: vec![],
            dispatch_paused_s: 120.0,
            in_flight: 0,
            waiting: 0,
        };
        let value = serde_json::to_value(&answer).expect("reply encodes");
        let mut keys: Vec<String> = value.as_object().expect("reply is an object").keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["dispatch_paused_s", "in_flight", "safe", "unsettled", "waiting"]);
    }

    #[test]
    fn waiting_accounts_keep_the_hold_from_reading_idle() {
        let _guard = test_lock();
        let answer = take_with_waiting(Some(0.0), Some(120.0), || Ok(3), || Ok(vec![]))
            .expect("hold taken");
        assert!(answer.safe, "queued accounts resume, so a restart is still safe");
        assert_eq!(answer.in_flight, 0, "nothing is starting or closing");
        assert_eq!(answer.waiting, 3, "work waiting is not an idle hold");
        let taken = hold_taken_line(&answer);
        let settled = settled_line(&answer);
        assert!(taken.contains("in_flight=0") && taken.contains("waiting=3"), "{taken}");
        assert!(settled.contains("in_flight=0") && settled.contains("waiting=3"), "{settled}");
        release_with_waiting(|| Ok(3), || Ok(vec![])).expect("release");
        assert!(!paused());
    }

    #[test]
    fn timeouts_and_pauses_are_bounded_the_way_the_contract_says() {
        assert_eq!(bounded(None, DEFAULT_TIMEOUT_S, 0.0, MAX_TIMEOUT_S), 20.0, "default 20");
        assert_eq!(bounded(Some(999.0), DEFAULT_TIMEOUT_S, 0.0, MAX_TIMEOUT_S), 30.0, "ceiling 30");
        assert_eq!(bounded(Some(-4.0), DEFAULT_TIMEOUT_S, 0.0, MAX_TIMEOUT_S), 0.0, "a negative wait is no wait");
        assert_eq!(bounded(Some(f64::NAN), DEFAULT_TIMEOUT_S, 0.0, MAX_TIMEOUT_S), 20.0, "garbage is the default");
        assert_eq!(bounded(None, DEFAULT_PAUSE_S, MIN_PAUSE_S, MAX_PAUSE_S), 120.0, "default pause");
        assert_eq!(bounded(Some(0.0), DEFAULT_PAUSE_S, MIN_PAUSE_S, MAX_PAUSE_S), 1.0, "the pause always has a moment");
        assert_eq!(bounded(Some(1e9), DEFAULT_PAUSE_S, MIN_PAUSE_S, MAX_PAUSE_S), 900.0, "the pause is bounded too");
    }

    #[test]
    fn the_pause_is_counted_down_from_the_clock_and_never_stored() {
        let mut hold = Hold::default();
        let start = Instant::now();
        hold.pause_for(120.0);
        assert!(hold.paused_at(start), "the hold stands");
        assert!(hold.paused_at(start + Duration::from_secs(119)));
        assert!(!hold.paused_at(start + Duration::from_secs(121)), "it expires from the clock alone");
        hold.resume();
        assert!(!hold.is_paused(), "DELETE resumes at once");
    }

    #[test]
    fn a_restart_builds_an_open_hold_with_nothing_to_clear() {
        let _guard = test_lock();
        take(Some(0.0), Some(120.0), || Ok(vec![InFlight {
            id: "profile-a".into(),
            resumable: true,
        }]))
        .expect("hold taken");
        assert!(paused(), "the pause stands until it expires");
        // A restart is a fresh process: it builds this again, and nothing was written down.
        assert!(!Hold::default().is_paused(), "a restart dispatches with no manual step");
        release(|| Ok(vec![])).expect("release");
        assert!(!paused());
    }

    #[test]
    fn an_abandoned_hold_resumes_by_itself_after_the_pause() {
        let _guard = test_lock();
        let answer = take(Some(0.0), Some(1.0), || Ok(vec![])).expect("hold taken");
        assert_eq!(answer.dispatch_paused_s, 1.0);
        assert!(paused(), "no DELETE: the hold stands");
        std::thread::sleep(Duration::from_millis(1100));
        assert!(
            !paused(),
            "the pause expires by itself, so an aborted install never leaves Relay refusing work"
        );
    }
}
