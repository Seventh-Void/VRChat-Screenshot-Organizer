use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMeta {
    pub world_name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub software: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OrganizerConfig {
    pub base_path: PathBuf,
    pub dry_run: bool,
    pub scan_all_months: bool,
    pub single_folder: bool,
    pub template: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OrganizerStats {
    pub processed: usize,
    pub organized: usize,
    pub already_organized: usize,
    pub no_metadata: usize,
    pub errors: usize,
    pub undone: usize,
    pub total: usize,
    pub worlds: HashMap<String, usize>,
    pub preview: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub original_path: String,
    pub destination_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub timestamp: String,
    pub operation: String,
    pub status: String,
    pub duration_ms: u128,
    pub files_affected: usize,
    pub details: String,
}

pub fn log_activity(base_path: &Path, entry: &ActivityEntry) -> Result<()> {
    let path = base_path.join(".vrchat-organizer-activity.json");
    let mut entries: Vec<ActivityEntry> = if path.exists() {
        serde_json::from_str(&fs::read_to_string(&path)?)
            .with_context(|| format!("activity log is corrupt: {}", path.display()))?
    } else {
        Vec::new()
    };
    entries.push(entry.clone());
    if entries.len() > 500 {
        entries.drain(..entries.len() - 500);
    }
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    fs::write(&tmp, serde_json::to_string_pretty(&entries)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn read_activity_log(base_path: &Path) -> Result<Vec<ActivityEntry>> {
    let path = base_path.join(".vrchat-organizer-activity.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&fs::read_to_string(&path)?)
        .with_context(|| format!("activity log is corrupt: {}", path.display()))
}

/// Log an undo entry to the undo JSON file in the base folder.
/// Uses atomic write (temp file + rename) to prevent corruption on crash.
pub fn log_undo_entry(base_path: &Path, entry: &UndoEntry) -> Result<()> {
    let undo_path = base_path.join(".vrchat-organizer-undo.json");
    let mut entries: Vec<UndoEntry> = if undo_path.exists() {
        let data = fs::read_to_string(&undo_path)?;
        serde_json::from_str(&data)
            .with_context(|| format!("existing undo log is corrupt: {}", undo_path.display()))?
    } else {
        Vec::new()
    };
    entries.push(entry.clone());
    let data = serde_json::to_string_pretty(&entries)?;
    // Atomic write: write to temp file first, then rename to prevent partial writes
    let tmp_path = base_path.join(format!(
        ".vrchat-organizer-undo.json.tmp.{}",
        std::process::id()
    ));
    fs::write(&tmp_path, data)?;
    fs::rename(&tmp_path, &undo_path)?;
    Ok(())
}

/// Read the undo log from the base folder.
pub fn read_undo_log(base_path: &Path) -> Result<Vec<UndoEntry>> {
    let undo_path = base_path.join(".vrchat-organizer-undo.json");
    if !undo_path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&undo_path)?;
    let entries = serde_json::from_str(&data)
        .with_context(|| format!("undo log is corrupt: {}", undo_path.display()))?;
    Ok(entries)
}

/// Undo the last organization run by moving files back to their original paths.
pub fn undo_organization(base_path: &Path, stats: &mut OrganizerStats) -> Result<()> {
    let entries = read_undo_log(base_path)?;
    if entries.is_empty() {
        anyhow::bail!("no undo entries found");
    }

    for entry in &entries {
        let dest = PathBuf::from(&entry.destination_path);
        let orig = PathBuf::from(&entry.original_path);

        if !dest.exists() {
            // File was already moved or deleted; skip
            stats.errors += 1;
            continue;
        }

        if let Some(parent) = orig.parent() {
            fs::create_dir_all(parent)?;
        }
        if orig.exists() {
            stats.errors += 1;
            continue;
        }
        // Use the same cross-volume-safe move helper as organization.
        move_file(&dest, &orig)?;
        stats.undone += 1;
        println!(
            "undone: moved {} back to {}",
            dest.display(),
            orig.display()
        );
    }

    // Remove the undo file after successful undo
    let undo_path = base_path.join(".vrchat-organizer-undo.json");
    let _ = fs::remove_file(&undo_path);

    Ok(())
}

/// Check if a file is already organized (in a destination subfolder matching the template).
pub fn is_already_organized(file_path: &Path, config: &OrganizerConfig) -> bool {
    // Compile once and reuse across calls (avoids recompiling a regex per file).
    static DATE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let date_re = DATE_RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}$").unwrap());

    let Some(parent) = file_path.parent() else {
        return false;
    };
    if parent == config.base_path {
        return false;
    }

    // A nested folder is not automatically an organized folder: arbitrary
    // user-created nesting must still be scanned. Only the app's
    // YYYY-MM/<destination> layout counts as organized.
    let mut current = Some(parent);
    while let Some(dir) = current {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if date_re.is_match(name) {
            return dir != parent;
        }
        if dir == config.base_path {
            break;
        }
        current = dir.parent();
    }
    false
}

/// Move a file from `src` to `dst`, falling back to copy+remove if they are on
/// different filesystems (EXDEV). Returns the error if the move still fails.
pub fn move_file(src: &Path, dst: &Path) -> anyhow::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(17) => {
            // EXDEV: copy to a same-directory temporary file first so an
            // interrupted copy never leaves a partial destination.
            let parent = dst
                .parent()
                .ok_or_else(|| anyhow::anyhow!("destination has no parent"))?;
            let tmp = parent.join(format!(
                ".{}.copying.{}",
                dst.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("screenshot"),
                std::process::id()
            ));
            let result = (|| -> anyhow::Result<()> {
                fs::copy(src, &tmp)?;
                fs::rename(&tmp, dst)?;
                fs::remove_file(src)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

pub fn sanitize_name(name: &str) -> String {
    let mut cleaned = name.to_string();
    for ch in ['<', '>', ':', '"', '|', '?', '*', '/', '\\'] {
        cleaned = cleaned.replace(ch, "_");
    }
    while cleaned.contains("__") {
        cleaned = cleaned.replace("__", "_");
    }
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "Unnamed World".to_string()
    } else {
        cleaned
    }
}

const TEMPLATE_VARIABLES: [&str; 6] = ["world", "width", "height", "year", "month", "day"];

pub fn validate_template(template: &str) -> Result<()> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        anyhow::bail!("template must not be empty");
    }
    for placeholder in trimmed.match_indices('{').map(|(index, _)| index) {
        let end = trimmed[placeholder..]
            .find('}')
            .map(|offset| placeholder + offset)
            .ok_or_else(|| anyhow::anyhow!("template contains an unclosed '{{'"))?;
        let variable = &trimmed[placeholder + 1..end];
        if !TEMPLATE_VARIABLES.contains(&variable) {
            anyhow::bail!("unsupported template variable: {{{variable}}}");
        }
    }
    if trimmed.contains('}') && !trimmed.contains('{') {
        anyhow::bail!("template contains an unmatched '}}'");
    }
    Ok(())
}

/// Resolve template variables against image metadata and filename.
/// Supported variables: {world}, {width}, {height}, {year}, {month}, {day}
pub fn resolve_template(template: &str, meta: &ImageMeta, filename: &str) -> String {
    let mut target = template.to_string();
    if let Some(world) = &meta.world_name {
        target = target.replace("{world}", world);
    }
    target = target.replace("{width}", &meta.width.to_string());
    target = target.replace("{height}", &meta.height.to_string());
    if let Some(date) = extract_date_from_filename(filename) {
        target = target.replace("{year}", &date.format("%Y").to_string());
        target = target.replace("{month}", &date.format("%m").to_string());
        target = target.replace("{day}", &date.format("%d").to_string());
    }
    sanitize_name(&target)
}

pub fn extract_date_from_filename(filename: &str) -> Option<chrono::NaiveDate> {
    static DATE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = DATE_RE.get_or_init(|| Regex::new(r"VRChat_(\d{4})-(\d{2})-(\d{2})").unwrap());
    let caps = re.captures(filename)?;
    let year: i32 = caps.get(1)?.as_str().parse().ok()?;
    let month: u32 = caps.get(2)?.as_str().parse().ok()?;
    let day: u32 = caps.get(3)?.as_str().parse().ok()?;
    chrono::NaiveDate::from_ymd_opt(year, month, day)
}

// ── PNG text chunk extraction ──────────────────────────────────────────────
// VRChat embeds world metadata in PNG text chunks (tEXt, zTXt, iTXt).
// We parse the PNG file at the byte level to extract them.

/// Represents a parsed PNG text chunk with keyword and value.
struct PngTextEntry {
    keyword: String,
    value: String,
}

/// Read PNG file and extract all text chunks (tEXt, zTXt, iTXt).
fn extract_png_text_chunks(path: &Path) -> Result<Vec<PngTextEntry>> {
    let data = fs::read(path).context("failed to read PNG file")?;

    // PNG signature check
    if data.len() < 8 || data[..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        anyhow::bail!("not a valid PNG file");
    }

    let mut entries = Vec::new();
    let mut offset = 8; // start after signature

    while offset + 12 <= data.len() {
        let chunk_len = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;

        // Guard against malformed headers: ensure the full chunk (length +
        // type + data + CRC) is within bounds before slicing. Prevents a
        // panic on corrupt/incomplete PNG files.
        let chunk_total = 12usize.checked_add(chunk_len).ok_or_else(|| {
            anyhow::anyhow!("malformed PNG: chunk length {} overflows usize", chunk_len)
        })?;
        if offset + chunk_total > data.len() {
            anyhow::bail!(
                "malformed PNG: chunk length {} exceeds file size",
                chunk_len
            );
        }

        let chunk_type = &data[offset + 4..offset + 8];

        // If we hit IEND, we're done
        if chunk_type == b"IEND" {
            break;
        }

        if chunk_type == b"tEXt" || chunk_type == b"zTXt" || chunk_type == b"iTXt" {
            let chunk_data = &data[offset + 8..offset + 8 + chunk_len];

            // Find the null byte separating keyword from value
            if let Some(null_pos) = chunk_data.iter().position(|&b| b == 0) {
                let keyword = String::from_utf8_lossy(&chunk_data[..null_pos]).to_string();
                let raw_value = &chunk_data[null_pos + 1..];

                let value = match chunk_type {
                    b"tEXt" => {
                        // Raw Latin-1 text after keyword
                        raw_value.iter().map(|&b| b as char).collect::<String>()
                    }
                    b"zTXt" => {
                        // Byte after null = compression method (should be 0 for zlib)
                        if raw_value.is_empty() {
                            String::new()
                        } else {
                            let compressed = &raw_value[1..]; // skip compression method byte
                            let mut decoder = ZlibDecoder::new(compressed);
                            let mut decompressed = String::new();
                            decoder.read_to_string(&mut decompressed).ok();
                            decompressed
                        }
                    }
                    b"iTXt" => {
                        // compression flag (1 byte) + compression method (1 byte) + language (null-term) + translated keyword (null-term) + text
                        if raw_value.len() < 2 {
                            String::new()
                        } else {
                            let compression_flag = raw_value[0];
                            // skip compression flag + method (2 bytes)
                            let after_flags = &raw_value[2..];
                            // skip language (null-terminated)
                            if let Some(lang_end) = after_flags.iter().position(|&b| b == 0) {
                                let after_lang = &after_flags[lang_end + 1..];
                                // skip translated keyword (null-terminated)
                                if let Some(tk_end) = after_lang.iter().position(|&b| b == 0) {
                                    let text_bytes = &after_lang[tk_end + 1..];
                                    if compression_flag == 0 {
                                        // Uncompressed UTF-8
                                        String::from_utf8_lossy(text_bytes).to_string()
                                    } else {
                                        // Compressed
                                        let mut decoder = ZlibDecoder::new(text_bytes);
                                        let mut decompressed = String::new();
                                        decoder.read_to_string(&mut decompressed).ok();
                                        decompressed
                                    }
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            }
                        }
                    }
                    _ => unreachable!(),
                };

                entries.push(PngTextEntry { keyword, value });
            }
        }

        // Move to next chunk: 4 bytes type + chunk_len + 4 bytes CRC
        offset += 12 + chunk_len;
    }

    Ok(entries)
}

/// Parse world name and software from a list of PNG text entries.
fn parse_png_entries(entries: &[PngTextEntry]) -> (Option<String>, Option<String>) {
    let mut world_name: Option<String> = None;
    let mut software: Option<String> = None;

    for entry in entries {
        match entry.keyword.to_lowercase().as_str() {
            "description" | "comment" => {
                if world_name.is_some() {
                    continue;
                }
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&entry.value) {
                    if let Some(world) = parsed
                        .get("world")
                        .and_then(|w| w.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        world_name = Some(sanitize_name(world));
                    }
                }
            }
            "software" | "creator tool" | "creator_tool"
                if software.is_none() && !entry.value.is_empty() =>
            {
                software = Some(entry.value.clone());
            }
            _ => {}
        }
    }

    (world_name, software)
}

// ── JPEG EXIF extraction ──────────────────────────────────────────────────

/// Try to extract world_name and software from JPEG EXIF data.
/// Uses tag 270 (ImageDescription) for JSON with world info, and tag 305 for Software.
///
/// NOTE: `kamadak-exif`'s `display_value()` wraps string fields in quotes, which would
/// break JSON parsing. We access the raw ASCII bytes directly instead.
fn extract_exif_meta(path: &Path) -> (Option<String>, Option<String>) {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None),
    };
    let mut reader = std::io::BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut reader) {
        Ok(e) => e,
        Err(_) => return (None, None),
    };

    let mut world_name: Option<String> = None;
    let mut software: Option<String> = None;

    // Tag 270 = ImageDescription — often contains JSON with world info
    if let Some(field) = exif.get_field(exif::Tag::ImageDescription, exif::In::PRIMARY) {
        // Use raw ASCII bytes instead of display_value() to avoid quoting issues
        if let exif::Value::Ascii(bytes) = &field.value {
            if let Some(first) = bytes.first() {
                if let Ok(value) = std::str::from_utf8(first) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
                        if let Some(world) = parsed
                            .get("world")
                            .and_then(|w| w.get("name"))
                            .and_then(|n| n.as_str())
                        {
                            world_name = Some(sanitize_name(world));
                        }
                    }
                }
            }
        }
    }

    // Tag 305 = Software — also use raw bytes to avoid display_value quoting
    if let Some(field) = exif.get_field(exif::Tag::Software, exif::In::PRIMARY) {
        if let exif::Value::Ascii(bytes) = &field.value {
            if let Some(first) = bytes.first() {
                if let Ok(value) = std::str::from_utf8(first) {
                    let trimmed = value.trim().trim_matches('"').to_string();
                    if !trimmed.is_empty() {
                        software = Some(trimmed);
                    }
                }
            }
        }
    }

    (world_name, software)
}

