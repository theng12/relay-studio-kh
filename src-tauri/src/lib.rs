mod browser_manager;
mod devtools;
mod extensions;
mod maintenance;
mod seeding;
mod storage;
mod login_item;
mod manager_gateway;
mod pairing;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command, sync::Mutex, time::Duration};
use tauri::{
    menu::{MenuBuilder, MenuItem, SubmenuBuilder},
    webview::PageLoadEvent,
    Emitter, Manager, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use url::Url;

fn default_appearance() -> String {
    "system".into()
}
fn valid_appearance(value: &str) -> bool {
    matches!(value, "system" | "light" | "dark")
}
/// A stored `default` came from a build that offered a fourth option; it now means `system`.
fn normalized_appearance(value: &str) -> String {
    match value {
        "default" => "system".to_string(),
        other if valid_appearance(other) => other.to_string(),
        _ => default_appearance(),
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    gateway_url: Option<String>,
    start_at_login: bool,
    /// In-app only: system, light, dark or the app's designed look. Never changes macOS.
    #[serde(default = "default_appearance")]
    appearance: String,
    /// Accounts page layout: compact (default) or card. In-app only.
    #[serde(default = "default_accounts_view")]
    accounts_view: String,
    /// Off by default: a Gateway on another Mac must use HTTPS, because plain HTTP carries the
    /// worker token unencrypted. Loopback is always allowed; this turns plain HTTP on for a fleet.
    #[serde(default)]
    allow_plain_http: bool,
    /// What Relay quotes Gateway for one image on the image channel. Free is 0 — a real price,
    /// never "unpriced" — and the owner may change it here at any time.
    #[serde(default)]
    image_credits_per_image: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            gateway_url: None,
            start_at_login: true,
            appearance: default_appearance(),
            accounts_view: default_accounts_view(),
            allow_plain_http: false,
            image_credits_per_image: 0.0,
        }
    }
}
fn normalized_credits(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        0.0
    }
}
fn default_accounts_view() -> String {
    "card".into()
}
fn valid_accounts_view(value: &str) -> bool {
    matches!(value, "compact" | "card" | "table")
}
fn normalized_accounts_view(value: &str) -> String {
    // `grid` was this view's stored name before it was renamed to Card.
    if value == "grid" {
        return "card".to_string();
    }
    if valid_accounts_view(value) {
        value.to_string()
    } else {
        default_accounts_view()
    }
}
struct NativeState {
    settings: Mutex<Result<Settings, String>>,
    path: PathBuf,
    client: reqwest::Client,
    login_error: Mutex<Option<String>>,
}
#[derive(Serialize)]
struct Snapshot {
    settings: Settings,
    autostart_observed: Option<bool>,
    autostart_error: Option<String>,
}
#[derive(Serialize)]
struct Health {
    configured: bool,
    reachable: bool,
    origin: Option<String>,
}

fn local_only(window: &WebviewWindow) -> Result<(), String> {
    // The Gateway webview is a browser, never a native settings authority.
    if window.label() != "main" {
        return Err("Native settings are available only in Relay's local window.".into());
    }
    Ok(())
}
/// Gateway's default port; the address field supplies it when the person leaves it out.
const DEFAULT_GATEWAY_PORT: u16 = 8787;

