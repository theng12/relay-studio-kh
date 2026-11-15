//! Update hold — the app side of `_orchestra/youtube-stack/update-hold/CONTRACT.md`.
//!
//! A hold closes Relay's start path first (no new Chrome begins, so no new claiming worker),
//! then waits a bounded moment for the work already inside a launch or a close to settle, and
//! answers whether a restart now would strand anything. The pause is one instant in memory and
//! nothing else: DELETE clears it at once, it expires on its own after `pause_s`, and a restart
//! always starts open — an install that aborts can never leave Relay refusing work.

use serde::Serialize;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{State, WebviewWindow};

use crate::browser_manager::BrowserManager;

// Contract bounds: settle within 20 s, never wait past 30; the pause defaults to two minutes.
const DEFAULT_TIMEOUT_S: f64 = 20.0;
const MAX_TIMEOUT_S: f64 = 30.0;
const DEFAULT_PAUSE_S: f64 = 120.0;
const MIN_PAUSE_S: f64 = 1.0;
const MAX_PAUSE_S: f64 = 900.0;
const POLL: Duration = Duration::from_millis(250);

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

fn reply(current: Vec<InFlight>, pause_s: f64) -> Reply {
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
    }
}

/// The hold request: close the claim path first, then wait for the work already inside.
pub(crate) fn take(
    timeout_s: Option<f64>,
    pause_s: Option<f64>,
    work: impl FnMut() -> Result<Vec<InFlight>, String>,
) -> Result<Reply, String> {
    let pause = bounded(pause_s, DEFAULT_PAUSE_S, MIN_PAUSE_S, MAX_PAUSE_S);
    let timeout = bounded(timeout_s, DEFAULT_TIMEOUT_S, 0.0, MAX_TIMEOUT_S);
    {
        // The guard ends with this block. A hold must never wait while holding the pause lock:
        // reconcile reads it under the state lock, so waiting here would deadlock the two.
        lock().pause_for(pause);
    }
    // A failure to read the work leaves the pause standing; it still expires by itself.
    Ok(reply(wait_for_settle(timeout, work)?, pause))
}

/// The release request: resume at once.
pub(crate) fn release(
    mut work: impl FnMut() -> Result<Vec<InFlight>, String>,
) -> Result<Reply, String> {
    lock().resume();
    Ok(reply(work()?, 0.0))
}

#[tauri::command]
pub(crate) fn maintenance_hold(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
    timeout_s: Option<f64>,
    pause_s: Option<f64>,
) -> Result<Reply, String> {
    super::local_only(&window)?;
    let answer = take(timeout_s, pause_s, || crate::browser_manager::hold_work(&manager))?;
    crate::browser_manager::log_line(
        &manager,
        &format!(
            "update hold taken: in_flight={}, dispatch paused {}s",
            answer.in_flight, answer.dispatch_paused_s
        ),
    );
    let outcome = if answer.unsettled.is_empty() && answer.in_flight == 0 {
        "update hold settled: in_flight=0".to_string()
    } else if answer.unsettled.is_empty() {
        format!(
            "update hold settled: in_flight={}, every id follows",
            answer.in_flight
        )
    } else {
        format!("update hold unsettled: {}", answer.unsettled.join(", "))
    };
    crate::browser_manager::log_line(&manager, &outcome);
    Ok(answer)
}

#[tauri::command]
pub(crate) fn maintenance_release(
    window: WebviewWindow,
    manager: State<'_, BrowserManager>,
) -> Result<Reply, String> {
    super::local_only(&window)?;
    let answer = release(|| crate::browser_manager::hold_work(&manager))?;
    crate::browser_manager::log_line(&manager, "update hold resumed");
    Ok(answer)
}

/// The four contract checks mutate the process-wide pause, so they take turns.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reply_is_exactly_the_contract_shape() {
        let answer = Reply {
            safe: true,
            unsettled: vec![],
            dispatch_paused_s: 120.0,
            in_flight: 0,
        };
        let value = serde_json::to_value(&answer).expect("reply encodes");
        let mut keys: Vec<String> = value.as_object().expect("reply is an object").keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["dispatch_paused_s", "in_flight", "safe", "unsettled"]);
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
