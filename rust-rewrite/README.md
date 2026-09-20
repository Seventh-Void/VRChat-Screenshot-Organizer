# VRChat Organizer — Rust Rewrite

This directory contains the Rust-based implementation of the VRChat Screenshot Organizer. It is a pure-Rust replacement for the original Python version — **no Python or Pillow dependencies required**.

## Architecture

The project is a Cargo workspace with three crates:

| Crate | Description |
|---|---|
| **`organizer-core`** | Core library: metadata extraction (EXIF/PNG text chunks), file organization logic |
| **`desktop`** | CLI binary — simple command-line interface |
| **`src-tauri`** | Tauri v2 desktop application (GUI shell) |

## How Metadata Extraction Works

Metadata (world name, software) is extracted **without any external dependencies**:

- **JPEG files** — Uses `kamadak-exif` to read EXIF tags:
  - Tag 270 (ImageDescription) — JSON containing `{"world": {"name": "..."}}`
  - Tag 305 (Software) — Creator software name
- **PNG files** — Manually parses PNG text chunks at the byte level:
  - `tEXt`, `zTXt` (zlib-compressed), `iTXt` (UTF-8) chunks
  - Keywords: `Description`, `Comment` — JSON containing world metadata
  - Keywords: `Software`, `Creator Tool` — software name
  - `VRChat Organizer Participants` — names added from the image viewer
- **WebP** — Dimensions only (no EXIF/text chunk metadata supported yet)

When a screenshot is open in the viewer, right-click the image to search for a
player and save an explicit in-frame tag. World metadata participants remain
available as general capture metadata, while the People view and tagged count
use only these explicit image tags. Tags are embedded in PNG metadata without
re-encoding the image. JPEG and WebP screenshots can still be viewed, but
embedded participant tagging currently supports PNG screenshots only.

## Library classification

The Library keeps organisation views separate from the photo collection:

- **All worlds** contains only real, non-placeholder world names that have at
  least two photos. World groups and their counts are derived from the current
  library data each time it is scanned, so a group moves between views as
  photos are added or removed.
- **All images** contains every other photo, including photos without world
  metadata, placeholder names such as `Unsorted` or `Unknown World`, names
  that look like screenshot filenames, and one-photo worlds.
- **Current session** contains captures taken since the currently running
  VRChat process started. When VRChat is already open at Organizer startup,
  or transitions from closed to open while Organizer is running, Library
  switches to this tab automatically. It is empty while VRChat is closed.

Missing metadata is represented as no world information rather than a filename
or a synthetic `Unsorted` world. No files are moved, renamed, or deleted by
this classification; it only changes how the existing library is displayed.
The Timeline still includes every photo.

The desktop Library stores one SQLite row per photo in the application data
directory (`library.sqlite`). Startup uses unchanged rows immediately and
rescans only new or changed files in the background. PNG metadata reads only
the signature, chunk headers, and text chunks before `IDAT`; pixel decoding is
reserved for generating a cached 320px JPEG thumbnail.

## Build & Run

### CLI Tool

```bash
# Run directly with cargo
cargo run -p desktop --release -- ~/Pictures/VRChat/VRChat

# Or build a standalone binary
cargo build -p desktop --release
./target/release/desktop ~/Pictures/VRChat/VRChat
```

### Tauri Desktop App

```bash
cargo build -p app --release
./target/release/app
```

### Full Workspace

```bash
cargo build --release
```

## CLI Usage

```
Usage: desktop [PATH]

Arguments:
  [PATH]  Path to VRChat screenshots [default: ~/Pictures/VRChat/VRChat]

Environment Variables:
  VRCHAT_DRY_RUN          Enable dry-run mode
  VRCHAT_SCAN_ALL_MONTHS  Scan all month folders instead of only the latest
  VRCHAT_SINGLE_FOLDER    Treat path as a single folder
```

## Dependencies

All Rust crates used:

- `kamadak-exif` — EXIF data reader
- `image` — Image decoding and dimension reading
- `png` / `flate2` — Raw PNG byte-level parsing with zlib decompression
- `serde` / `serde_json` — JSON parsing for world metadata
- `chrono` — Date handling
- `regex` / `walkdir` — File scanning
- `tauri` — Desktop application framework

## Project structure

```text
rust-rewrite/
├── crates/
│   ├── organizer-core/        Metadata, validation, safe file operations and undo
│   └── desktop/               Scriptable CLI entry point
├── frontend/                  Tauri frontend and dashboard UI
├── src-tauri/                 Tauri commands, watcher lifecycle and app shell
├── packaging/linux/           Desktop entry and AppStream metadata
├── build-appimage.sh          Reproducible Linux AppImage build
└── build-windows.sh           Windows packaging helper
```

## Linux packaging

The supported release artifact is an x86_64 AppImage. Install the Tauri Linux
prerequisites and `appimagetool`, then run:

```bash
cd rust-rewrite
./build-appimage.sh
```

The script fails fast when a required tool is missing, embeds the desktop entry,
AppStream metadata and icon, and writes the versioned result to
`release/v2.1.0/` (with the version read from Cargo metadata).
The AppImage can be launched on both X11 and Wayland through WebKitGTK.

The Windows cross-compilation helper uses the same versioned release directory:

```bash
./build-windows.sh
```

It produces `release/v2.1.0/VRChatOrganizer-2.1.0-windows-x86_64.exe` and
updates `SHA256SUMS` for all generated release binaries.
This is the standalone Windows CLI executable; the Tauri GUI executable and
NSIS/MSI installers must be built natively on Windows or by the Windows CI
runner because they require the native WebView2 toolchain.

For a local development cycle:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

## Troubleshooting

- **No worlds are detected:** keep VRCX running and enable VRCX's Screenshot
  Metadata setting before taking new screenshots.
- **A folder is unavailable:** choose an existing, readable folder. Permission
  errors are reported in the activity panel and do not terminate the app.
- **A template is rejected:** use only `{world}`, `{width}`, `{height}`,
  `{year}`, `{month}`, and `{day}`. Path separators are sanitised.
- **Undo is unavailable:** undo entries are written only for completed moves;
  if the original path already contains a file, the restore is skipped rather
  than overwriting data.
- **Simulation mode:** use **Settings → Preview organization** to run a
  no-write scan. The returned preview includes planned moves and files skipped
  because metadata is missing.