pub fn extract_image_meta(path: &Path) -> Result<ImageMeta> {
    // Use `into_dimensions()` which reads only the image header rather than
    // decoding the full pixel data — significantly faster for large screenshots.
    let reader = image::ImageReader::open(path)
        .with_context(|| format!("failed to open image {}", path.display()))?;
    let (width, height) = reader
        .into_dimensions()
        .with_context(|| format!("failed to read image dimensions for {}", path.display()))?;

    // Determine file extension
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase());

    let (world_name, software) = match ext.as_deref() {
        Some("jpg") | Some("jpeg") => {
            // Try EXIF for JPEG files
            extract_exif_meta(path)
        }
        Some("png") => {
            // Try PNG text chunks
            let entries = extract_png_text_chunks(path).unwrap_or_default();
            parse_png_entries(&entries)
        }
        // For WebP and other formats, we can only get dimensions
        _ => (None, None),
    };

    Ok(ImageMeta {
        world_name,
        width,
        height,
        software,
    })
}

fn determine_month_folder(file_path: &Path, config: &OrganizerConfig) -> PathBuf {
    static MONTH_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let month_re = MONTH_RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}$").unwrap());

    let mut current = file_path.parent();
    while let Some(dir) = current {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if month_re.is_match(name) {
            return dir.to_path_buf();
        }

        if dir == config.base_path || dir == Path::new("") {
            break;
        }

        current = dir.parent();
    }

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if let Some(date) = extract_date_from_filename(filename) {
        return config
            .base_path
            .join(format!("{}-{:02}", date.format("%Y"), date.format("%m")));
    }

    config.base_path.clone()
}

