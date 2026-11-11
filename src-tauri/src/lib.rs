mod browser_manager;
mod extensions;
mod manager_gateway;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex, time::Duration};
use tauri::{
    menu::{MenuBuilder, MenuItem, SubmenuBuilder},
    Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use url::Url;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    gateway_url: Option<String>,
    start_at_login: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            gateway_url: None,
            start_at_login: true,
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
        .map_err(|_| "Could not replace Relay settings; existing choices are retained.")
}
fn read_settings(path: &PathBuf) -> Result<(Settings, bool), String> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut settings: Settings = serde_json::from_slice(&bytes)
                .map_err(|_| "Saved settings are unreadable. The file has not been reset.")?;
            settings.gateway_url = origin(settings.gateway_url.as_deref().unwrap_or(""))?;
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
) -> Result<(), String> {
    local_only(&window)?;
    let state = app.state::<NativeState>();
    let mut guard = state.settings.lock().map_err(|_| "Settings are busy.")?;
    let previous = guard.as_ref().map_err(Clone::clone)?;
    let settings = Settings {
        gateway_url: origin(&gateway_url)?,
        start_at_login: start_at_login.unwrap_or(previous.start_at_login),
    };
    persist(&state.path, &settings)?;
    *guard = Ok(settings);
    drop(guard);
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
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, open_gateway, gateway_health, quit_app, extensions::get_extension_setup, extensions::open_extension_settings, extensions::prepare_extension, extensions::show_extension_folder, manager_gateway::manager_read_gateway, manager_gateway::manager_control_gateway, manager_gateway::manager_paired_gateway, browser_manager::manager_pair_gateway, browser_manager::manager_clear_gateway, browser_manager::manager_snapshot, browser_manager::manager_add_profiles, browser_manager::manager_update_profile, browser_manager::manager_open_profiles, browser_manager::manager_show_profiles, browser_manager::manager_close_profiles, browser_manager::manager_save_settings])
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
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Relay Studio KH").inner_size(780.0, 620.0)
                .on_navigation(|url| {
                    ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost")))
                        && url.port().is_none() && url.username().is_empty() && url.password().is_none()
                })
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .build()?;
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
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("Relay could not start; existing settings and Gateway work were not reset.")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows: false, .. } = event { show_settings(app); }
        });
}
