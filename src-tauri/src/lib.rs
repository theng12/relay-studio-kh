mod browser_manager;
mod extensions;
mod manager_gateway;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command, sync::Mutex, time::Duration};
use tauri::{
    menu::{MenuBuilder, MenuItem, SubmenuBuilder},
    Emitter, Manager, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use url::Url;

fn default_appearance() -> String {
    "system".into()
}
fn valid_appearance(value: &str) -> bool {
    matches!(value, "system" | "light" | "dark" | "default")
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    gateway_url: Option<String>,
    start_at_login: bool,
    /// In-app only: system, light, dark or the app's designed look. Never changes macOS.
    #[serde(default = "default_appearance")]
    appearance: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            gateway_url: None,
            start_at_login: true,
            appearance: default_appearance(),
        }
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
fn origin(value: &str) -> Result<Option<String>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let parsed =
        Url::parse(value.trim()).map_err(|_| "Use a complete HTTP or HTTPS Gateway address.")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(
            "Use the Gateway origin without a path, password, token, query or fragment.".into(),
        );
    }
    Ok(Some(parsed.origin().ascii_serialization()))
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
            settings.gateway_url = origin(settings.gateway_url.as_deref().unwrap_or(""))?;
            if !valid_appearance(&settings.appearance) {
                settings.appearance = default_appearance();
            }
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
    let observed = app.autolaunch().is_enabled();
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
) -> Result<(), String> {
    local_only(&window)?;
    let state = app.state::<NativeState>();
    let mut guard = state.settings.lock().map_err(|_| "Settings are busy.")?;
    let previous = guard.as_ref().map_err(Clone::clone)?;
    if let Some(value) = appearance.as_deref() {
        if !valid_appearance(value) {
            return Err("Choose System, Light, Dark or Default for appearance.".into());
        }
    }
    let settings = Settings {
        gateway_url: origin(&gateway_url)?,
        start_at_login: start_at_login.unwrap_or(previous.start_at_login),
        appearance: appearance.unwrap_or_else(|| previous.appearance.clone()),
    };
    persist(&state.path, &settings)?;
    *guard = Ok(settings.clone());
    drop(guard);
    apply_appearance(&window, &settings.appearance);
    // Save intent first; a URL-only edit never reverses an OS opt-out.
    if let Some(enabled) = start_at_login {
        let result = if enabled {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
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
        // Default is Relay's designed iris look: the light palette.
        "light" | "default" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}
/// The appearance is pinned by the user's in-app choice; `system` releases it to macOS.
fn apply_appearance(window: &WebviewWindow, value: &str) {
    let _ = window.set_theme(appearance_theme(value));
    let ground = match appearance_theme(value) {
        Some(theme) => window_ground(theme),
        None => window.theme().map(window_ground).unwrap_or(window_ground(Theme::Light)),
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
    app.path().app_config_dir().ok().map(|dir| dir.join("window.json"))
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

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| { show_settings(app); }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, open_gateway, gateway_health, quit_app, data_folder, open_data_folder, extensions::get_extension_setup, extensions::open_extension_settings, extensions::prepare_extension, extensions::show_extension_folder, manager_gateway::manager_read_gateway, manager_gateway::manager_control_gateway, manager_gateway::manager_paired_gateway, browser_manager::manager_pair_gateway, browser_manager::manager_clear_gateway, browser_manager::manager_snapshot, browser_manager::manager_add_profiles, browser_manager::manager_update_profile, browser_manager::manager_open_profiles, browser_manager::manager_show_profiles, browser_manager::manager_close_profiles, browser_manager::manager_force_close_profiles, browser_manager::manager_save_settings])
        .setup(|app| {
            app.manage(browser_manager::init(app.handle())?);
            let path = app.path().app_config_dir()?.join("settings.json");
            let loaded = read_settings(&path);
            let settings = loaded.as_ref().map(|(s, _)| s.clone()).map_err(Clone::clone);
            app.manage(NativeState {
                settings: Mutex::new(settings), path: path.clone(), login_error: Mutex::new(None),
                client: reqwest::Client::builder().timeout(Duration::from_secs(4))
                    .redirect(reqwest::redirect::Policy::none()).build()?,
            });
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
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny);
            if let Some(frame) = frame {
                builder = builder.position(frame.x, frame.y);
            }
            let main = builder.build()?;
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
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows: false, .. } = event { show_settings(app); }
        });
}
