# Changelog

## 2.3.1 - 2026-10-08

- **Fixed a memory runaway that crashed the app on Linux.** After scanning all
  month folders, the library page grew by hundreds of MB per second (20 GB+)
  until the app died. The cause was a blur effect on the favourite button of
  every world card, which WebKitGTK handles badly. The library now stays at
  about 400 MB with every month loaded.

## 2.3.0 - 2026-10-08

- **Lite edition:** a compact window and tray icon with watcher mode,
  Organize now, Undo and three quick settings. It uses the same engine as
  the full app, so results are identical. Closing the window keeps the
  watcher running from the tray without a browser view. Ships as
  `*-lite-x86_64.AppImage` and `*-windows-x86_64-lite-portable.exe`; any
  build starts as Lite with `--lite`.
  - Closing the window really frees memory: Lite restarts itself as a small
    tray-only process (about 60 MB, no browser view) once any move in
    progress has finished.
  - Opening Lite again shows the running one instead of starting a second
    watcher.
- **Safer moves:** with two organizers watching the same folder on a drive
  without hard links (FAT/exFAT, some network shares), a photo could be
  deleted. A move now never removes its copy when the original is already gone.
- Changing "Include older months" in the full app now restarts a running
  watcher with the new setting.

## 2.2.1 - 2026-10-08

- **Fixed drawing person boxes:** dragging on the photo dragged the image
  itself, so no box was drawn or saved. The box now follows the pointer while
  you draw, and existing boxes can be moved and resized in draw mode too.

## 2.2.0 - 2026-10-07

- **Removed avatar AI.** Recognition was unreliable (avatars change constantly);
  people are now tagged by drawing a box and picking a name from the
  screenshot's player list. Drawn boxes and their pictures are kept; old AI data
  is cleaned up on first launch. The AppImage shrinks from 522 MB to about 7 MB.
- **Reads VRChat's own metadata:** screenshots without VRCX data are now
  organized by the world name VRChat writes itself (XMP), into the same
  folder VRCX-tagged photos of that world use.
- **Fail-safe organizing:** moves never overwrite a file, anything that can't be
  organized is left untouched, and a move whose undo entry can't be written is
  rolled back. Undo never overwrites and keeps unreadable log lines.
- **Faster and lighter:** library scans ~20× faster, organizing 5,000 photos
  0.2 s instead of 4.7 s with ~1000× less disk writing; no full rescan per new
  screenshot; idle CPU reduced (process check, polling paused when hidden).
- **Security:** the screenshot folder can only be chosen from the native picker
  or auto-detect, and the app can only read that folder; content security
  policy enabled; crafted images can no longer exhaust memory; symlinks in
  shared folders can't redirect writes; world names can't collide with
  month/Prints/thumbnail folders.
- **UI:** consistent fonts, spacing and colours; themes now apply everywhere;
  more readable secondary text; keyboard focus and Escape work in all dialogs;
  in-app dialogs replace browser pop-ups; layout fixed at the minimum window
  size; notifications no longer hidden behind open panels.
- Fixed People profiles, tag search highlighting the wrong name, and several
  watcher/stop-start and undo edge cases.
- Release builds are smaller (LTO, stripped); release tooling is pinned and
  hash-verified. The Windows command-line build is now named `*-cli.exe`.

## 2.1.0 - 2026-09-20

- Added explicit image person tags separate from world participant metadata.
- Added subtle background library refreshes and detailed organization toasts.
- Started and stopped auto-watch with the VRChat process session.

## 2.0.2 - 2026-09-20

- Added a Current session Library tab showing screenshots captured since VRChat
  started.
- Automatically selects Current session when VRChat opens or is already running
  when Organizer starts.
- Kept session filtering metadata-only with the existing lazy thumbnail loading.

## 2.0.0 - 2026-09-20

The first public release of the Rust/Tauri rewrite.

- Added the Tauri desktop application with library scanning, organization preview,
  undo, watch mode, and activity history.
- Added the standalone Rust CLI for scripted and headless organization.
- Added PNG and JPEG metadata extraction for VRChat world information.
- Added Linux AppImage packaging and Windows build workflows.
- Bundled offline avatar-recognition model assets into the application and
  added explicit model-health reporting with graceful recognition disablement
  when assets are unavailable.