/// Loopback is the same Mac, so plain HTTP never leaves it. Anything else needs HTTPS unless the
/// person has explicitly allowed plain HTTP for a fleet.
fn loopback(host: Option<url::Host<&str>>) -> bool {
    match host {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// A saved address that is plain HTTP off this Mac — the fleet setup the switch exists for.
fn plain_remote(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| url.scheme() == "http" && !loopback(url.host()))
}

/// Takes what the person types: a bare host or IP gets `http://` and Gateway's default port;
/// a trailing slash is trimmed; every refusal names the address actually tried.
fn origin(value: &str, allow_plain_http: bool) -> Result<Option<String>, String> {
    let text = value.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let candidate = if text.contains("://") {
        text.to_string()
    } else {
        format!("http://{text}")
    };
    let parsed = Url::parse(&candidate)
        .map_err(|_| format!("{text} is not a Gateway address. Use a host name or IP address."))?;
    if parsed.scheme() == "http" && !allow_plain_http && !loopback(parsed.host()) {
        return Err(format!(
            "{text} uses plain HTTP, which sends your worker token so anyone on the network can read it. Use an https:// address for a Gateway on another Mac, or turn on “Allow plain HTTP” below for a network you trust."
        ));
    }
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(format!(
            "{text} is not a Gateway origin. Leave out the path, password, query and fragment."
        ));
    }
    let host = match parsed.host() {
        Some(url::Host::Ipv6(address)) => format!("[{address}]"),
        Some(url::Host::Ipv4(address)) => address.to_string(),
        Some(url::Host::Domain(domain)) => domain.to_string(),
        None => return Err(format!("{text} has no Gateway host.")),
    };
    let port = parsed.port().unwrap_or(DEFAULT_GATEWAY_PORT);
    Ok(Some(format!("{}://{host}:{port}", parsed.scheme())))
}
fn current(app: &tauri::AppHandle) -> Result<Settings, String> {
    app.state::<NativeState>()
        .settings
        .lock()
        .map_err(|_| "Settings are busy.".to_string())?
        .as_ref()
        .cloned()
        .map_err(Clone::clone)
}
fn persist(path: &PathBuf, settings: &Settings) -> Result<(), String> {
    let parent = path.parent().ok_or("Settings folder is unavailable.")?;
    fs::create_dir_all(parent).map_err(|_| "Could not create Relay's settings folder.")?;
    let bytes =
        serde_json::to_vec_pretty(settings).map_err(|_| "Could not encode Relay settings.")?;
    let temporary = path.with_extension("pending");
    fs::write(&temporary, bytes)
        .map_err(|_| "Could not save Relay settings; existing choices are retained.")?;
    fs::rename(&temporary, path)
        .map_err(|_| "Could not replace Relay settings; existing choices are retained.".to_string())
}
fn read_settings(path: &PathBuf) -> Result<(Settings, bool), String> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut settings: Settings = serde_json::from_slice(&bytes)
                .map_err(|_| "Saved settings are unreadable. The file has not been reset.")?;
            // An address saved by an older build already works: carry that choice forward, visibly.
            settings.allow_plain_http |= plain_remote(settings.gateway_url.as_deref().unwrap_or(""));
            settings.gateway_url =
                origin(settings.gateway_url.as_deref().unwrap_or(""), settings.allow_plain_http)?;
            settings.appearance = normalized_appearance(&settings.appearance);
            settings.accounts_view = normalized_accounts_view(&settings.accounts_view);
            settings.image_credits_per_image = normalized_credits(settings.image_credits_per_image);
            Ok((settings, false))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok((Settings::default(), true))
        }
        Err(_) => {
            Err("Saved settings could not be read. Existing choices have not been reset.".into())
        }
    }
}
fn snapshot(app: &tauri::AppHandle) -> Result<Snapshot, String> {
    let settings = current(app)?;
    // A real login item first; the autostart plugin only answers when SMAppService is absent.
    let observed = match crate::login_item::registered() {
        Some(value) => Ok(value),
        None => app.autolaunch().is_enabled(),
    };
    let state = app.state::<NativeState>();
    let saved_error = state
        .login_error
        .lock()
        .map_err(|_| "Settings are busy.")?
        .clone();
    let (autostart_observed, autostart_error) = match observed {
        Ok(value) if value != settings.start_at_login => (Some(value), Some(format!(
            "Your saved choice is {}, but macOS reports Start at Login is {}. Check it in System Settings.",
            if settings.start_at_login { "on" } else { "off" }, if value { "on" } else { "off" }
        ))),
        Ok(value) => (Some(value), saved_error),
        Err(_) => (None, Some("Start at Login could not be checked. Check it in System Settings.".into())),
    };
    Ok(Snapshot {
        settings,
        autostart_observed,
        autostart_error,
    })
}
#[tauri::command]
fn get_settings(window: WebviewWindow, app: tauri::AppHandle) -> Result<Snapshot, String> {
    local_only(&window)?;
    snapshot(&app)
}
#[tauri::command]
fn save_settings(
    window: WebviewWindow,
    app: tauri::AppHandle,
    gateway_url: String,
    start_at_login: Option<bool>,
    appearance: Option<String>,
    accounts_view: Option<String>,
    allow_plain_http: Option<bool>,
    image_credits_per_image: Option<f64>,
) -> Result<(), String> {
    local_only(&window)?;
    let state = app.state::<NativeState>();
    let mut guard = state.settings.lock().map_err(|_| "Settings are busy.")?;
    let previous = guard.as_ref().map_err(Clone::clone)?;
    if let Some(value) = accounts_view.as_deref() {
        if !valid_accounts_view(value) {
            return Err("Choose Compact, Card or Table for the account list.".into());
        }
    }
    if let Some(value) = appearance.as_deref() {
        if !valid_appearance(value) {
            return Err("Choose System, Light or Dark for appearance.".into());
        }
    }
    let allow = allow_plain_http.unwrap_or(previous.allow_plain_http);
    let settings = Settings {
        gateway_url: origin(&gateway_url, allow)?,
        start_at_login: start_at_login.unwrap_or(previous.start_at_login),
        appearance: appearance.unwrap_or_else(|| previous.appearance.clone()),
        accounts_view: accounts_view
            .map(|value| value.to_string())
            .unwrap_or_else(|| previous.accounts_view.clone()),
        allow_plain_http: allow,
        image_credits_per_image: image_credits_per_image
            .map(normalized_credits)
            .unwrap_or(previous.image_credits_per_image),
    };
    persist(&state.path, &settings)?;
    *guard = Ok(settings.clone());
    drop(guard);
    apply_appearance(&window, &settings.appearance);
    // Save intent first; a URL-only edit never reverses an OS opt-out.
    if let Some(enabled) = start_at_login {
        let result = match crate::login_item::set_enabled(enabled) {
            Ok(_) => {
                // Retire the old raw agent only after the new login item is registered.
                if enabled {
                    if let Ok(home) = app.path().home_dir() {
                        let present = crate::login_item::legacy_agent(&home).exists();
                        if crate::login_item::should_retire_agent(present, true) {
                            let _ = crate::login_item::retire_legacy_agent(&home);
                        }
                    }
                }
                Ok(())
            }
            Err(reason) if reason == "unsupported" => {
                let plugin = if enabled { app.autolaunch().enable() } else { app.autolaunch().disable() };
                plugin.map_err(|_| ())
            }
            Err(_) => Err(()),
        };
        let error = result.err().map(|_| "Settings were saved, but macOS refused the Start at Login change. Check it in System Settings.".to_string());
        *state.login_error.lock().map_err(|_| "Settings are busy.")? = error.clone();
        if let Some(error) = error {
            return Err(error);
        }
    }
    Ok(())
}
// Native window ground behind the webview, following the active scheme.
fn window_ground(theme: Theme) -> tauri::window::Color {
    match theme {
        Theme::Dark => tauri::window::Color(20, 18, 28, 255),
        _ => tauri::window::Color(245, 243, 250, 255),
    }
}
fn appearance_theme(value: &str) -> Option<Theme> {
    match value {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}
/// The appearance is pinned by the user's in-app choice; `system` releases it to macOS.
fn apply_appearance(window: &WebviewWindow, value: &str) {
    let _ = window.set_theme(appearance_theme(value));
    let ground = match appearance_theme(value) {
        Some(theme) => window_ground(theme),
        None => window
            .theme()
            .map(window_ground)
            .unwrap_or(window_ground(Theme::Light)),
    };
    let _ = window.set_background_color(Some(ground));
}
#[derive(Serialize, Deserialize)]
struct SavedFrame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
fn frame_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("window.json"))
}
fn read_frame(app: &tauri::AppHandle) -> Option<SavedFrame> {
    let frame: SavedFrame = serde_json::from_slice(&fs::read(frame_path(app)?).ok()?).ok()?;
    let usable = frame.x.is_finite()
        && frame.y.is_finite()
        && frame.width.is_finite()
        && frame.height.is_finite()
        && frame.width >= 860.0
        && frame.height >= 560.0
        && frame.width <= 10000.0
        && frame.height <= 10000.0;
    usable.then_some(frame)
}
fn write_frame(
    app: &tauri::AppHandle,
    scale: Result<f64, tauri::Error>,
    position: Result<tauri::PhysicalPosition<i32>, tauri::Error>,
    size: Result<tauri::PhysicalSize<u32>, tauri::Error>,
) {
    let (Ok(scale), Ok(position), Ok(size)) = (scale, position, size) else {
        return;
    };
    if scale <= 0.0 {
        return;
    }
    let Some(path) = frame_path(app) else {
        return;
    };
    let frame = SavedFrame {
        x: f64::from(position.x) / scale,
        y: f64::from(position.y) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&frame) {
        let _ = fs::write(path, bytes);
    }
}
fn save_frame(window: &tauri::Window) {
    write_frame(
        window.app_handle(),
        window.scale_factor(),
        window.outer_position(),
        window.inner_size(),
    );
}
fn save_webview_frame(window: &WebviewWindow) {
    write_frame(
        window.app_handle(),
        window.scale_factor(),
        window.outer_position(),
        window.inner_size(),
    );
}
#[tauri::command]
fn data_folder(window: WebviewWindow, app: tauri::AppHandle) -> Result<String, String> {
    local_only(&window)?;
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|_| "Relay's data folder is unavailable.")?
        .to_string_lossy()
        .into_owned())
}
// The only external links Relay opens by itself. Anything else is refused.
const SUPPORT_URL: &str = "https://nowpayments.io/donation/studiokh";
const LAUNCHER_APP: &str = "/Applications/Studio Launcher KH.app";