pub fn organize_path(config: &OrganizerConfig, stats: &mut OrganizerStats) -> Result<()> {
    let started = std::time::Instant::now();
    validate_template(&config.template)?;
    if !config.base_path.exists() {
        anyhow::bail!("path does not exist: {}", config.base_path.display());
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    // Compile once — used to detect YYYY-MM month folder names.
    static MONTH_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let month_re = MONTH_RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}$").unwrap());

    if config.single_folder {
        candidates.push(config.base_path.clone());
    } else {
        for entry in fs::read_dir(&config.base_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if month_re.is_match(name) {
                    candidates.push(path);
                }
            }
        }
        candidates.sort();
        if !config.scan_all_months {
            if let Some(last) = candidates.last() {
                candidates = vec![last.clone()];
            } else {
                anyhow::bail!("no month folders found in {}", config.base_path.display());
            }
        }
    }

    for folder in candidates {
        process_folder(&folder, config, stats)?;
    }

    let status = if stats.errors == 0 {
        "success"
    } else {
        "warning"
    };
    log_activity(
        &config.base_path,
        &ActivityEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            operation: if config.dry_run { "simulation" } else { "scan" }.to_string(),
            status: status.to_string(),
            duration_ms: started.elapsed().as_millis(),
            files_affected: stats.organized,
            details: format!(
                "processed={}, organized={}, skipped={}, no_metadata={}, errors={}",
                stats.processed,
                stats.organized,
                stats.already_organized,
                stats.no_metadata,
                stats.errors
            ),
        },
    )
    .context("failed to write activity log")?;
    Ok(())
}

