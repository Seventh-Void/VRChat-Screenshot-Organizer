use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use organizer_core::{
    cached_library, clear_cancel_request, extract_image_meta, generate_thumbnail, organize_path,
    organize_single_file, read_activity_log, request_cancel, scan_library_with_cache,
    set_photo_tags, tag_photo_participant_in_store, undo_organization, validate_template,
    ActivityEntry, OrganizerConfig, OrganizerStats,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use sysinfo::{ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State};

/// Managed state holding the optional file watcher and a running flag.
pub struct WatcherState {
    pub running: AtomicBool,
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
    folder_path: String,
    dry_run: bool,
    scan_all_months: bool,
    single_folder: bool,
    template: String,
) -> Result<OrganizerStats, String> {
    clear_cancel_request();
    tauri::async_runtime::spawn_blocking(move || {
        organize_folder_blocking(
            folder_path,
            dry_run,
            scan_all_months,
            single_folder,
            template,
        )
    })
    .await
    .map_err(|err| format!("scan task failed: {err}"))?
}

#[tauri::command]
fn cancel_scan() -> Result<String, String> {
    request_cancel();
    Ok("cancelling".to_string())
}

#[tauri::command]
async fn simulate_folder(
    folder_path: String,
    scan_all_months: bool,
    single_folder: bool,
    template: String,
) -> Result<OrganizerStats, String> {
    organize_folder(folder_path, true, scan_all_months, single_folder, template).await
}

#[tauri::command]
fn get_activity(folder_path: String) -> Result<Vec<ActivityEntry>, String> {
    let expanded = shellexpand::tilde(&folder_path).to_string();
    read_activity_log(&PathBuf::from(expanded)).map_err(|err| err.to_string())
}

#[tauri::command]
async fn get_library(
    app: AppHandle,
    folder_path: String,
    scan_all_months: bool,
) -> Result<OrganizerStats, String> {
    let expanded = shellexpand::tilde(&folder_path)
        .to_string()
        .replace('\\', "/");
    let cache_path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("could not locate app data directory: {error}"))?
        .join("library.sqlite");
    let base_path = PathBuf::from(&expanded);
    if let Some(snapshot) =
        cached_library(&cache_path, &base_path, scan_all_months).map_err(|err| err.to_string())?
    {
        let background_app = app.clone();
        let background_path = base_path;
        let background_cache = cache_path.clone();
        tauri::async_runtime::spawn_blocking(move || {
            match scan_library_with_cache(
                &background_path,
                scan_all_months,
                Some(&background_cache),
            ) {
                Ok(stats) => {
                    let _ = background_app.emit("library-updated", stats);
                }

                Err(error) => {
                    let _ = background_app.emit(
                        "library-error",
                        serde_json::json!({ "error": error.to_string() }),
                    );
                }
            }
        });
        return Ok(snapshot);
    }
    let background_app = app.clone();
    let background_path = base_path;
    let background_cache = cache_path;
    tauri::async_runtime::spawn_blocking(move || {
        match scan_library_with_cache(&background_path, scan_all_months, Some(&background_cache)) {
            Ok(stats) => {
                let _ = background_app.emit("library-updated", stats);
            }
            Err(error) => {
                let _ = background_app.emit(
                    "library-error",
                    serde_json::json!({ "error": error.to_string() }),
                );
            }
        }
    });
    Ok(OrganizerStats::default())
}

#[tauri::command]
async fn get_thumbnail(folder_path: String, photo_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base_path = PathBuf::from(shellexpand::tilde(&folder_path).to_string());
        let photo_path = PathBuf::from(shellexpand::tilde(&photo_path).to_string());
        if !photo_path.is_file() || !photo_path.starts_with(&base_path) {
            return Err(format!(
                "photo is outside the selected library: {}",
                photo_path.display()
            ));
        }

        generate_thumbnail(&photo_path, &base_path)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|error| format!("thumbnail generation failed: {error}"))
    })
    .await
    .map_err(|error| format!("thumbnail task failed: {error}"))?
}

