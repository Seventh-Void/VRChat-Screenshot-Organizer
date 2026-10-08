use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use organizer_core::{
    avatar_people, avatar_person_photos, cached_library, clear_cancel_request,
    delete_positional_person_tag, extract_image_meta, generate_thumbnail, is_ignored_path,
    is_image_path, organize_path, organize_single_file, positional_person_tags, read_activity_log,
    request_cancel, sanitize_name, save_positional_person_tag, scan_library_with_cache,
    set_photo_tags, tag_photo_participant_in_store, undo_organization,
    update_positional_person_tag, validate_template, ActivityEntry, AvatarPerson,
    AvatarPersonPhoto, OrganizerConfig, OrganizerStats,
};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State};

/// Managed watcher state. Every started watcher thread owns one `generation`
/// and exits once it changes, so stop→start can never leave two threads.
#[derive(Default)]
pub struct WatcherState {
    pub running: AtomicBool,
    pub generation: AtomicU64,
}

/// The approved library root: (tilde-expanded path the user sees, canonical
/// form for containment checks). Every library command works inside it; the
/// webview never chooses which folder a command touches.
#[derive(Default)]
struct LibraryRoot(Mutex<Option<(String, PathBuf)>>);

/// Background library scan slot: (runner active, latest queued request).
#[derive(Default)]
struct LibraryScan(Mutex<(bool, Option<(PathBuf, bool)>)>);

/// Kept alive because dropping an `arboard::Clipboard` loses the copied text on Linux.
#[derive(Default)]
struct ClipboardState(Mutex<Option<arboard::Clipboard>>);

fn expand(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).as_ref())
}

/// Library root as used for library cache keys (forward slashes, like scanned photo paths).
fn library_base(folder_path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(folder_path).replace('\\', "/"))
}

/// Checks that `path` is an existing file inside `folder` (both canonicalized, so
/// `..` and symlinks cannot escape) and returns the tilde-expanded, non-canonical
/// path: organizer-core keys caches and thumbnails by the path the UI knows.
fn library_path(folder: &Path, path: &str) -> Result<PathBuf, String> {
    let base = std::fs::canonicalize(folder)
        .map_err(|error| format!("screenshot folder is not accessible: {error}"))?;
    let expanded = expand(path);
    let canonical = std::fs::canonicalize(&expanded)
        .map_err(|error| format!("photo is not accessible: {error}"))?;
    if !canonical.starts_with(&base) || !canonical.is_file() {
        return Err("photo is outside the selected screenshot folder".to_string());
    }
    Ok(expanded)
}

fn library_root(app: &AppHandle) -> Result<(String, PathBuf), String> {
    app.state::<LibraryRoot>()
        .inner()
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "no screenshot folder selected".to_string())
}

fn root_folder(app: &AppHandle) -> Result<String, String> {
    library_root(app).map(|(folder, _)| folder)
}

/// A library root must be an existing, readable folder and never a filesystem
/// or drive root or the home folder itself: the asset protocol serves it all.
fn approve_root(folder: &str, home: Option<&Path>) -> Result<(String, PathBuf), String> {
    let expanded = shellexpand::tilde(folder).into_owned();
    let canonical = std::fs::canonicalize(&expanded)
        .and_then(|canonical| std::fs::read_dir(&canonical).map(|_| canonical))
        .map_err(|error| format!("folder is not accessible: {expanded}: {error}"))?;
    let is_home = home
        .and_then(|home| std::fs::canonicalize(home).ok())
        .is_some_and(|home| home == canonical);
    if canonical.parent().is_none() || is_home {
        return Err("choose the VRChat screenshot folder, not a drive or home folder".to_string());
    }
    Ok((expanded, canonical))
}

fn root_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("library-root.txt"))
        .map_err(|error| format!("could not locate app data directory: {error}"))
}

/// Saves the approved root. `first_only` uses create_new, so a legacy adoption
/// can never overwrite a root the user already chose.
fn persist_root(file: &Path, folder: &str, first_only: bool) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("could not create app data directory: {error}"))?;
    }
    if first_only {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(file)
            .and_then(|mut saved| saved.write_all(folder.as_bytes()))
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::AlreadyExists => {
                    "a screenshot folder is already selected".to_string()
                }
                _ => format!("could not save screenshot folder: {error}"),
            })
    } else {
        std::fs::write(file, folder)
            .map_err(|error| format!("could not save screenshot folder: {error}"))
    }
}