#[tauri::command]
fn open_support_link(window: WebviewWindow) -> Result<(), String> {
    local_only(&window)?;
    let status = Command::new("/usr/bin/open")
        .arg(SUPPORT_URL)
        .status()
        .map_err(|_| "Could not open the support page.".to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "The support page did not open.".to_string())
}

/// All updates go through Studio Launcher KH; Relay never installs its own update.
#[tauri::command]
fn open_launcher(window: WebviewWindow) -> Result<(), String> {
    local_only(&window)?;
    if !std::path::Path::new(LAUNCHER_APP).is_dir() {
        return Err("Studio Launcher KH is not installed on this Mac.".into());
    }
    let status = Command::new("/usr/bin/open")
        .args(["-a", LAUNCHER_APP])
        .status()
        .map_err(|_| "Could not open Studio Launcher KH.".to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "Studio Launcher KH did not open.".to_string())
}

#[tauri::command]
fn open_data_folder(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local_only(&window)?;
    let path = app
        .path()
        .app_config_dir()
        .map_err(|_| "Relay's data folder is unavailable.")?;
    fs::create_dir_all(&path).map_err(|_| "Could not create Relay's data folder.")?;
    let status = Command::new("/usr/bin/open")
        .arg(path)
        .status()
        .map_err(|_| "Could not open Relay's data folder.")?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "Could not open Relay's data folder.".to_string())
}