#[tauri::command]
async fn tag_photo_participant(
    folder_path: String,
    photo_path: String,
    participant: String,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base_path = std::fs::canonicalize(shellexpand::tilde(&folder_path).to_string())
            .map_err(|error| format!("screenshot folder is not accessible: {error}"))?;
        let photo_path = std::fs::canonicalize(shellexpand::tilde(&photo_path).to_string())
            .map_err(|error| format!("photo is not accessible: {error}"))?;
        if !photo_path.starts_with(&base_path) || !photo_path.is_file() {
            return Err("photo is outside the selected screenshot folder".to_string());
        }

        tag_photo_participant_in_store(&base_path, &photo_path, &participant)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("tagging task failed: {error}"))?
}

#[tauri::command]
async fn set_photo_participant_tags(
    folder_path: String,
    photo_path: String,
    participants: Vec<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base_path = std::fs::canonicalize(shellexpand::tilde(&folder_path).to_string())
            .map_err(|error| format!("screenshot folder is not accessible: {error}"))?;
        let photo_path = std::fs::canonicalize(shellexpand::tilde(&photo_path).to_string())
            .map_err(|error| format!("photo is not accessible: {error}"))?;
        if !photo_path.starts_with(&base_path) || !photo_path.is_file() {
            return Err("photo is outside the selected screenshot folder".to_string());
        }
        set_photo_tags(&base_path, &photo_path, &participants).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("tag editing task failed: {error}"))?
}

#[tauri::command]
async fn undo_last_run(folder_path: String) -> Result<OrganizerStats, String> {
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

/// Auto-detect the VRChat screenshot folder across platforms.
#[tauri::command]
fn get_default_path() -> String {
    // Linux: check common paths
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());

    let candidates = if cfg!(target_os = "windows") {
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
    };

    // Return the first path that actually exists
    for path in &candidates {
        let expanded = shellexpand::tilde(path).to_string();
        if std::path::Path::new(&expanded).exists() {
            return path.clone();
        }
    }

    // Fallback to default
    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| format!("{home}/Pictures/VRChat/VRChat"))
}

#[tauri::command]
fn open_photo_location(path: String) -> Result<(), String> {
    let photo = PathBuf::from(&path);
    if !photo.is_file() {
        return Err(format!("photo does not exist: {}", photo.display()));
    }

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe")
        .args(["/select,", &path])
        .spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open")
        .arg(photo.parent().unwrap_or(&photo))
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg(photo.parent().unwrap_or(&photo))
        .spawn();
    result
        .map(|_| ())
        .map_err(|error| format!("could not open photo location: {error}"))
}

#[tauri::command]
fn copy_photo_path(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_file() {
        return Err(format!("photo does not exist: {path}"));
    }
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("could not access clipboard: {error}"))?;
    clipboard
        .set_text(path)
        .map_err(|error| format!("could not copy photo path: {error}"))
}

#[tauri::command]
async fn add_photos_to_collection(
    folder_path: String,
    collection_name: String,
    photo_paths: Vec<String>,
) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let expanded_folder = shellexpand::tilde(&folder_path).to_string();
        let base_path = std::fs::canonicalize(&expanded_folder)
            .map_err(|error| format!("screenshot folder is not accessible: {error}"))?;
        let collection_name = collection_name.trim();
        if collection_name.is_empty()
            || collection_name == "."
            || collection_name == ".."
            || collection_name.contains('/')
            || collection_name.contains('\\')
        {
            return Err("collection name contains invalid path characters".to_string());
        }

        let destination = base_path.join(collection_name);
        std::fs::create_dir_all(&destination)
            .map_err(|error| format!("could not create collection folder: {error}"))?;
        let mut copied = 0;
        for source_string in photo_paths {
            let source = std::fs::canonicalize(&source_string)
                .map_err(|error| format!("photo is not accessible: {source_string}: {error}"))?;
            if !source.starts_with(&base_path) || !source.is_file() {
                return Err(format!(
                    "photo is outside the selected screenshot folder: {source_string}"
                ));
            }
            let file_name = source
                .file_name()
                .ok_or_else(|| format!("photo has no file name: {source_string}"))?;
            let mut target = destination.join(file_name);
            if target.exists() {
                let stem = target
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("photo")
                    .to_string();
                let extension = target
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("")
                    .to_string();
                let mut suffix = 2;
                while target.exists() {
                    let name = if extension.is_empty() {
                        format!("{stem} ({suffix})")
                    } else {
                        format!("{stem} ({suffix}).{extension}")
                    };
                    target = destination.join(name);
                    suffix += 1;
                }
            }
            std::fs::copy(&source, &target)
                .map_err(|error| format!("could not copy {}: {error}", source.display()))?;
            copied += 1;
        }
        Ok(copied)
    })
    .await
    .map_err(|error| format!("collection copy task failed: {error}"))?
}