fn activate_root(
    app: &AppHandle,
    (folder, canonical): (String, PathBuf),
) -> Result<String, String> {
    // ponytail: scope only grows; a previously chosen root stays readable until restart
    // (Tauri can only "forbid", which would also block a later re-selection).
    app.asset_protocol_scope()
        .allow_directory(&canonical, true)
        .map_err(|error| format!("could not allow folder access: {error}"))?;
    *app.state::<LibraryRoot>().inner().0.lock().unwrap() = Some((folder.clone(), canonical));
    Ok(folder)
}

fn select_root(app: &AppHandle, folder: &str, first_only: bool) -> Result<String, String> {
    let root = approve_root(folder, app.path().home_dir().ok().as_deref())?;
    persist_root(&root_file(app)?, &root.0, first_only)?;
    activate_root(app, root)
}

/// Activates the saved library root, if any.
#[tauri::command(async)]
fn get_library_root(app: AppHandle) -> Result<Option<String>, String> {
    let saved = match std::fs::read_to_string(root_file(&app)?) {
        Ok(saved) => saved,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read saved screenshot folder: {error}")),
    };
    let root = approve_root(&saved, app.path().home_dir().ok().as_deref())?;
    activate_root(&app, root).map(Some)
}

/// Native folder picker; the chosen folder becomes the library root.
#[tauri::command]
async fn pick_library_folder(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(picked) = app
            .dialog()
            .file()
            .set_title("Choose your VRChat screenshot folder")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(|error| error.to_string())?;
        select_root(&app, &path.to_string_lossy(), false).map(Some)
    })
    .await
    .map_err(|error| format!("folder picker failed: {error}"))?
}

/// Uses the first auto-detected VRChat folder that exists as the library root.
#[tauri::command(async)]
fn use_detected_folder(app: AppHandle) -> Result<String, String> {
    let folder = default_candidates()
        .into_iter()
        .find(|candidate| expand(candidate).is_dir())
        .ok_or_else(|| "no VRChat screenshot folder was found".to_string())?;
    select_root(&app, &folder, false)
}

/// One-time migration of the folder older versions kept in localStorage.
#[tauri::command(async)]
fn adopt_legacy_folder(app: AppHandle, folder_path: String) -> Result<String, String> {
    select_root(&app, &folder_path, true)
}

fn library_cache_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("library.sqlite"))
        .map_err(|error| format!("could not locate app data directory: {error}"))
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryEstimate {
    screenshot_count: u64,
    total_bytes: u64,
    estimated_processing_seconds: u64,
}

#[tauri::command]
async fn estimate_library(app: AppHandle) -> Result<LibraryEstimate, String> {
    let folder_path = root_folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let base_path = expand(&folder_path);
        if !base_path.is_dir() {
            return Err(format!("folder does not exist: {}", base_path.display()));
        }

        let mut screenshot_count = 0_u64;
        let mut total_bytes = 0_u64;
        for entry in walkdir::WalkDir::new(&base_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !is_ignored_path(entry.path(), &base_path))
            .filter_map(Result::ok)
        {
            if entry.file_type().is_file() && is_image_path(entry.path()) {
                screenshot_count += 1;
                total_bytes = total_bytes
                    .saturating_add(entry.metadata().map_or(0, |metadata| metadata.len()));
            }
        }

        Ok(LibraryEstimate {
            screenshot_count,
            total_bytes,
            estimated_processing_seconds: (screenshot_count.saturating_mul(2).saturating_add(99))
                / 100,
        })
    })
    .await
    .map_err(|error| format!("estimate task failed: {error}"))?
}

fn organize_folder_blocking(
    folder_path: String,
    dry_run: bool,
    scan_all_months: bool,
    single_folder: bool,
    template: String,
) -> Result<OrganizerStats, String> {
    let expanded = shellexpand::tilde(&folder_path).to_string();
    let template = if template.trim().is_empty() {
        "{world}".to_string()
    } else {
        template.trim().to_string()
    };
    validate_template(&template).map_err(|err| err.to_string())?;
    let config = OrganizerConfig {
        base_path: PathBuf::from(expanded.clone()),
        dry_run,
        scan_all_months,
        single_folder,
        template,
    };

    let mut stats = OrganizerStats::default();
    organize_path(&config, &mut stats).map_err(|err| err.to_string())?;
    Ok(stats)
}

