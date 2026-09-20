# VRChat Organizer 2.0.2

VRChat Organizer 2.0.2 improves library startup, makes People search
search-first, and adds a focused view for screenshots from the active VRChat
session.

## Highlights

- **Current session tab** — View only screenshots captured since the current
  VRChat process started.
- **Automatic session switching** — Organizer selects Current session when
  VRChat opens, including when VRChat was already running at startup.
- **Faster library startup** — Cached metadata appears immediately while
  background scanning checks only new or changed files.
- **Persistent metadata cache** — SQLite stores photo metadata, file state, and
  thumbnail paths between launches.
- **Lazy thumbnails** — Thumbnails are generated and loaded only as content
  approaches the viewport, using a bounded background queue.
- **Faster People search** — Suggestions use cached player metadata, and
  confirmed results render in small batches instead of creating every card at
  once.
- **Image participant tags** — Right-click an open PNG screenshot, search or
  select a player name, and save an explicit in-frame tag directly into the
  image metadata. World participant metadata remains separate.
- **PNG metadata efficiency** — PNG metadata reads chunk headers and text
  chunks without decoding pixel data during library scanning.
- **Release cleanup** — Removed generated build directories and development
  timing/debug instrumentation from the shipped source.

## Downloads

| File | Platform | SHA-256 |
|---|---|---|
| `VRChatOrganizer-2.0.2-x86_64.AppImage` | Linux x86_64 GUI | `4e2a35a355fb2cc3aa69b155386d69b62656472aaf0529a18982982ce273ee8c` |
| `VRChatOrganizer-2.0.2-windows-x86_64.exe` | Windows x86_64 CLI | `a5d2845f42ce61dbb292973ee7c6a358db2287f873c69524a157349dbdcdc152` |

The Windows executable in this release directory is the standalone CLI build.
The full Tauri GUI executable and NSIS/MSI installers require a native Windows
or Windows CI build because they use the Windows WebView2 toolchain.

## Verification

- `organizer-core` release tests: 32 passed, 0 failed.
- Frontend JavaScript syntax validation passed.
- Rust formatting validation passed.
- AppImage smoke test launched successfully without panic or error output.
- Release artifacts were verified as ELF AppImage and PE32+ Windows executable.

## Linux installation

```bash
chmod +x VRChatOrganizer-2.0.2-x86_64.AppImage
./VRChatOrganizer-2.0.2-x86_64.AppImage
```

Before organizing files, use the application’s preview flow or CLI dry-run
mode and keep a backup of the VRChat screenshots folder.
