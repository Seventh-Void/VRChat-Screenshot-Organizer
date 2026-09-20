use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use organizer_core::{
    organize_path, organize_single_file, read_activity_log, undo_organization, validate_template,
    ActivityEntry, OrganizerConfig, OrganizerStats,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use sysinfo::{ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State};

/// Managed state holding the optional file watcher and a running flag.
pub struct WatcherState {
    pub watcher: Mutex<Option<RecommendedWatcher>>,
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
        vec![format!(
            "{}/Pictures/VRChat/VRChat",
            userprofile.replace('\\', "/")
        )]
    } else {
        // Linux / Steam Proton paths
        vec![
      format!("{}/Pictures/VRChat/VRChat", home),
      format!("{}/.local/share/Steam/steamapps/compatdata/438100/pfx/drive_c/users/steamuser/Pictures/VRChat/VRChat", home),
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

/// Start watching the folder for new files and auto-organize them.
/// Uses both filesystem events (recursive) + periodic polling every 5 seconds
/// to handle Steam Proton/Wine where inotify events might not fire reliably.
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
    // Prevent double start
    if state.running.load(Ordering::SeqCst) {
        return Err("already watching".to_string());
    }

    let expanded = shellexpand::tilde(&folder_path).to_string();
    let base_path = PathBuf::from(expanded.clone());

    if !base_path.exists() {
        return Err(format!("path does not exist: {}", expanded));
    }

    // Build the config
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

    // Always watch the base path recursively so all YYYY-MM subfolders are covered
    let watch_path = base_path.clone();

    // Run an initial scan to organize any existing unorganized files first
    let initial_app_handle = app.clone();
    let initial_config = config.clone();
    std::thread::spawn(move || {
        let mut stats = OrganizerStats::default();
        match organize_path(&initial_config, &mut stats) {
            Ok(()) => {
                let _ = initial_app_handle.emit(
                    "watch-initial-scan",
                    serde_json::json!({
                      "stats": stats
                    }),
                );
            }
            Err(e) => {
                let _ = initial_app_handle.emit(
                    "watch-initial-scan",
                    serde_json::json!({
                      "error": e.to_string()
                    }),
                );
            }
        }
    });

    // Notify frontend
    let _ = app.emit(
        "watch-status",
        serde_json::json!({
          "status": "started",
          "path": watch_path.to_string_lossy()
        }),
    );

    state.running.store(true, Ordering::SeqCst);

    // Create the watcher in a spawned blocking task
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
                return;
            }
        };

        // Watch recursively so files in YYYY-MM subfolders are detected
        if let Err(e) = watcher.watch(&watch_path_clone, RecursiveMode::Recursive) {
            let _ = app_handle.emit(
                "watch-error",
                serde_json::json!({"error": format!("Failed to watch: {}", e)}),
            );
            let state: State<WatcherState> = app_handle.state();
            state.running.store(false, Ordering::SeqCst);
            return;
        }

        // Store watcher in state so it can be dropped later
        let state: State<WatcherState> = app_handle.state();
        *state.watcher.lock().unwrap() = Some(watcher);

        // Track already-known files to avoid re-processing from events
        let mut known_files: HashSet<PathBuf> = HashSet::new();
        for entry in walkdir::WalkDir::new(&watch_path_clone)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            known_files.insert(entry.into_path());
        }

        let mut poll_counter: u32 = 0;

        // Event + Polling loop
        loop {
            // Check if we should stop
            {
                let state: State<WatcherState> = app_handle.state();
                if !state.running.load(Ordering::SeqCst) {
                    break;
                }
            }

            // Keep a slow fallback poll for filesystems that do not reliably emit
            // events (notably some Proton/Wine setups). Normal changes are handled
            // immediately by the watcher.
            poll_counter += 1;
            let should_poll = poll_counter >= 60;

            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    // Filter for file creation/modification events
                    let is_relevant =
                        matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_));
                    if !is_relevant {
                        continue;
                    }

                    // Process only image files
                    for path in &event.paths {
                        let ext = path
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(str::to_ascii_lowercase)
                            .unwrap_or_default();
                        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                            continue;
                        }
                        // Skip if we already know about this file
                        if known_files.contains(path) {
                            continue;
                        }
                        known_files.insert(path.clone());

                        // Small delay to let the file finish writing
                        std::thread::sleep(Duration::from_millis(500));

                        // Run organize on this single file, preserving its YYYY-MM parent folder
                        let mut stats = OrganizerStats::default();
                        if let Err(e) = organize_single_file(path, &config_clone, &mut stats) {
                            let _ = app_handle.emit(
                                "watch-error",
                                serde_json::json!({
                                  "error": e.to_string(),
                                  "file": path.to_string_lossy()
                                }),
                            );
                        } else {
                            let _ = app_handle.emit(
                                "watch-organized",
                                serde_json::json!({
                                  "stats": stats,
                                  "file": path.to_string_lossy()
                                }),
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    let _ =
                        app_handle.emit("watch-error", serde_json::json!({"error": e.to_string()}));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Normal timeout — proceed to check polling
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }

            // Periodic fallback poll: every ~30 seconds, scan for new files
            // that might have been missed without adding another full scan to
            // the normal watcher path.
            if should_poll {
                poll_counter = 0;
                let mut new_files_found = false;

                for entry in walkdir::WalkDir::new(&watch_path_clone)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().is_file())
                {
                    let path = entry.into_path();
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(str::to_ascii_lowercase)
                        .unwrap_or_default();
                    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                        continue;
                    }
                    if known_files.insert(path.clone()) {
                        // New file found via polling!
                        new_files_found = true;

                        // Run organize on this single file, preserving its YYYY-MM parent folder
                        let mut stats = OrganizerStats::default();
                        if let Err(e) = organize_single_file(&path, &config_clone, &mut stats) {
                            let _ = app_handle.emit(
                                "watch-error",
                                serde_json::json!({
                                  "error": e.to_string(),
                                  "file": path.to_string_lossy()
                                }),
                            );
                        } else {
                            let _ = app_handle.emit(
                                "watch-organized",
                                serde_json::json!({
                                  "stats": stats,
                                  "file": path.to_string_lossy()
                                }),
                            );
                        }
                    }
                }

                // Even if no new files were found, emit a status heartbeat so frontend knows
                // the watcher is alive. Only do this if no new files were processed.
                if !new_files_found {
                    let _ = app_handle.emit(
                        "watch-status",
                        serde_json::json!({
                          "status": "heartbeat",
                          "path": watch_path_clone.to_string_lossy()
                        }),
                    );
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

    // Drop the watcher to release file handles
    if let Some(watcher) = state.watcher.lock().unwrap().take() {
        drop(watcher);
    }

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
            watcher: Mutex::new(None),
            running: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            organize_folder,
            simulate_folder,
            get_activity,
            get_default_path,
            validate_folder,
            undo_last_run,
            has_undo,
            start_watching,
            stop_watching,
            is_watching,
            get_vrchat_status,
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