#[tauri::command]
async fn organize_folder(
    app: AppHandle,
    dry_run: bool,
    scan_all_months: bool,
    single_folder: bool,
    template: String,
) -> Result<OrganizerStats, String> {
    let folder_path = root_folder(&app)?;
    clear_cancel_request();
    let result = tauri::async_runtime::spawn_blocking(move || {
        organize_folder_blocking(
            folder_path,
            dry_run,
            scan_all_months,
            single_folder,
            template,
        )
    })
    .await
    .map_err(|err| format!("scan task failed: {err}"))?;
    clear_cancel_request();
    result
}

#[tauri::command]
fn cancel_scan() -> Result<String, String> {
    request_cancel();
    Ok("cancelling".to_string())
}

#[tauri::command]
async fn simulate_folder(
    app: AppHandle,
    scan_all_months: bool,
    single_folder: bool,
    template: String,
) -> Result<OrganizerStats, String> {
    organize_folder(app, true, scan_all_months, single_folder, template).await
}

#[tauri::command(async)]
fn get_activity(app: AppHandle) -> Result<Vec<ActivityEntry>, String> {
    read_activity_log(&expand(&root_folder(&app)?)).map_err(|err| err.to_string())
}

#[tauri::command]
async fn get_library(app: AppHandle, scan_all_months: bool) -> Result<OrganizerStats, String> {
    let cache_path = library_cache_path(&app)?;
    let base_path = library_base(&root_folder(&app)?);
    let snapshot =
        cached_library(&cache_path, &base_path, scan_all_months).map_err(|err| err.to_string())?;
    queue_library_scan(app, cache_path, base_path, scan_all_months);
    Ok(snapshot.unwrap_or_default())
}

/// Runs at most one background library scan at a time; requests made while it
/// runs collapse into a single rerun with the latest parameters.
fn queue_library_scan(app: AppHandle, cache_path: PathBuf, base_path: PathBuf, all_months: bool) {
    {
        let mut slot = app.state::<LibraryScan>().inner().0.lock().unwrap();
        slot.1 = Some((base_path, all_months));
        if std::mem::replace(&mut slot.0, true) {
            return;
        }
    }
    tauri::async_runtime::spawn_blocking(move || loop {
        let next = {
            let mut slot = app.state::<LibraryScan>().inner().0.lock().unwrap();
            let next = slot.1.take();
            slot.0 = next.is_some();
            next
        };
        let Some((base_path, all_months)) = next else {
            return;
        };
        // A panic must not leave the slot marked as running forever.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scan_library_with_cache(&base_path, all_months, Some(&cache_path))
        })) {
            Ok(Ok(stats)) => {
                let _ = app.emit("library-updated", stats);
            }
            Ok(Err(error)) => {
                let _ = app.emit(
                    "library-error",
                    serde_json::json!({ "error": error.to_string() }),
                );
            }
            Err(_) => {
                let _ = app.emit(
                    "library-error",
                    serde_json::json!({ "error": "library scan crashed" }),
                );
            }
        }
    });
}

#[tauri::command]
async fn get_thumbnail(app: AppHandle, photo_path: String) -> Result<String, String> {
    let (folder_path, root) = library_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let photo_path = library_path(&root, &photo_path)?;
        generate_thumbnail(&photo_path, &expand(&folder_path))
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|error| format!("thumbnail generation failed: {error}"))
    })
    .await
    .map_err(|error| format!("thumbnail task failed: {error}"))?
}

#[tauri::command]
async fn tag_photo_participant(
    app: AppHandle,
    photo_path: String,
    participant: String,
) -> Result<Vec<String>, String> {
    let (folder_path, root) = library_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let base_path = expand(&folder_path);
        let photo_path = library_path(&root, &photo_path)?;
        tag_photo_participant_in_store(&base_path, &photo_path, &participant)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("tagging task failed: {error}"))?
}

