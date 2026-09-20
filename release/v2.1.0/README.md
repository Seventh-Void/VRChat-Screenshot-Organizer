# VRChat Organizer 2.1.0

VRChat Organizer 2.1.0 adds explicit image-level person tagging, smoother
library updates, richer organization notifications, and automatic watcher
control tied to the VRChat process.

## Highlights

- **Explicit image tags** — Right-click an open PNG screenshot, search or
  select a player, and save the person as a visible-in-this-image tag.
- **Separate metadata sources** — Explicit image tags are kept separate from
  world participant metadata. People search and tagged-photo counts use only
  the people manually identified in images.
- **Persistent PNG metadata** — Tags are stored in an Organizer-owned PNG
  metadata chunk without re-encoding image pixels.
- **Subtle library refreshes** — Background refreshes keep existing content
  visible and use a short opacity transition instead of an abrupt loading
  replacement.
- **Detailed organization notifications** — Organization toasts identify the
  photo and destination folder, and remain visible for 10 seconds.
- **VRChat session auto-watch** — Watch mode starts when VRChat opens and stops
  when VRChat closes. If VRChat is already running at startup, watch mode
  starts during initial status synchronization.
- **Full release rebuilds** — The release build path rebuilds both the Linux
  AppImage and Windows CLI executable, then verifies both outputs and writes
  their checksums.

## Downloads

| File | Platform | SHA-256 |
|---|---|---|
| `VRChatOrganizer-2.1.0-x86_64.AppImage` | Linux x86_64 GUI | `aedc5a3431a59a79e09706e9aa296e10dab2594ab90246b761c8f920e0d273aa` |
| `VRChatOrganizer-2.1.0-windows-x86_64.exe` | Windows x86_64 CLI | `89e3a4bed862b15d9742746a6f6bd6023a6ad004eaa8000f3f200ed1192b4989` |

The Windows executable in this release directory is the standalone CLI build.
The full Tauri GUI executable and NSIS/MSI installers require a native Windows
or Windows CI build because they use the Windows WebView2 toolchain.

## Verification

- `organizer-core` tests: 33 passed, 0 failed.
- Tauri application release build completed successfully.
- Windows x86_64 CLI release build completed successfully.
- AppImage packaging completed successfully.
- Release artifacts were verified as a Linux AppImage and PE32+ Windows
  executable.

## Linux installation

```bash
chmod +x VRChatOrganizer-2.1.0-x86_64.AppImage
./VRChatOrganizer-2.1.0-x86_64.AppImage
```

Before organizing files, use the application’s preview flow or CLI dry-run
mode and keep a backup of the VRChat screenshots folder.
