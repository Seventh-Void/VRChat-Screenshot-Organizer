# Gotchas

Traps found hard way. Add new at end of right section; keep each few lines with *why*.

## Files, moves, paths
- `fs::rename` **silently replaces** existing destination (Windows and Unix). Checking `exists()` first = race. Use no-clobber `move_file` (hard_link → remove src, or copy-to-temp → verify → no-clobber final step).
- Cross-device rename error code differs: Linux EXDEV = **18**, Windows ERROR_NOT_SAME_DEVICE = **17**. `move_file` sidesteps: any `hard_link` error other than AlreadyExists (cross-drive, FAT/exFAT) falls back to verified copy path.
- `move_file` order matters: move → append undo line; append fails → move file **back** (un-undoable move worse than none). On failure remove just-created empty world/month dir.
- `remove_source` keeps `dst` when `src` is already gone (NotFound): with Full + Lite both watching, the other process moved it, and on the FAT/exFAT copy path `dst` may be the only copy.
- Leftover copy temps cleaned next run — **only** names matching `is_copy_temp_name` (`.<image>.copying.<pid>.<n>`). Looser match deleted user files like `.notes.copying.txt`.
- `fs::canonicalize` returns `\\?\C:\…` on Windows and resolves symlinks (Steam/Proton paths often symlinked). Canonical paths **only for containment checks / asset scope**, never cache, thumbnail or tag keys — changing key paths forces full rescan + thumbnail rebuild.
- Undo log entries stored with canonical paths → containment checks during undo must canonicalize `base_path` too.
- `Path::starts_with` compares components, doesn't resolve `..` — canonicalize before using as security check. Lexical checks miss symlinked dirs inside library → use `inside_without_symlinks` (every component below base must be real dir).
- Shared/synced library folders are attacker-reachable: refuse symlinks for undo/tag/activity files and `vrchat-organizer-thumbnails` (`refuse_symlink`); write via `write_atomic` (`create_new` temp + unique suffix). `fs::copy` follows a symlink planted at destination — use `OpenOptions::create_new` + `io::copy`, then set permissions.
- `WalkDir` follows symlinked root by default (`follow_root_links`). Allow only for user's chosen library folder (often symlink on Steam/Proton), never month subfolders; pick month dirs by `entry.file_type()`, not `path.is_dir()`.
- World names from metadata are untrusted: `sanitize_name` suffixes `_` to month-shaped (`YYYY-MM`), `Prints`, `vrchat-organizer-thumbnails`. Month-shaped world folder made file sink one level deeper every scan.
- VRChat PNGs put iTXt metadata **before** IDAT; IDAT comes in many 8 KiB chunks — walking every chunk header past IDAT reads whole file. Stop at first IDAT once text found; keep seeking only when no text seen yet.
- Two world-name sources: VRCX `Description` JSON (has players) and VRChat's own XMP iTXt `XML:com.adobe.xmp` → `vrc:WorldDisplayName` (no players; some photos have only this). XMP swaps unsafe punctuation for lookalikes (`|`→`｜`, `.`→`․`, `:`→`˸`, `/`→`⁄`, `,`→`‚`, `!`→`ǃ`, fullwidth `#()[]`) — `parse_vrchat_xmp` maps back, else same world splits into two folders. VRCX wins; XMP fills when VRCX missing/empty. Lightroom rewrites XMP as attributes. Verified 719/719 match on real library 2026-10-08.
- PNG text DoS: per-chunk inflate cap isn't enough (5,000 tiny zTXt bombs = GBs). Only inflate known keywords; stop at 64 text chunks / 4 MiB total. Cap participants 256, names 80 chars, world name 100. Image decode limits 16384² / 256 MiB (`decode_image`).

## Data compatibility (existing users' files)
- Tag store `<base>/.vrchat-organizer-tags.json` keyed by `normalized_png_fingerprint` (SHA-256 of PNG minus organizer tag chunk). Any change to hashed bytes **wipes every user's tags** — keep output byte-identical, test old vs new (golden hashes in tests).
- Undo log `<base>/.vrchat-organizer-undo.json` was JSON array; now JSON Lines. Readers accept both; convert legacy array before appending (watcher appends without starting new run). Corrupt/torn lines kept verbatim on rewrite, never dropped.
- `library.sqlite` cache: latest-month scan must not delete rows for other months; limit stale deletes to roots just scanned.
- People DB stays named `avatar-recognition.sqlite` (compat). `user_version` 2 migration dropped AI tables, kept `avatar_people` + `positional_person_tags` and `avatar-crops/manual-*.jpg`. `PRAGMA user_version` inside `execute_batch` in a transaction rolls back with it → migration all-or-nothing.
- Orphan-thumbnail cleanup: skip if scan found zero photos (unmounted drive would wipe cache); delete only thumbs older than scan start.