#[tauri::command]
async fn get_positional_person_tags(
    app: AppHandle,
    screenshot_path: String,
) -> Result<Vec<organizer_core::PositionalPersonTag>, String> {
    let db_path = recognition_db_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        positional_person_tags(
            &db_path,
            &PathBuf::from(shellexpand::tilde(&screenshot_path).to_string()),
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("tag lookup task failed: {error}"))?
}

#[tauri::command]
async fn save_positional_person_tag_command(
    app: AppHandle,
    screenshot_path: String,
    person_name: String,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<organizer_core::PositionalPersonTag, String> {
    let db_path = recognition_db_path(&app)?;
    let (_, root) = library_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        save_positional_person_tag(
            &db_path,
            &library_path(&root, &screenshot_path)?,
            &person_name,
            x,
            y,
            width,
            height,
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("tag save task failed: {error}"))?
}

#[tauri::command(async)]
fn delete_positional_person_tag_command(app: AppHandle, tag_id: i64) -> Result<(), String> {
    delete_positional_person_tag(&recognition_db_path(&app)?, tag_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn update_positional_person_tag_command(
    app: AppHandle,
    tag_id: i64,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<organizer_core::PositionalPersonTag, String> {
    let db_path = recognition_db_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        update_positional_person_tag(&db_path, tag_id, x, y, width, height)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("tag update task failed: {error}"))?
}

#[tauri::command]
async fn set_photo_participant_tags(
    app: AppHandle,
    photo_path: String,
    participants: Vec<String>,
) -> Result<Vec<String>, String> {
    let (folder_path, root) = library_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let base_path = expand(&folder_path);
        let photo_path = library_path(&root, &photo_path)?;
        set_photo_tags(&base_path, &photo_path, &participants).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("tag editing task failed: {error}"))?
}

#[tauri::command]
async fn undo_last_run(app: AppHandle) -> Result<OrganizerStats, String> {
    let folder_path = root_folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let expanded = shellexpand::tilde(&folder_path).to_string();
        let base_path = PathBuf::from(expanded);
        let mut stats = OrganizerStats::default();
        undo_organization(&base_path, &mut stats).map_err(|err| err.to_string())?;
        Ok(stats)
    })
    .await
    .map_err(|err| format!("undo task failed: {err}"))?
}

/// Likely VRChat screenshot folders, most specific first.
fn default_candidates() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
    if cfg!(target_os = "windows") {
        let userprofile =
            std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
        vec![
            format!("{}/Pictures/VRChat", userprofile.replace('\\', "/")),
            format!("{}/Pictures/VRChat/VRChat", userprofile.replace('\\', "/")),
        ]
    } else {
        // Linux / Steam Proton paths
        vec![
            format!("{}/.local/share/Steam/steamapps/compatdata/438100/pfx/drive_c/users/steamuser/Pictures/VRChat", home),
            format!("{}/Pictures/VRChat", home),
            format!("{}/Pictures/VRChat/VRChat", home),
        ]
    }
}

/// Auto-detect the VRChat screenshot folder across platforms (display only).
#[tauri::command]
fn get_default_path() -> String {
    let candidates = default_candidates();
    candidates
        .iter()
        .find(|path| expand(path).exists())
        .unwrap_or(&candidates[0])
        .clone()
}

#[tauri::command(async)]
fn open_photo_location(app: AppHandle, path: String) -> Result<(), String> {
    let photo = library_path(&library_root(&app)?.1, &path)?;

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe")
        .arg("/select,")
        // Explorer's /select rejects forward slashes, which library paths use.
        .arg(photo.to_string_lossy().replace('/', "\\"))
        .spawn();
    // Canonical folders are absolute, so they can never be read as a `-option`.
    #[cfg(not(target_os = "windows"))]
    let folder = std::fs::canonicalize(&photo)
        .map_err(|error| format!("photo is not accessible: {error}"))?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "photo has no folder".to_string())?;
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(&folder).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&folder).spawn();
    result
        .map(|_| ())
        .map_err(|error| format!("could not open photo location: {error}"))
}

#[tauri::command(async)]
fn copy_photo_path(
    app: AppHandle,
    clipboard: State<'_, ClipboardState>,
    path: String,
) -> Result<(), String> {
    let photo = library_path(&library_root(&app)?.1, &path)?;
    let mut clipboard = clipboard.0.lock().unwrap();
    if clipboard.is_none() {
        *clipboard = Some(
            arboard::Clipboard::new()
                .map_err(|error| format!("could not access clipboard: {error}"))?,
        );
    }
    clipboard
        .as_mut()
        .expect("clipboard initialised above")
        .set_text(photo.to_string_lossy())
        .map_err(|error| format!("could not copy photo path: {error}"))
}

