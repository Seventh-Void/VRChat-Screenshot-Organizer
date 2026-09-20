# Changelog

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
