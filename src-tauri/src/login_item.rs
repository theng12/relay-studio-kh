//! Start at Login as a real login item (SMAppService), not a raw LaunchAgent that macOS can
//! list as a nameless entry. The old agent is kept until the new registration succeeds, then
//! retired recoverably. When SMAppService is unavailable (older macOS or a dev build), the
//! caller falls back to the previous autostart plugin.
use std::path::{Path, PathBuf};

pub(crate) const LEGACY_AGENT_NAME: &str = "Relay Studio KH.plist";

/// The agent the previous builds wrote, if it exists.
pub(crate) fn legacy_agent(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents").join(LEGACY_AGENT_NAME)
}

/// Retire the old agent only after the new registration actually succeeded.
pub(crate) fn should_retire_agent(legacy_present: bool, registered_now: bool) -> bool {
    legacy_present && registered_now
}

fn status_is_registered(status: isize) -> Option<bool> {
    match status {
        0 => Some(false),          // notRegistered
        1 => Some(true),           // enabled
        2 => Some(false),          // requiresApproval: registered but not yet approved
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn service() -> Option<*mut objc2::runtime::AnyObject> {
    use objc2::runtime::AnyClass;
    unsafe {
        // The framework is loaded on demand so no extra typed crate is needed.
        let path = c"/System/Library/Frameworks/ServiceManagement.framework/ServiceManagement";
        libc::dlopen(path.as_ptr(), libc::RTLD_LAZY);
        let class = AnyClass::get(c"SMAppService")?;
        let service: *mut objc2::runtime::AnyObject = objc2::msg_send![class, mainAppService];
        (!service.is_null()).then_some(service)
    }
}

#[cfg(target_os = "macos")]
fn call(service: *mut objc2::runtime::AnyObject, register: bool) -> Result<(), String> {
    use objc2::runtime::AnyObject;
    unsafe {
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = if register {
            objc2::msg_send![service, registerAndReturnError: &mut error]
        } else {
            objc2::msg_send![service, unregisterAndReturnError: &mut error]
        };
        if ok {
            Ok(())
        } else {
            Err("macOS refused the Login Item change; check Login Items in System Settings.".into())
        }
    }
}

/// `None` means this macOS or build cannot use SMAppService; the caller keeps the old path.
pub(crate) fn registered() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        let service = service()?;
        let status: isize = unsafe { objc2::msg_send![service, status] };
        status_is_registered(status)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Registers or unregisters Relay as a login item. On success the legacy agent is retired;
/// on any failure the agent is left in place so the owner's autostart is never lost.
pub(crate) fn set_enabled(enabled: bool) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let Some(service) = service() else { return Err("unsupported".into()) };
        call(service, enabled)?;
        Ok(enabled)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = enabled;
        Err("unsupported".into())
    }
}

/// Unloads and moves the old agent out of the way, never deleting it.
pub(crate) fn retire_legacy_agent(home: &Path) -> Result<(), String> {
    let agent = legacy_agent(home);
    if !agent.exists() {
        return Ok(());
    }
    let _ = std::process::Command::new("/bin/launchctl").arg("unload").arg(&agent).status();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let trash = home.join(".kh").join("_trash");
    std::fs::create_dir_all(&trash).map_err(|_| "The old login agent could not be moved aside.".to_string())?;
    let target = trash.join(format!("{LEGACY_AGENT_NAME}.{stamp}"));
    std::fs::rename(&agent, &target).map_err(|_| "The old login agent could not be moved aside.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_agent_is_retired_only_after_the_new_registration_works() {
        assert!(!should_retire_agent(true, false));
        assert!(!should_retire_agent(false, true));
        assert!(should_retire_agent(true, true));
    }

    #[test]
    fn status_maps_to_the_truth_including_needing_approval() {
        assert_eq!(status_is_registered(0), Some(false));
        assert_eq!(status_is_registered(1), Some(true));
        assert_eq!(status_is_registered(2), Some(false), "approval pending is not enabled yet");
        assert_eq!(status_is_registered(9), None);
    }

    #[test]
    fn the_legacy_agent_path_is_the_one_previous_builds_wrote() {
        let home = Path::new("/Users/example");
        assert_eq!(
            legacy_agent(home),
            PathBuf::from("/Users/example/Library/LaunchAgents/Relay Studio KH.plist")
        );
    }
}