/// A collection is a plain folder in the library root: reject anything the
/// organizer would rename, hide, treat as a month folder, or already owns.
fn valid_collection_name(name: &str) -> bool {
    let month_like = name.len() == 7
        && name.bytes().enumerate().all(|(index, byte)| {
            if index == 4 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        });
    sanitize_name(name) == name
        && !name.starts_with('.')
        && !month_like
        && !name.eq_ignore_ascii_case("Prints")
        && !name.eq_ignore_ascii_case("vrchat-organizer-thumbnails")
}

/// Creates `<dir>/<name>`, or `<stem> (n).<ext>` if taken, without following or
/// replacing anything that already exists there.
fn create_unique(dir: &Path, name: &std::ffi::OsStr) -> std::io::Result<(std::fs::File, PathBuf)> {
    let name = Path::new(name);
    let stem = name.file_stem().unwrap_or_default().to_string_lossy();
    let extension = name.extension().map(|value| value.to_string_lossy());
    for suffix in 1.. {
        let target = match (suffix, &extension) {
            (1, _) => dir.join(name),
            (_, Some(extension)) => dir.join(format!("{stem} ({suffix}).{extension}")),
            (_, None) => dir.join(format!("{stem} ({suffix})")),
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(file) => return Ok((file, target)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    unreachable!("suffixes are unbounded")
}

#[tauri::command]
async fn add_photos_to_collection(
    app: AppHandle,
    collection_name: String,
    photo_paths: Vec<String>,
) -> Result<usize, String> {
    let (_, root) = library_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let collection_name = collection_name.trim();
        if !valid_collection_name(collection_name) {
            return Err("invalid collection name".to_string());
        }

        let destination = root.join(collection_name);
        match std::fs::create_dir(&destination) {
            Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                return Err(format!("could not create collection folder: {error}"));
            }
            _ => {}
        }
        // An existing entry may be a symlink leading out of the library.
        if !std::fs::symlink_metadata(&destination).is_ok_and(|meta| meta.file_type().is_dir()) {
            return Err("collection folder is not a plain folder".to_string());
        }
        let mut copied = 0;
        for source_string in photo_paths {
            let source = library_path(&root, &source_string)
                .and_then(|source| std::fs::canonicalize(source).map_err(|error| error.to_string()))
                .map_err(|error| format!("{error}: {source_string}"))?;
            let file_name = source
                .file_name()
                .ok_or_else(|| format!("photo has no file name: {source_string}"))?;
            let copy = |(mut output, target): (std::fs::File, PathBuf)| {
                let result = std::fs::File::open(&source)
                    .and_then(|mut input| std::io::copy(&mut input, &mut output));
                if result.is_err() {
                    let _ = std::fs::remove_file(&target);
                }
                result
            };
            create_unique(&destination, file_name)
                .and_then(copy)
                .map_err(|error| format!("could not copy {}: {error}", source.display()))?;
            copied += 1;
        }
        Ok(copied)
    })
    .await
    .map_err(|error| format!("collection copy task failed: {error}"))?
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct VrChatStatus {
    vrcx_running: bool,
    vrchat_running: bool,
    vrchat_started_at: Option<u64>,
}

/// Check whether VRCX and VRChat are running, including the current VRChat session start.
#[tauri::command]
async fn get_vrchat_status() -> VrChatStatus {
    // `sysinfo` process enumeration is blocking and can take tens of ms; run it
    // on a blocking thread so the async UI doesn't stall on the 5s poll.
    tauri::async_runtime::spawn_blocking(|| {
        // We need to refresh the process list each call for a live check
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let mut vrcx_running = false;
        let mut vrchat_started_at = None;
        for process in system.processes().values() {
            if let Some(name) = process.name().to_str() {
                let lower = name.to_lowercase();
                if lower.starts_with("vrcx") || lower.contains("vrcx") {
                    vrcx_running = true;
                }
                if lower == "vrchat" || lower == "vrchat.exe" || lower.starts_with("vrchat.") {
                    let started_at = process.start_time();
                    vrchat_started_at = Some(
                        vrchat_started_at
                            .map_or(started_at, |current: u64| current.min(started_at)),
                    );
                }
            }
        }
        VrChatStatus {
            vrcx_running,
            vrchat_running: vrchat_started_at.is_some(),
            vrchat_started_at,
        }
    })
    .await
    .unwrap_or(VrChatStatus {
        vrcx_running: false,
        vrchat_running: false,
        vrchat_started_at: None,
    })
}

/// Check if there's an undo log available.
#[tauri::command(async)]
fn has_undo(app: AppHandle) -> bool {
    root_folder(&app)
        .is_ok_and(|folder| expand(&folder).join(".vrchat-organizer-undo.json").exists())
}

fn collect_watch_paths(
    event: &Event,
    base_path: &Path,
    pending: &mut std::collections::HashSet<PathBuf>,
) {
    if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
        return;
    }

    for path in &event.paths {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        if !matches!(extension.as_deref(), Some("png" | "jpg" | "jpeg" | "webp"))
            || path
                .strip_prefix(base_path)
                .unwrap_or(path)
                .components()
                .any(|component| {
                    let name = component.as_os_str().to_string_lossy();
                    name == "Prints"
                        || name == "vrchat-organizer-thumbnails"
                        || name.starts_with('.')
                })
        {
            continue;
        }
        pending.insert(path.clone());
    }
}