#[tauri::command]
fn validate_folder(folder_path: String) -> Result<String, String> {
    let expanded = shellexpand::tilde(&folder_path).to_string();
    let path = std::path::Path::new(&expanded);
    if !path.is_dir() {
        return Err(format!("folder does not exist: {expanded}"));
    }
    std::fs::read_dir(path)
        .map(|_| folder_path)
        .map_err(|error| format!("folder is not accessible: {error}"))
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
        system.refresh_processes(ProcessesToUpdate::All, true);
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
#[tauri::command]
fn has_undo(folder_path: String) -> bool {
    let expanded = shellexpand::tilde(&folder_path).to_string();
    let undo_path = std::path::Path::new(&expanded).join(".vrchat-organizer-undo.json");
    undo_path.exists()
}

fn collect_watch_paths(event: &Event, pending: &mut std::collections::HashSet<PathBuf>) {
    if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
        return;
    }

    for path in &event.paths {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        if !matches!(extension.as_deref(), Some("png" | "jpg" | "jpeg" | "webp"))
            || path.components().any(|component| {
                let name = component.as_os_str().to_string_lossy();
                name == "Prints" || name == "vrchat-organizer-thumbnails" || name.starts_with('.')
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
    folder_path: String,
    dry_run: bool,
    scan_all_months: bool,
    single_folder: bool,
    template: String,
    state: State<'_, WatcherState>,
) -> Result<String, String> {
    if state.running.load(Ordering::SeqCst) {
        return Err("already watching".to_string());
    }

    let expanded = shellexpand::tilde(&folder_path).to_string();
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
    state.running.store(true, Ordering::SeqCst);
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
        let (tx, rx) = std::sync::mpsc::channel::<Result<Event, notify::Error>>();

        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => {
                let _ = app_handle.emit(
                    "watch-error",
                    serde_json::json!({"error": format!("Failed to create watcher: {}", e)}),
                );
                let state: State<WatcherState> = app_handle.state();
                state.running.store(false, Ordering::SeqCst);
                let _ = app_handle.emit("watch-status", serde_json::json!({"status": "stopped"}));
                return;
            }
        };

        if let Err(e) = watcher.watch(&watch_path_clone, RecursiveMode::Recursive) {
            let _ = app_handle.emit(
                "watch-error",
                serde_json::json!({"error": format!("Failed to watch: {}", e)}),
            );
            let state: State<WatcherState> = app_handle.state();
            state.running.store(false, Ordering::SeqCst);
            let _ = app_handle.emit("watch-status", serde_json::json!({"status": "stopped"}));
            return;
        }

        // Finish the initial organization before consuming notifications. This
        // prevents the watcher's own moves and metadata writes from becoming a
        // second organization pass.
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
        loop {
            let state: State<WatcherState> = app_handle.state();
            if !state.running.load(Ordering::SeqCst) {
                break;
            }

            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    let mut pending = std::collections::HashSet::new();
                    collect_watch_paths(&event, &mut pending);
                    let deadline = Instant::now() + Duration::from_millis(700);
                    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                        match rx.recv_timeout(remaining) {
                            Ok(Ok(next)) => collect_watch_paths(&next, &mut pending),
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
                        processed.insert(path, fingerprint);
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
    state.running.store(false, Ordering::SeqCst);

    let _ = app.emit("watch-status", serde_json::json!({"status": "stopped"}));

    Ok("stopped".to_string())
}

/// Check if watcher is running.
#[tauri::command]
fn is_watching(state: State<'_, WatcherState>) -> bool {
    state.running.load(Ordering::SeqCst)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(WatcherState {
            running: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            organize_folder,
            cancel_scan,
            simulate_folder,
            get_activity,
            get_library,
            get_thumbnail,
            tag_photo_participant,
            set_photo_participant_tags,
            get_default_path,
            open_photo_location,
            copy_photo_path,
            add_photos_to_collection,
            validate_folder,
            undo_last_run,
            has_undo,
            start_watching,
            stop_watching,
            is_watching,
            get_vrchat_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