/// Organize a single file, always routing it into the correct YYYY-MM subfolder.
///
/// The file's date is extracted from its filename (VRChat_YYYY-MM-DD_*.png).
/// The destination is always `base_path/YYYY-MM/worldname/filename`, where YYYY-MM
/// comes from the extracted date. This ensures files stay organized into the correct
/// date-based subfolder regardless of where they were on disk before organization.
///
/// If the file is already inside a YYYY-MM parent folder, that folder is verified
/// against the extracted date and used as confirmation. If they don't match, the
/// extracted date takes precedence (the file will be moved to the correct folder).
///
/// If no date can be extracted from the filename, the file is organized directly
/// under `base_path/worldname/filename` as a fallback.
fn unique_destination(candidate: &Path) -> PathBuf {
    if !candidate.exists() {
        return candidate.to_path_buf();
    }

    let parent = candidate.parent().unwrap_or_else(|| Path::new(""));
    let stem = candidate
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("screenshot");
    let extension = candidate.extension().and_then(|value| value.to_str());
    for index in 1.. {
        let suffix = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let destination = parent.join(suffix);
        if !destination.exists() {
            return destination;
        }
    }
    unreachable!("range is infinite")
}

pub fn organize_single_file(
    file_path: &Path,
    config: &OrganizerConfig,
    stats: &mut OrganizerStats,
) -> Result<()> {
    validate_template(&config.template)?;
    stats.processed += 1;
    stats.total += 1;

    // Skip if already organized
    if is_already_organized(file_path, config) {
        stats.already_organized += 1;
        return Ok(());
    }

    // Prefer the existing YYYY-MM folder if the file is already nested inside one.
    // This keeps files in their current month bucket instead of re-routing them
    // based on the filename date when they were already moved once before.
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let month_folder = determine_month_folder(file_path, config);

    let meta = match extract_image_meta(file_path) {
        Ok(meta) => meta,
        Err(err) => {
            stats.errors += 1;
            eprintln!("error reading metadata from {}: {err}", file_path.display());
            return Err(err).context("failed to extract metadata");
        }
    };

    let target_sub = if (meta.width, meta.height) == (2048, 1440) {
        Some("Prints".to_string())
    } else if meta.world_name.is_some() {
        Some(resolve_template(&config.template, &meta, filename))
    } else {
        None
    };

    if let Some(target_sub) = target_sub {
        let destination = unique_destination(
            &month_folder
                .join(&target_sub)
                .join(file_path.file_name().unwrap_or_default()),
        );
        if config.dry_run {
            stats.preview.push(format!(
                "Would move:\n{}\n→ {}",
                file_path.display(),
                destination.display()
            ));
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }

            // Log undo entry before moving
            let original_full = file_path
                .canonicalize()
                .unwrap_or_else(|_| file_path.to_path_buf());
            // Canonicalize the parent folder (the destination file doesn't exist
            // yet) so the undo log is stable even if the base path contains
            // symlinks or is later resolved differently.
            let dest_parent = destination
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .unwrap_or_else(|| destination.parent().unwrap_or(Path::new("")).to_path_buf());
            let dest_full = dest_parent.join(destination.file_name().unwrap_or_default());
            log_undo_entry(
                config.base_path.as_path(),
                &UndoEntry {
                    original_path: original_full.to_string_lossy().to_string(),
                    destination_path: dest_full.to_string_lossy().to_string(),
                },
            )?;
            // Move with cross-device (EXDEV) fallback; propagate errors so the
            // stats aren't inflated for moves that never happened.
            move_file(file_path, &destination).with_context(|| {
                format!(
                    "failed to move {} -> {}",
                    file_path.display(),
                    destination.display()
                )
            })?;
            println!("moved {} -> {}", file_path.display(), target_sub);
        }
        stats.organized += 1;
        // Track per-world photo count (skip "Prints" folder)
        if target_sub != "Prints" {
            let world_name = target_sub.clone();
            *stats.worlds.entry(world_name).or_insert(0) += 1;
        }
    } else {
        stats.no_metadata += 1;
        if config.dry_run {
            stats.preview.push(format!(
                "Would skip:\n{}\n→ Missing image metadata",
                file_path.display()
            ));
        }
    }

    Ok(())
}

