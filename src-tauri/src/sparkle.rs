//! The Sparkle shim (`vendor/khsparkle/khsparkle.h`). The shim owns Sparkle, the update hold and the
//! no-UI driver; Relay starts it once, after its window exists. Its config is the bundle's
//! Info.plist (SUFeedURL, SUPublicEDKey, KHHoldURL), merged by Tauri from `src-tauri/Info.plist`.

use std::ffi::CStr;
use std::os::raw::c_char;

type StateFn = extern "C" fn(i32);

extern "C" {
    fn kh_sparkle_start() -> i32;
    fn kh_sparkle_last_error() -> *const c_char;
    fn kh_sparkle_set_state_callback(cb: Option<StateFn>);
}

/// 1 updating, 2 relaunching (set before the shim quits the app). The hold has already paused
/// dispatch by then, so there is nothing for Relay to stop.
extern "C" fn on_state(_state: i32) {}

/// 0 = started. Non-zero is a refusal (missing plist keys, another install in progress) and the
/// reason is worth a log line; it never blocks Relay's own work.
pub(crate) fn start() -> Result<(), String> {
    unsafe {
        kh_sparkle_set_state_callback(Some(on_state));
        if kh_sparkle_start() == 0 {
            return Ok(());
        }
        Err(CStr::from_ptr(kh_sparkle_last_error()).to_string_lossy().into_owned())
    }
}