fn show_settings(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
/// A second launch must not take the desktop. Record it and stay behind; the window is only
/// brought forward when the person asks for it.
fn report_second_launch(app: &tauri::AppHandle) {
    let line = format!(
        "{} another Relay launch was ignored; the running instance kept the screen\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    if let Ok(dir) = app.path().app_config_dir() {
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("second-launch.log");
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = std::io::Write::write_all(&mut file, line.as_bytes());
        }
    }
}
/// True when the plist names a different bundle than the one now running.
fn login_item_is_stale(registered: &str, exe_path: &str) -> bool {
    // Compare the plist's own entries exactly: a substring test would accept another copy whose
    // path merely ends with the same bundle name.
    let entries = registered
        .split("<string>")
        .skip(1)
        .filter_map(|part| part.split("</string>").next());
    !entries.into_iter().any(|entry| entry.trim() == exe_path)
}
/// The LaunchAgent plist is written with the executable path of whichever copy registered it, so
/// moving or reinstalling the app leaves it pointing at a bundle that no longer exists — and that
/// fails silently at the next login. Rewrite it whenever the saved choice is on and the path is
/// stale. A missing plist is an OS-level opt-out and is left alone.
fn heal_login_item(app: &tauri::AppHandle, start_at_login: bool) {
    if !start_at_login {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Ok(home) = app.path().home_dir() else {
        return;
    };
    let plist = home.join("Library/LaunchAgents/Relay Studio KH.plist");
    let Ok(registered) = fs::read_to_string(&plist) else {
        return;
    };
    if login_item_is_stale(&registered, &exe.to_string_lossy())
        && app.autolaunch().enable().is_err()
    {
        if let Some(state) = app.try_state::<NativeState>() {
            if let Ok(mut error) = state.login_error.lock() {
                *error = Some(
                    "Start at Login pointed at an old copy of Relay and could not be repaired. Your saved choice is retained; check it in System Settings.".into(),
                );
            }
        }
    }
}
fn connect(app: &tauri::AppHandle) -> Result<(), String> {
    let address = current(app)?
        .gateway_url
        .ok_or("Save a Gateway address first.")?;
    let mut url = Url::parse(&address).map_err(|_| "Saved Gateway address is invalid.")?;
    url.set_path("/admin");
    url.set_fragment(Some("extension-workers"));
    if let Some(window) = app.get_webview_window("gateway") {
        // A new GET navigation, never a reload of an earlier form submission.
        window
            .navigate(url)
            .map_err(|_| "Could not reconnect to Gateway.")?;
        window.show().map_err(|_| "Could not show Gateway.")?;
        window.set_focus().map_err(|_| "Could not focus Gateway.")?;
        return Ok(());
    }
    let handle = app.clone();
    WebviewWindowBuilder::new(app, "gateway", WebviewUrl::External(url))
        .title("Relay Studio KH — Browser workers")
        .inner_size(1100.0, 780.0)
        .on_navigation(move |target| {
            current(&handle)
                .ok()
                .and_then(|s| s.gateway_url)
                .is_some_and(|allowed| target.origin().ascii_serialization() == allowed)
        })
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()
        .map_err(|_| "Could not open Gateway's interface.")?;
    Ok(())
}
#[tauri::command]
async fn open_gateway(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local_only(&window)?;
    connect(&app)
}
#[tauri::command]
async fn gateway_health(window: WebviewWindow, app: tauri::AppHandle) -> Result<Health, String> {
    local_only(&window)?;
    let address = current(&app)?.gateway_url;
    let Some(address) = address else {
        return Ok(Health {
            configured: false,
            reachable: false,
            origin: None,
        });
    };
    let url = Url::parse(&address)
        .map_err(|_| "Saved Gateway address is invalid.")?
        .join("/health")
        .map_err(|_| "Saved Gateway address is invalid.")?;
    let client = app.state::<NativeState>().client.clone();
    let reachable = client
        .get(url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    // Transport only: no body, credentials, readiness, credit or worker inference.
    Ok(Health {
        configured: true,
        reachable,
        origin: Some(address.clone()),
    })
}
#[tauri::command]
fn quit_app(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local_only(&window)?;
    app.exit(0);
    Ok(())
}

/// The local pairing file wins over a stale saved address; the manual field remains the fallback.
/// The pairing file is 0600 and written by the local Gateway, so a plain HTTP fleet address it
/// names is the person's own setup: accept it and show the plain-HTTP switch on.
fn apply_pairing_url(state: &NativeState, url: &str) {
    if let Ok(mut guard) = state.settings.lock() {
        if let Ok(saved) = guard.as_mut() {
            let allow = saved.allow_plain_http || plain_remote(url);
            let Ok(Some(url)) = origin(url, allow) else { return };
            if saved.gateway_url.as_deref() != Some(url.as_str()) || saved.allow_plain_http != allow {
                saved.gateway_url = Some(url);
                saved.allow_plain_http = allow;
                let _ = persist(&state.path, saved);
            }
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| { report_second_launch(app); }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, open_gateway, open_support_link, open_launcher, pairing::pairing_status, pairing::pairing_refresh, gateway_health, quit_app, data_folder, open_data_folder, extensions::get_extension_setup, extensions::open_extension_settings, extensions::prepare_extension, extensions::set_extension_enabled, seeding::extension_worker_status, seeding::save_extension_worker, seeding::clear_extension_worker, extensions::show_extension_folder, manager_gateway::manager_read_gateway, manager_gateway::manager_control_gateway, manager_gateway::manager_paired_gateway, browser_manager::manager_pair_gateway, browser_manager::manager_clear_gateway, browser_manager::manager_snapshot, browser_manager::manager_loaded_extensions, browser_manager::manager_add_profiles, browser_manager::manager_update_profile, browser_manager::manager_delete_profile, browser_manager::manager_open_profiles, browser_manager::manager_show_profiles, browser_manager::manager_close_profiles, browser_manager::manager_force_close_profiles, browser_manager::manager_save_settings, maintenance::maintenance_hold, maintenance::maintenance_release])
        .setup(|app| {
            app.manage(browser_manager::init(app.handle())?);
            maintenance::start_http_listener(app.handle());
            let path = app.path().app_config_dir()?.join("settings.json");
            let loaded = read_settings(&path);
            let settings = loaded.as_ref().map(|(s, _)| s.clone()).map_err(Clone::clone);
            app.manage(NativeState {
                settings: Mutex::new(settings), path: path.clone(), login_error: Mutex::new(None),
                client: reqwest::Client::builder().timeout(Duration::from_secs(4))
                    .redirect(reqwest::redirect::Policy::none()).build()?,
            });
            // The Gateway's local pairing file, when present, points Relay at that Gateway.
            if let Ok(Some(endpoint)) = pairing::read_endpoint() {
                apply_pairing_url(&app.state::<NativeState>(), &endpoint.url);
            }
            if let Ok((settings, _)) = &loaded {
                // A moved or reinstalled app leaves the login item pointing at the old bundle.
                heal_login_item(app.handle(), settings.start_at_login);
            }
            if let Ok((settings, true)) = loaded {
                // Only first launch supplies the default. Later OS opt-outs are left alone.
                let saved = persist(&path, &settings);
                if saved.is_ok() {
                    if app.autolaunch().enable().is_err() {
                        *app.state::<NativeState>().login_error.lock().map_err(|_| "Settings are busy")? = Some(
                            "Start at Login could not be enabled. Your saved choice is retained; check it in System Settings.".into()
                        );
                    }
                } else {
                    *app.state::<NativeState>().settings.lock().map_err(|_| "Settings are busy")? = saved.map(|_| settings);
                }
            }
            let frame = read_frame(app.handle());
            let refresh_handle = app.handle().clone();
            let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Relay Studio KH")
                .inner_size(frame.as_ref().map_or(1100.0, |f| f.width), frame.as_ref().map_or(760.0, |f| f.height))
                .min_inner_size(860.0, 560.0)
                // Matches the design ground, so resizing never flashes white gutters.
                .background_color(window_ground(Theme::Light))
                .on_navigation(|url| {
                    ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost")))
                        && url.port().is_none() && url.username().is_empty() && url.password().is_none()
                })
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                // A newer bundled extension reaches Relay's support folder on launch, without
                // touching existing version folders or any browser.
                .on_page_load(move |window, payload| {
                    if payload.event() == PageLoadEvent::Finished {
                        let handle = refresh_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let problems = extensions::refresh_bundled_copies(&handle);
                            if !problems.is_empty() {
                                let _ = window.emit("relay-error", format!(
                                    "Bundled extension copies were not refreshed: {}", problems.join(" · ")
                                ));
                            }
                        });
                    }
                });
            if let Some(frame) = frame {
                builder = builder.position(frame.x, frame.y);
            }
            let main = builder.build()?;
            // Mint a worker token per bound kind through the pairing token, off the UI thread.
            let pairing_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let notes = pairing::autopair(pairing_handle.clone()).await;
                if notes.iter().any(|note| note.contains("failed") || note.contains("could not")) {
                    let _ = pairing_handle.emit_to("main", "relay-error", format!("Automatic pairing: {}", notes.join(" · ")));
                }
            });
            let appearance = current(app.handle())
                .map(|settings| settings.appearance)
                .unwrap_or_else(|_| default_appearance());
            apply_appearance(&main, &appearance);
            // Always leave a frame on disk, even if the app is quit from the menu later.
            save_webview_frame(&main);
            let version = MenuItem::with_id(app, "version", format!("Relay Studio KH {}", app.package_info().version), false, None::<&str>)?;
            let application = SubmenuBuilder::new(app, "Relay Studio KH")
                .text("show", "Relay Settings…").text("reconnect", "Reconnect to Gateway")
                .separator().item(&version).separator().quit().build()?;
            let edit = SubmenuBuilder::new(app, "Edit");
            #[cfg(target_os = "macos")]
            let edit = edit.undo().redo().separator();
            let edit = edit.cut().copy().paste().select_all().build()?;
            app.set_menu(MenuBuilder::new(app).item(&application).item(&edit).build()?)?;
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_settings(app),
            "reconnect" => { if let Err(error) = connect(app) { show_settings(app); let _ = app.emit_to("main", "relay-error", error); } },
            _ => {}
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    save_frame(window);
                    api.prevent_close();
                    let _ = window.hide();
                }
                if let tauri::WindowEvent::ThemeChanged(theme) = event {
                    // A forced in-app appearance owns the title bar and ground.
                    let appearance = window
                        .app_handle()
                        .state::<NativeState>()
                        .settings
                        .lock()
                        .ok()
                        .and_then(|guard| guard.as_ref().ok().map(|settings| settings.appearance.clone()))
                        .unwrap_or_else(default_appearance);
                    if appearance == "system" {
                        let _ = window.set_background_color(Some(window_ground(*theme)));
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("Relay could not start; existing settings and Gateway work were not reset.")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
                if let Some(window) = app.get_webview_window("main") {
                    save_webview_frame(&window);
                }
                // Nothing Relay starts may outlive it: idle browsers are stopped, busy ones are
                // left running and named in lifecycle.log.
                if let Some(manager) = app.try_state::<browser_manager::BrowserManager>() {
                    browser_manager::shutdown(&manager);
                }
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows: false, .. } = event { show_settings(app); }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_default_reads_as_system_and_only_three_values_are_accepted() {
        // The fourth option is gone; an older build's saved choice must not fall through.
        assert_eq!(normalized_appearance("default"), "system");
        assert_eq!(normalized_appearance("system"), "system");
        assert_eq!(normalized_appearance("light"), "light");
        assert_eq!(normalized_appearance("dark"), "dark");
        assert_eq!(normalized_appearance("sepia"), "system");
        assert!(!valid_appearance("default"));
        assert!(appearance_theme("default").is_none());
        assert_eq!(appearance_theme("system"), None);
    }

    #[test]
    fn a_login_item_naming_another_bundle_is_stale_and_one_naming_this_bundle_is_not() {
        let mine = "/Applications/Relay Studio KH.app/Contents/MacOS/Relay Studio KH";
        let registered_here = format!("<string>{mine}</string>");
        assert!(!login_item_is_stale(&registered_here, mine));
        let registered_elsewhere = "<string>/Users/x/Applications/Relay Studio KH.app/Contents/MacOS/Relay Studio KH</string>";
        assert!(login_item_is_stale(registered_elsewhere, mine));
        assert!(login_item_is_stale("", mine));
    }

    #[test]
    fn the_address_field_takes_a_bare_host_and_fills_gateway_defaults() {
        assert_eq!(origin("127.0.0.1", false).unwrap().unwrap(), "http://127.0.0.1:8787");
        assert_eq!(origin("http://127.0.0.1/", false).unwrap().unwrap(), "http://127.0.0.1:8787");
        assert_eq!(origin("localhost:9000", false).unwrap().unwrap(), "http://localhost:9000");
        assert_eq!(origin("https://gateway.example", false).unwrap().unwrap(), "https://gateway.example:8787");
        assert!(origin("https://gateway.example/path", false).unwrap_err().contains("gateway.example"));
        assert!(origin("not a host", false).unwrap_err().contains("not a host"));
    }

    #[test]
    fn plain_http_is_loopback_only_unless_the_switch_is_on() {
        assert!(origin("gateway.example", false).unwrap_err().contains("https://"), "remote plain HTTP names the fix");
        assert!(origin("http://192.168.1.20:8787", false).is_err());
        assert_eq!(origin("gateway.example", true).unwrap().unwrap(), "http://gateway.example:8787");
        assert_eq!(origin("192.168.1.20:9000", true).unwrap().unwrap(), "http://192.168.1.20:9000");
        assert!(!plain_remote("http://127.0.0.1:8787"), "loopback is never the fleet case");
        assert!(plain_remote("http://192.168.1.20:8787"));
        assert!(!plain_remote("https://192.168.1.20:8787"));
    }

    #[test]
    fn compact_card_and_table_are_accepted_and_card_is_the_default() {
        assert_eq!(default_accounts_view(), "card");
        assert_eq!(normalized_accounts_view("card"), "card");
        // The view was stored as `grid` before it was renamed to Card.
        assert_eq!(normalized_accounts_view("grid"), "card");
        assert_eq!(normalized_accounts_view("compact"), "compact");
        assert_eq!(normalized_accounts_view("table"), "table");
        assert_eq!(normalized_accounts_view("cards"), "card");
        assert!(!valid_accounts_view("grid"));
    }
}