fn process_folder(
    folder: &Path,
    config: &OrganizerConfig,
    stats: &mut OrganizerStats,
) -> Result<()> {
    let mut image_files: Vec<PathBuf> = WalkDir::new(folder)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension()
                .and_then(|s| s.to_str())
                .map(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "webp"
                    )
                })
                .unwrap_or(false)
        })
        .collect();
    image_files.sort();

    for image_file in image_files {
        // Delegate to organize_single_file which handles stats, metadata, template resolution, undo
        if let Err(e) = organize_single_file(&image_file, config, stats) {
            eprintln!("error processing {}: {e}", image_file.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_invalid_path_characters() {
        assert_eq!(sanitize_name("My/World:Name"), "My_World_Name");
    }

    #[test]
    fn sanitizes_trailing_dots_and_leading_dots() {
        assert_eq!(sanitize_name("...World..."), "World");
        assert_eq!(sanitize_name("  padded  "), "padded");
    }

    #[test]
    fn extracts_date_from_vrchat_filename() {
        assert_eq!(
            extract_date_from_filename("VRChat_2025-01-12_foo.png"),
            Some(chrono::NaiveDate::from_ymd_opt(2025, 1, 12).unwrap())
        );
    }

    #[test]
    fn no_date_in_filename_returns_none() {
        assert_eq!(extract_date_from_filename("Screenshot_foo.png"), None);
    }

    #[test]
    fn resolve_template_replaces_all_variables() {
        let meta = ImageMeta {
            world_name: Some("Black Cat".to_string()),
            width: 1920,
            height: 1080,
            software: None,
        };
        let result = resolve_template(
            "{world}-{year}-{month}-{day}-{width}x{height}",
            &meta,
            "VRChat_2025-01-12_foo.png",
        );
        assert_eq!(result, "Black Cat-2025-01-12-1920x1080");
    }

    #[test]
    fn resolve_template_handles_missing_world() {
        let meta = ImageMeta {
            world_name: None,
            width: 2048,
            height: 1440,
            software: None,
        };
        // {world} stays literal when no world metadata is present
        let result = resolve_template(
            "{world}-{width}x{height}",
            &meta,
            "VRChat_2025-01-12_foo.png",
        );
        assert_eq!(result, "{world}-2048x1440");
    }

    #[test]
    fn validates_supported_template_variables() {
        assert!(validate_template("{world}/{year}").is_ok());
        assert!(validate_template("{unknown}").is_err());
        assert!(validate_template("{world").is_err());
    }

    #[test]
    fn sanitizes_template_path_separators() {
        let meta = ImageMeta {
            world_name: Some("World".to_string()),
            width: 1920,
            height: 1080,
            software: None,
        };
        assert_eq!(
            resolve_template("../{world}/nested", &meta, "capture.png"),
            "_World_nested"
        );
    }

    #[test]
    fn chooses_non_destructive_destination_for_duplicate_names() {
        let dir = std::env::temp_dir().join("vrchat-organizer-test-destination");
        std::fs::create_dir_all(&dir).unwrap();
        let candidate = dir.join("capture.png");
        std::fs::write(&candidate, "existing").unwrap();

        let next = unique_destination(&candidate);

        assert_eq!(
            next.file_name().and_then(|name| name.to_str()),
            Some("capture (1).png")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_already_organized_detects_subfolder() {
        let config = OrganizerConfig {
            base_path: PathBuf::from("/tmp/vrchat"),
            dry_run: false,
            scan_all_months: false,
            single_folder: false,
            template: "{world}".to_string(),
        };
        // Inside a non-date world folder → organized
        assert!(is_already_organized(
            Path::new("/tmp/vrchat/2025-01/Black Cat/foo.png"),
            &config
        ));
        // Loose file directly in a date folder → NOT organized
        assert!(!is_already_organized(
            Path::new("/tmp/vrchat/2025-01/foo.png"),
            &config
        ));
    }

    #[test]
    fn is_already_organized_loose_root_file_in_single_folder_mode() {
        let config = OrganizerConfig {
            base_path: PathBuf::from("/tmp/vrchat"),
            dry_run: false,
            scan_all_months: false,
            single_folder: true,
            template: "{world}".to_string(),
        };
        // A loose file at the base path root must NOT be treated as organized
        // (this was a bug in single-folder mode).
        assert!(!is_already_organized(
            Path::new("/tmp/vrchat/foo.png"),
            &config
        ));
    }

    #[test]
    fn prefers_existing_month_folder_over_filename_date_when_file_is_already_in_a_month_folder() {
        let config = OrganizerConfig {
            base_path: PathBuf::from("/tmp/vrchat"),
            dry_run: false,
            scan_all_months: false,
            single_folder: false,
            template: "{world}".to_string(),
        };
        let file_path = Path::new("/tmp/vrchat/2025-01/World/VRChat_2024-12-01_foo.png");

        let month_folder = determine_month_folder(file_path, &config);

        assert_eq!(month_folder, PathBuf::from("/tmp/vrchat/2025-01"));
    }

    #[test]
    fn png_chunks_extract_text_entries() {
        // Build a minimal PNG with a tEXt chunk containing a Description JSON.
        // PNG signature
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
        // IHDR chunk (minimal 13-byte payload) so the file is a valid-enough PNG
        let ihdr_payload: Vec<u8> = vec![
            0, 0, 0, 1, // width=1
            0, 0, 0, 1, // height=1
            8, // bit depth
            6, // color type (RGBA)
            0, // compression
            0, // filter
            0, // interlace
        ];
        png.extend_from_slice(&(ihdr_payload.len() as u32).to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&ihdr_payload);
        png.extend_from_slice(&[0, 0, 0, 0]); // dummy CRC

        // tEXt chunk: "Description\0{\"world\":{\"name\":\"Black Cat\"}}"
        let text = b"Description\0{\"world\":{\"name\":\"Black Cat\"}}";
        png.extend_from_slice(&(text.len() as u32).to_be_bytes());
        png.extend_from_slice(b"tEXt");
        png.extend_from_slice(text);
        png.extend_from_slice(&[0, 0, 0, 0]); // dummy CRC

        // IEND chunk
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0, 0, 0, 0]);

        let dir = std::env::temp_dir().join("vrchat-organizer-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_with_meta.png");
        std::fs::write(&path, &png).unwrap();

        let entries = extract_png_text_chunks(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].keyword, "Description");

        let (world, _software) = parse_png_entries(&entries);
        assert_eq!(world.as_deref(), Some("Black Cat"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_png_returns_error_not_panic() {
        // Header claims a chunk length that exceeds the file size → must error,
        // not panic with an index-out-of-bounds.
        let dir = std::env::temp_dir().join("vrchat-organizer-test-bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.png");
        // Valid PNG signature
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
        // Chunk length 0xFFFFFFFF (huge), type tEXt, then truncated data.
        // Total file must be long enough for the chunk-header loop guard
        // (offset + 12 <= len) to pass so the bounds check is actually reached.
        png.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        png.extend_from_slice(b"tEXt");
        png.extend_from_slice(b"abcdefghij"); // 10 bytes of "data" (far less than claimed)
        std::fs::write(&path, &png).unwrap();

        let result = extract_png_text_chunks(&path);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_file_works_within_same_filesystem() {
        let dir = std::env::temp_dir().join("vrchat-organizer-test-move");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.txt");
        let dst = dir.join("b.txt");
        std::fs::write(&src, "hello").unwrap();

        move_file(&src, &dst).unwrap();
        assert!(!src.exists());
        assert!(dst.exists());
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "hello");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
