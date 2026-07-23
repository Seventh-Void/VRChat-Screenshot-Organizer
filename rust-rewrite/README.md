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
- **WebP** — Dimensions only (no EXIF/text chunk metadata supported yet)

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
  VRCHAT_SCAN_ALL_MONTHS  Scan all month folders
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
