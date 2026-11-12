use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

#[derive(Clone, Deserialize, Serialize)]
struct Browser {
    id: String,
    name: String,
    bundle_id: String,
    app_name: String,
    settings_url: String,
    instructions: String,
    #[serde(default)]
    detected: bool,
}
#[derive(Clone, Deserialize, Serialize)]
struct FileEntry {
    path: String,
    sha256: String,
}
fn required_by_default() -> bool {
    true
}

#[derive(Clone, Deserialize, Serialize)]
struct Extension {
    id: String,
    name: String,
    version: Option<String>,
    source_commit: Option<String>,
    available: bool,
    #[serde(default = "required_by_default")]
    required: bool,
    reason: Option<String>,
    files: Vec<FileEntry>,
    payload_sha256: Option<String>,
}
#[derive(Deserialize, Serialize)]
struct Catalog {
    schema: u8,
    extensions: Vec<Extension>,
}
#[derive(Serialize)]
pub(crate) struct Setup {
    browsers: Vec<Browser>,
    extensions: Vec<Extension>,
}
fn browsers() -> Result<Vec<Browser>, String> {
    let mut browsers: Vec<Browser> =
        serde_json::from_str(include_str!("../../desktop/browsers.json"))
            .map_err(|_| "Browser setup definitions are unavailable.")?;
    for browser in &mut browsers {
        let system = Path::new("/Applications").join(&browser.app_name).is_dir();
        let user = std::env::var_os("HOME").is_some_and(|home| {
            PathBuf::from(home)
                .join("Applications")
                .join(&browser.app_name)
                .is_dir()
        });
        // Spotlight supplements the standard folders; absence means not detected, not uninstalled.
        let indexed = !system
            && !user
            && Command::new("/usr/bin/mdfind")
                .arg(format!(
                    "kMDItemCFBundleIdentifier == '{}'",
                    browser.bundle_id
                ))
                .output()
                .ok()
                .filter(|out| out.status.success())
                .is_some_and(|out| {
                    String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .any(|path| Path::new(path).is_dir())
                });
        browser.detected = system || user || indexed;
    }
    Ok(browsers)
}
fn catalog(app: &tauri::AppHandle) -> Result<(Catalog, PathBuf), String> {
    let root = app
        .path()
        .resource_dir()
        .map_err(|_| "App resources are unavailable.")?
        .join("relay/extensions");
    let data = fs::read(root.join("index.json")).map_err(|_| {
        "Bundled extension index is unavailable. No installation has been performed."
    })?;
    let catalog: Catalog =
        serde_json::from_slice(&data).map_err(|_| "Bundled extension index is invalid.")?;
    if catalog.schema != 1
        || catalog.extensions.len() != 3
        || ["video", "image", "audio"]
            .iter()
            .any(|id| catalog.extensions.iter().filter(|e| e.id == *id).count() != 1)
    {
        return Err(
            "Bundled extension index does not contain the three expected extensions.".into(),
        );
    }
    Ok((catalog, root))
}
fn hex(value: &str, size: usize) -> bool {
    value.len() == size && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn relative(value: &str) -> Result<&Path, String> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Extension payload contains an unsafe path.".into());
    }
    Ok(path)
}
fn verified_file(root: &Path, file: &FileEntry) -> Result<Vec<u8>, String> {
    let relative = relative(&file.path)?;
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part.as_os_str());
        if fs::symlink_metadata(&path)
            .map_err(|_| "Extension payload is incomplete.")?
            .file_type()
            .is_symlink()
        {
            return Err("Extension payload contains a symbolic link.".into());
        }
    }
    if !fs::symlink_metadata(&path)
        .map_err(|_| "Extension file is unavailable.")?
        .is_file()
    {
        return Err("Extension payload is not a regular file.".into());
    }
    let bytes = fs::read(path).map_err(|_| "Extension payload could not be read.")?;
    if !hex(&file.sha256, 64) || format!("{:x}", Sha256::digest(&bytes)) != file.sha256 {
        return Err("Extension payload hash does not match its reviewed release.".into());
    }
    Ok(bytes)
}
fn folder(app: &tauri::AppHandle, extension: &Extension) -> Result<PathBuf, String> {
    let version = extension
        .version
        .as_deref()
        .filter(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit() || c == b'.'))
        .ok_or("Extension version is unavailable.")?;
    let hash = extension
        .payload_sha256
        .as_deref()
        .filter(|value| hex(value, 64))
        .ok_or("Extension payload hash is unavailable.")?;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| "Relay support folder is unavailable.")?
        .join("extensions")
        .join(&extension.id)
        .join(format!("{version}-{hash}")))
}
fn selected(app: &tauri::AppHandle, id: &str) -> Result<(Extension, PathBuf), String> {
    let (catalog, root) = catalog(app)?;
    let extension = catalog
        .extensions
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("Unknown extension.")?;
    if !extension.available {
        return Err(extension
            .reason
            .unwrap_or("Reviewed extension payload is unavailable.".into()));
    }
    if !extension
        .source_commit
        .as_deref()
        .is_some_and(|v| hex(v, 40))
        || !extension.files.iter().any(|f| f.path == "manifest.json")
    {
        return Err("Reviewed extension provenance is incomplete.".into());
    }
    let mut lock: Vec<String> = extension
        .files
        .iter()
        .map(|file| format!("{} {}", file.path, file.sha256))
        .collect();
    lock.sort();
    if extension.payload_sha256.as_deref()
        != Some(format!("{:x}", Sha256::digest(lock.join("\n").as_bytes())).as_str())
    {
        return Err("Extension payload lock hash is invalid.".into());
    }
    let source = root.join(id);
    let manifest_file = extension
        .files
        .iter()
        .find(|file| file.path == "manifest.json")
        .ok_or("Manifest unavailable.")?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&verified_file(&source, manifest_file)?)
            .map_err(|_| "Extension manifest is invalid.")?;
    if manifest.get("manifest_version").and_then(|v| v.as_u64()) != Some(3)
        || manifest.get("version").and_then(|v| v.as_str()) != extension.version.as_deref()
    {
        return Err("Extension manifest does not match its reviewed version.".into());
    }
    Ok((extension, source))
}
fn verify_folder(root: &Path, extension: &Extension) -> Result<(), String> {
    fn walk(root: &Path, path: &Path, found: &mut Vec<String>) -> Result<(), String> {
        for entry in fs::read_dir(path).map_err(|_| "Could not inspect extension folder.")? {
            let entry = entry.map_err(|_| "Could not inspect extension file.")?;
            let kind = entry
                .file_type()
                .map_err(|_| "Could not inspect extension file type.")?;
            if kind.is_dir() {
                walk(root, &entry.path(), found)?;
            } else if kind.is_file() {
                found.push(
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|_| "Unsafe extension path.")?
                        .to_string_lossy()
                        .into_owned(),
                );
            } else {
                return Err("Existing extension folder contains an unexpected file or link; it was not changed.".into());
            }
        }
        Ok(())
    }
    if fs::symlink_metadata(root)
        .map_err(|_| "Extension folder is unavailable.")?
        .file_type()
        .is_symlink()
    {
        return Err("Extension folder is a symbolic link; it was not changed.".into());
    }
    let mut found = Vec::new();
    walk(root, root, &mut found)?;
    found.sort();
    let mut expected: Vec<String> = extension
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    expected.sort();
    if found != expected {
        return Err("Existing extension folder has changed; it was not overwritten.".into());
    }
    for file in &extension.files {
        verified_file(root, file)?;
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn get_extension_setup(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<Setup, String> {
    super::local_only(&window)?;
    let (catalog, _) = catalog(&app)?;
    Ok(Setup {
        browsers: browsers()?,
        extensions: catalog.extensions,
    })
}
#[tauri::command]
pub(crate) async fn open_extension_settings(
    window: tauri::WebviewWindow,
    browser_id: String,
) -> Result<(), String> {
    super::local_only(&window)?;
    let browser = browsers()?
        .into_iter()
        .find(|b| b.id == browser_id && b.detected)
        .ok_or("Choose a detected browser first.")?;
    let status = Command::new("/usr/bin/open")
        .args(["-b", &browser.bundle_id, &browser.settings_url])
        .status()
        .map_err(|_| "Could not open browser extension settings.")?;
    if status.success() {
        Ok(())
    } else {
        Err(
            "Browser refused its extension settings address. Open it manually in that browser."
                .into(),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn bundled_versions_compare_by_number_not_text() {
        assert!(is_newer("1.3.18", "1.3.5"));
        assert!(is_newer("0.50.16", "0.50.15"));
        assert!(is_newer("2", "1.9.9"));
        assert!(!is_newer("0.7.28", "0.7.28"));
        assert!(!is_newer("0.7.9", "0.7.28"));
        assert!(!is_newer("1.0.0", "1.0.1"));
    }
}
// Copies one verified bundled payload into Relay's support folder. Version/hash folders are
// immutable: an existing folder is verified and left alone, never rewritten.
fn prepare_copy(app: &tauri::AppHandle, extension: &Extension, source: &Path) -> Result<String, String> {
    let target = folder(app, extension)?;
    let parent = target
        .parent()
        .ok_or("Relay extension folder is unavailable.")?;
    let support = app
        .path()
        .app_data_dir()
        .map_err(|_| "Relay support folder is unavailable.")?;
    fs::create_dir_all(&support).map_err(|_| "Could not create Relay support folder.")?;
    for path in [support.join("extensions"), parent.to_path_buf()] {
        if path
            .symlink_metadata()
            .is_ok_and(|info| !info.is_dir() || info.file_type().is_symlink())
        {
            return Err(
                "Managed extension folder is not a plain directory; it was not changed.".into(),
            );
        }
        fs::create_dir_all(path).map_err(|_| "Could not create Relay's extension folder.")?;
    }
    if fs::symlink_metadata(parent)
        .map_err(|_| "Could not check Relay's extension folder.")?
        .file_type()
        .is_symlink()
    {
        return Err("Relay's extension folder is a symbolic link; it was not changed.".into());
    }
    if target.exists() {
        if fs::symlink_metadata(&target)
            .map_err(|_| "Could not check existing extension folder.")?
            .file_type()
            .is_symlink()
        {
            return Err("Existing extension folder is a symbolic link; it was not changed.".into());
        }
        verify_folder(&target, extension)?;
        return Ok(target.to_string_lossy().into_owned());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Clock unavailable.")?
        .as_nanos();
    let staging = parent.join(format!(".pending-{}-{nonce}", std::process::id()));
    fs::create_dir(&staging).map_err(|_| "Could not stage extension files.")?;
    for file in &extension.files {
        let bytes = verified_file(source, file)?;
        let output = staging.join(relative(&file.path)?);
        fs::create_dir_all(output.parent().ok_or("Invalid payload path.")?)
            .map_err(|_| "Could not stage extension folder.")?;
        fs::write(output, bytes)
            .map_err(|_| "Could not stage extension file. Existing versions were not changed.")?;
    }
    fs::rename(staging, &target)
        .map_err(|_| "Could not finish extension folder. Existing versions were not changed.")?;
    // New version/hash folders only. Never mutate/reload installed workers, even when their state is unknown.
    Ok(target.to_string_lossy().into_owned())
}
#[tauri::command]
pub(crate) async fn prepare_extension(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    extension_id: String,
) -> Result<String, String> {
    super::local_only(&window)?;
    let (extension, source) = selected(&app, &extension_id)?;
    prepare_copy(&app, &extension, &source)
}

fn version_parts(value: &str) -> Option<Vec<u64>> {
    value.split('.').map(|part| part.parse().ok()).collect()
}
fn is_newer(candidate: &str, installed: &str) -> bool {
    let (Some(left), Some(right)) = (version_parts(candidate), version_parts(installed)) else {
        return false;
    };
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}
// Versions already present in Relay's support folder for one extension.
fn installed_versions(app: &tauri::AppHandle, id: &str) -> Vec<String> {
    let Ok(support) = app.path().app_data_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(support.join("extensions").join(id)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let (version, hash) = name.to_str()?.split_once('-')?;
            let plain_hash = hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
            (plain_hash && version_parts(version).is_some()).then(|| version.to_string())
        })
        .collect()
}
/// On launch, copy the bundled payload into Relay's support folder when the app carries a
/// newer version than the newest copy already there. Existing folders are never changed and
/// no browser is reloaded. Problems are returned so the page can report them.
pub(crate) fn refresh_bundled_copies(app: &tauri::AppHandle) -> Vec<String> {
    let mut problems = Vec::new();
    let (catalog, _) = match catalog(app) {
        Ok(value) => value,
        Err(error) => return vec![error],
    };
    for entry in catalog.extensions.iter().filter(|entry| entry.available) {
        let (extension, source) = match selected(app, &entry.id) {
            Ok(value) => value,
            Err(error) => {
                problems.push(format!("{}: {error}", entry.id));
                continue;
            }
        };
        let target = match folder(app, &extension) {
            Ok(path) => path,
            Err(error) => {
                problems.push(format!("{}: {error}", entry.id));
                continue;
            }
        };
        if target.exists() {
            continue;
        }
        let bundled = extension.version.clone().unwrap_or_default();
        if installed_versions(app, &extension.id)
            .into_iter()
            .any(|installed| !is_newer(&bundled, &installed))
        {
            continue;
        }
        if let Err(error) = prepare_copy(app, &extension, &source) {
            problems.push(format!("{}: {error}", extension.id));
        }
    }
    problems
}
#[tauri::command]
pub(crate) async fn show_extension_folder(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    extension_id: String,
) -> Result<(), String> {
    super::local_only(&window)?;
    let (extension, _) = selected(&app, &extension_id)?;
    let path = folder(&app, &extension)?;
    verify_folder(&path, &extension)?;
    let status = Command::new("/usr/bin/open")
        .arg(path)
        .status()
        .map_err(|_| "Could not show extension folder.")?;
    if status.success() {
        Ok(())
    } else {
        Err("Could not show extension folder.".into())
    }
}