fn watch_fingerprint(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}

fn wait_for_stable_file(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let mut previous = None;
    for _ in 0..20 {
        let current = watch_fingerprint(path)?;
        if previous.as_ref() == Some(&current) {
            return Some(current);
        }
        previous = Some(current);
        std::thread::sleep(Duration::from_millis(150));
    }
    None
}

fn wait_for_capture_metadata(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let fingerprint = wait_for_stable_file(path)?;
        match extract_image_meta(path) {
            Ok(meta) if meta.world_name.is_some() || (meta.width, meta.height) == (2048, 1440) => {
                return Some(fingerprint);
            }
            Ok(_) if Instant::now() >= deadline => return Some(fingerprint),
            Err(_) if Instant::now() >= deadline => return Some(fingerprint),
            Ok(_) | Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

/// Start watching the folder for new files and auto-organize them.
/// Filesystem notifications are batched briefly because a single capture is
/// commonly reported as several create/modify events while it is written.
#[tauri::command]
async fn start_watching(
    app: AppHandle,
    dry_run: bool,
    scan_all_months: bool,
    single_folder: bool,
    template: String,
    state: State<'_, WatcherState>,
) -> Result<String, String> {
    let expanded = shellexpand::tilde(&root_folder(&app)?).to_string();
    let base_path = PathBuf::from(expanded.clone());

    if !base_path.exists() {
        return Err(format!("path does not exist: {}", expanded));
    }

    let config = OrganizerConfig {
        base_path: base_path.clone(),
        dry_run,
        scan_all_months,
        single_folder,
        template: if template.trim().is_empty() {
            "{world}".to_string()
        } else {
            template.trim().to_string()
        },
    };
    validate_template(&config.template).map_err(|err| err.to_string())?;

    let watch_path = base_path.clone();
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("already watching".to_string());
    }
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.emit(
        "watch-status",
        serde_json::json!({
          "status": "started",
          "path": watch_path.to_string_lossy()
        }),
    );

    let app_handle = app.clone();
    let watch_path_clone = watch_path.clone();
    let config_clone = config.clone();

    std::thread::spawn(move || {
        let current = || {
            app_handle
                .state::<WatcherState>()
                .generation
                .load(Ordering::SeqCst)
                == generation
        };
        let fail = |error: String| {
            let _ = app_handle.emit("watch-error", serde_json::json!({ "error": error }));
            if current() {
                app_handle
                    .state::<WatcherState>()
                    .running
                    .store(false, Ordering::SeqCst);
                let _ = app_handle.emit("watch-status", serde_json::json!({"status": "stopped"}));
            }
        };
        let (tx, rx) = std::sync::mpsc::channel::<Result<Event, notify::Error>>();

        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => return fail(format!("Failed to create watcher: {e}")),
        };

        if let Err(e) = watcher.watch(&watch_path_clone, RecursiveMode::Recursive) {
            return fail(format!("Failed to watch: {e}"));
        }

        // Finish the initial organization before consuming notifications. This
        // prevents the watcher's own moves and metadata writes from becoming a
        // second organization pass.
        if !current() {
            return;
        }
        clear_cancel_request();
        let mut initial_stats = OrganizerStats::default();
        let initial_result = organize_path(&config_clone, &mut initial_stats);
        let _ = app_handle.emit(
            "watch-initial-scan",
            match initial_result {
                Ok(()) => serde_json::json!({"stats": initial_stats}),
                Err(error) => serde_json::json!({"error": error.to_string()}),
            },
        );
        while rx.try_recv().is_ok() {}

        let mut processed = std::collections::HashMap::new();
        let mut processed_order = VecDeque::new();
        while current() {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    let mut pending = std::collections::HashSet::new();
                    collect_watch_paths(&event, &watch_path_clone, &mut pending);
                    let deadline = Instant::now() + Duration::from_millis(700);
                    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                        match rx.recv_timeout(remaining) {
                            Ok(Ok(next)) => {
                                collect_watch_paths(&next, &watch_path_clone, &mut pending)
                            }
                            Ok(Err(error)) => {
                                let _ = app_handle.emit(
                                    "watch-error",
                                    serde_json::json!({"error": error.to_string()}),
                                );
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }

                    for path in pending {
                        if !current() {
                            return;
                        }
                        // Wait for the capture writer to finish. A create event
                        // can arrive before the file is complete, and a move
                        // event can refer to a path that no longer exists.
                        let Some(fingerprint) = wait_for_capture_metadata(&path) else {
                            continue;
                        };
                        if processed.get(&path) == Some(&fingerprint) {
                            continue;
                        }
                        let mut stats = OrganizerStats::default();
                        match organize_single_file(&path, &config_clone, &mut stats) {
                            Ok(()) if stats.organized > 0 || stats.no_metadata > 0 => {
                                let _ = app_handle.emit(
                                    "watch-organized",
                                    serde_json::json!({
                                        "stats": stats,
                                        "file": path.to_string_lossy()
                                    }),
                                );
                            }
                            Ok(()) => {}
                            Err(error) => {
                                let _ = app_handle.emit(
                                    "watch-error",
                                    serde_json::json!({
                                        "error": error.to_string(),
                                        "file": path.to_string_lossy()
                                    }),
                                );
                            }
                        }
                        if processed.insert(path.clone(), fingerprint).is_none() {
                            processed_order.push_back(path);
                        }
                        while processed_order.len() > 4096 {
                            if let Some(old_path) = processed_order.pop_front() {
                                processed.remove(&old_path);
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    let _ =
                        app_handle.emit("watch-error", serde_json::json!({"error": e.to_string()}));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }
    });

    Ok("watching".to_string())
}

/// Stop watching the folder.
#[tauri::command]
fn stop_watching(app: AppHandle, state: State<'_, WatcherState>) -> Result<String, String> {
    // Retire the thread first so a start racing this stop gets a fresh generation.
    state.generation.fetch_add(1, Ordering::SeqCst);
    state.running.store(false, Ordering::SeqCst);

    let _ = app.emit("watch-status", serde_json::json!({"status": "stopped"}));

    Ok("stopped".to_string())
}

/// Check if watcher is running.
#[tauri::command]
fn is_watching(state: State<'_, WatcherState>) -> bool {
    state.running.load(Ordering::SeqCst)
}

fn recognition_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("avatar-recognition.sqlite"))
        .map_err(|error| format!("could not locate recognition database directory: {error}"))
}

#[tauri::command(async)]
fn get_avatar_people(app: AppHandle) -> Result<Vec<AvatarPerson>, String> {
    avatar_people(&recognition_db_path(&app)?).map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn get_avatar_person_photos(
    app: AppHandle,
    person_id: i64,
) -> Result<Vec<AvatarPersonPhoto>, String> {
    avatar_person_photos(&recognition_db_path(&app)?, person_id).map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(WatcherState::default())
        .manage(LibraryRoot::default())
        .manage(LibraryScan::default())
        .manage(ClipboardState::default())
        .invoke_handler(tauri::generate_handler![
            organize_folder,
            cancel_scan,
            simulate_folder,
            get_activity,
            get_library,
            get_thumbnail,
            get_positional_person_tags,
            save_positional_person_tag_command,
            delete_positional_person_tag_command,
            update_positional_person_tag_command,
            tag_photo_participant,
            set_photo_participant_tags,
            get_default_path,
            estimate_library,
            open_photo_location,
            copy_photo_path,
            add_photos_to_collection,
            get_library_root,
            pick_library_folder,
            use_detected_folder,
            adopt_legacy_folder,
            undo_last_run,
            has_undo,
            start_watching,
            stop_watching,
            is_watching,
            get_vrchat_status,
            get_avatar_people,
            get_avatar_person_photos,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_path_accepts_inside_and_rejects_escapes() {
        let root = std::env::temp_dir().join(format!("vrco-library-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let library = root.join("library");
        std::fs::create_dir_all(library.join("2026-10")).unwrap();
        let inside = library.join("2026-10/a.png");
        std::fs::write(&inside, b"x").unwrap();
        std::fs::write(root.join("outside.png"), b"x").unwrap();
        let check = |path: PathBuf| library_path(&library, path.to_str().unwrap());

        // Returns the given (non-canonical) path, not the canonical one.
        let dotted = library.join("2026-10/../2026-10/a.png");
        assert_eq!(check(dotted.clone()), Ok(dotted));
        assert_eq!(check(inside), Ok(library.join("2026-10/a.png")));
        assert!(check(library.join("../outside.png")).is_err());
        assert!(
            check(library.join("2026-10")).is_err(),
            "directories are not photos"
        );
        assert!(check(library.join("missing.png")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("outside.png"), library.join("link.png")).unwrap();
            std::os::unix::fs::symlink(&root, library.join("up")).unwrap();
            assert!(check(library.join("link.png")).is_err());
            assert!(check(library.join("up/outside.png")).is_err());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn library_root_rejects_filesystem_root_home_and_missing() {
        let home = std::env::temp_dir().join(format!("vrco-root-{}", std::process::id()));
        let library = home.join("Pictures/VRChat");
        std::fs::create_dir_all(&library).unwrap();
        let approve = |path: &Path| approve_root(path.to_str().unwrap(), Some(&home));

        let (folder, canonical) = approve(&library).unwrap();
        assert_eq!(folder, library.to_str().unwrap());
        assert_eq!(canonical, std::fs::canonicalize(&library).unwrap());
        assert!(approve(&home).is_err(), "home folder itself");
        assert!(approve(&home.join("Pictures/..")).is_err(), "home via ..");
        assert!(approve(Path::new("/")).is_err(), "filesystem root");
        assert!(approve(&home.join("missing")).is_err());
        std::fs::write(home.join("file.png"), b"x").unwrap();
        assert!(approve(&home.join("file.png")).is_err(), "not a folder");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn legacy_root_is_adopted_only_once() {
        let dir = std::env::temp_dir().join(format!("vrco-root-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("app/library-root.txt");
        persist_root(&file, "/first", true).unwrap();
        assert!(persist_root(&file, "/second", true).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "/first");
        persist_root(&file, "/picked", false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "/picked");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collection_copies_never_replace_existing_files() {
        let dir = std::env::temp_dir().join(format!("vrco-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.png"), b"keep").unwrap();
        let (_, second) = create_unique(&dir, std::ffi::OsStr::new("a.png")).unwrap();
        let (_, third) = create_unique(&dir, std::ffi::OsStr::new("a.png")).unwrap();
        assert_eq!(second, dir.join("a (2).png"));
        assert_eq!(third, dir.join("a (3).png"));
        assert_eq!(std::fs::read(dir.join("a.png")).unwrap(), b"keep");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collection_names_must_be_plain_library_folders() {
        for name in [
            "Favourites",
            "Best of 2026",
            "2026-10 trip",
            "Unnamed World",
        ] {
            assert!(valid_collection_name(name), "{name}");
        }
        for name in [
            "",
            ".",
            "..",
            ".hidden",
            "../x",
            "a/b",
            "a\\b",
            "a:b",
            "CON",
            " padded",
            "Prints",
            "prints",
            "vrchat-organizer-thumbnails",
            "2026-10",
        ] {
            assert!(!valid_collection_name(name), "{name}");
        }
    }
}