## Avatar AI (removed 2026-10-07)
- Removed at user request: unreliable (VRChat avatars change constantly; who-was-there already in photo player metadata). Don't re-add without asking. Old `resources/ai/*.onnx` are untracked leftovers — unreferenced.
- Why it was "buggy": YOLOX export output was grid-relative and never decoded → every box few pixels at top-left. If AI ever returns: decode strides 8/16/32, RGB + ImageNet norm, person class only; SigLIP norm 0.5/0.5; never `include_bytes!` models (binaries hit 215 MB each); `ort` needs Rust ≥ 1.81 and ships `libonnxruntime.so.1` dynamically.

## Tauri 2
- Windows created in `setup` (`open_main_window`), not `tauri.conf.json` (`windows: []`). Label must stay `main` — `capabilities/default.json` grants by label.
- Library root lives in memory only after `restore_saved_root` (normally via the page's `get_library_root`). Start-in-tray Lite has no page, so `setup_lite` restores it itself — else watcher and tray actions silently fail.
- Tray "Watcher mode" mirrors the watcher's real state (`sync_tray_watch` in begin/end/fail), not the saved setting. Clone the menu item out of the mutex before `set_checked`: it waits on the main thread.
- WebKitGTK never gives a closed webview's memory back (web process + libs stay for process life: 58 MB tray-only → ~190 MB + ~260 MB WebKit after one window open). Lite close therefore hands over: hide window, `end_watching`, hold `hold_organization_lock()` (forgotten, so no move cut off), spawn `$APPIMAGE` or current exe with `--lite --background --after-pid <pid>`, `app.exit(0)`. New process waits for old pid before single-instance init. Inside AppImage spawn `$APPIMAGE`, not `current_exe` (mount dies with old process).
- Lite stays alive windowless only while tray exists (`RunEvent::ExitRequested` with `code: None` prevented). No tray → close = exit, else invisible process. Tray build wrapped in `catch_unwind`: Linux tray lib loaded at runtime, may panic when missing.
- Design tokens live in `frontend/tokens.css`, linked by `index.html` and `lite.html`. Add tokens there, not in a page.
- Linux bundle resource dir is `../lib/<productName>` ("VRChat Organizer"); resources declared as `../resources/...` land under `_up_/`. Resolve with `app.path().resource_dir()`, don't guess from exe path.
- CSP: Tauri adds nonces to `style-src` → browsers ignore `'unsafe-inline'` → JS-written `style=""` attributes break. Use `"dangerousDisableAssetCspModification": ["style-src"]`. Also need `connect-src ipc: http://ipc.localhost` or IPC falls back to postMessage. Inline `<script>` hashed automatically. `base-uri`/`form-action` don't fall back to `default-src` — set explicitly.
- Asset protocol scope `$HOME/**` exposes whole home dir (incl. dotfiles on Windows). Keep static scope minimal; add library folder at runtime with `asset_protocol_scope().allow_directory`. Can't safely revoke (forbid beats allow) — restart clears.
- Library root is trusted only from Rust: `pick_library_folder` (Rust dialog), `use_detected_folder`, one-time `adopt_legacy_folder` (only when none persisted). Persisted in app data `library-root.txt`. Commands take **no** `folder_path` — webview-supplied paths let injected script widen scope to `/`. Boot calls `get_library_root` first; test stubs must return a path or page boots into folder-needed state.
- Sync `#[tauri::command] fn` runs on main thread — disk/SQLite work should be `async` / `spawn_blocking`.
- `arboard::Clipboard` dropped right after `set_text` loses clipboard on Linux — keep alive in managed state.
- `let g = app.state::<T>().0.lock()` fails with E0716 (temporary `State` dropped while borrowed). Use `app.state::<T>().inner().0.lock()` — `inner()` borrows from AppHandle.
- `explorer /select,` rejects forward-slash paths — convert `/` → `\` on Windows. `xdg-open` arg starting with `-` parsed as option — pass canonical path.
- Collection named like `YYYY-MM` treated as month folder by organizer, copies re-organized into duplicates — collection names reject that pattern.

## Concurrency
- One global cancel flag shared by organize, library scan, watcher. Only operation that checks it should clear it; library scans don't use it.
- Quick watcher stop→start used to leave two watcher threads (flag-based). Use generation counter. Old thread's initial `organize_path` still runs to completion after stop (no per-run cancel).
- Tag store read-modify-write needs lock; public fns call each other → lock in wrappers around unlocked inner fn (std `Mutex` not re-entrant).

## Frontend
- `applyTheme` only updates CSS variables it sets; hard-coded accent `rgba(...)` stays violet in other themes — use accent tint tokens.
- Inter not bundled (no external font loads by design) → weight 650 renders as 600/700 — stick to 400/600/700.
- `color-mix()` needs WebKitGTK ≥ 2.40 — set tint variables from JS instead.
- Minimum window 860×620: `max-width: 600px` rules never apply; test layouts at 860×620.
- Library toolbar (tabs + world filters + resolution chips) barely fits one row at 1180 px — bigger chip font/padding wraps it. Re-check after touching chip/tab sizes.
- Testing real inline script in jsdom: inline `tagging-state.js`, stub `IntersectionObserver` in `beforeParse`, read top-level `let` via `window.eval('name')` (only `function` declarations become `window` props), `window.close()` at end or app's `setInterval` keeps node alive.
- `openViewer` renders twice (`loadPositionalTags` re-renders after `await`) — layout tests wait a macrotask and for wrapper width in `px` before measuring.
- Toggle track/knob and scrollbar use `--track`/`--knob`, not `--muted`/`--faint`, so text-contrast change doesn't recolour controls.
- Weight 600 on Windows (Segoe UI Semibold) visibly lighter than 700; on Linux (Noto Sans, no semibold) both render bold.
- Overlays = native `<dialog>` + `showModal()`. Everything outside open modal inert — toast is `popover="manual"` re-shown via `raiseToast()` so it paints above dialogs (hit-testing still skips it; test with `:popover-open`/screenshots).
- Class setting `display` on `<dialog>` overrides UA hidden state — keep `dialog:not([open]) { display: none !important }`.
- Escape on modal fires `cancel`, closes natively — app state (e.g. `viewerState`) needs `cancel` listener routing to app close function.
- jsdom 24 has `HTMLDialogElement` but no `showModal()`/`close()` — stub in tests.
- `<img>` is natively draggable: pointer-drag on it fires `dragstart` → `pointercancel`, no `pointerup` (box drawing silently died, ghost image followed cursor). `#lightboxImage` has `draggable="false"`; keep it. App captures `__TAURI__.core.invoke` at boot — tests swap behaviour via `window.__invokeOverride`, not by replacing `invoke`.
- `prompt()`/`confirm()` gone; use in-app `ask({title, message, value, confirmLabel})` dialog, re-check after `await` that viewer still shows same photo.

## Shell scripts / CI
- `cmd | grep -q` under `set -o pipefail` can fail spuriously (grep exits early → writer gets SIGPIPE). Capture output in variable first.
- `find -maxdepth` applies globally even inside `-o` group — once silently dropped files from SHA256SUMS. Use bash globs with `nullglob`.
- `sha256sum file` prefixes line with `\` when path contains backslash; hash via `sha256sum < file` when comparing.
- Release downloads (appimagetool etc.) pinned to numbered versions + SHA-256; `curl --proto '=https' --proto-redir '=https'`; actions pinned to commit SHAs; `cargo install ... --locked`.
- Pinning `dtolnay/rust-toolchain` to a SHA loses toolchain-from-ref → must pass `with: toolchain: stable`.
- `svenstaro/upload-release-action` treats `*` literally unless `file_glob: true` — 2.0–2.1 Windows uploads failed ENOENT on `VRChatOrganizer-*-….exe`.
- GitHub runners lack FUSE → `appimagetool` dies (`libfuse.so.2`). Set `APPIMAGE_EXTRACT_AND_RUN=1`.
- Release workflow checks out the **tag**; version read from that commit's Cargo.toml. Tag must point at version-bump commit (V2.1 tag built as 2.0.2). Workflow re-uploads AppImage/CLI with `overwrite: true` → regenerate SHA256SUMS from final release assets afterwards.
- First `get_library` on empty cache returns empty stats; real data arrives via `library-updated` event — tests poll/listen.
- Don't launch GUI test windows or take active-window screenshots while user may be gaming (VRChat) — steals focus, captures their game. Use Playwright harness instead.

## Tooling / environment
- GateGuard ("Fact-Forcing Gate") hook (ecc plugin, disabled 2026-10-07 by /doctor) blocked first Bash call and first Edit/Write per file until facts stated. If ecc re-enabled: answer briefly, retry, or `ECC_GATEGUARD=off`.
- `/usr/bin/time` not installed on dev machine — read peak memory from `VmHWM` in `/proc/self/status`. Perf harness: `cargo test --release -p organizer-core perf_harness -- --ignored --nocapture`.
- Windows release `.exe` from `build-windows.sh` is **CLI** cross-compiled from Linux; GUI/installers come from `release.yml` (`cargo tauri build`).
