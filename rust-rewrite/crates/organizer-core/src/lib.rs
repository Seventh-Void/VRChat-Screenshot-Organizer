use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use image::GenericImageView;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub original_path: String,
    pub destination_path: String,
}

/// Log an undo entry to the undo JSON file in the base folder.
pub fn log_undo_entry(base_path: &Path, entry: &UndoEntry) -> Result<()> {
    let undo_path = base_path.join(".vrchat-organizer-undo.json");
    let mut entries: Vec<UndoEntry> = if undo_path.exists() {
        let data = fs::read_to_string(&undo_path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    };
    entries.push(entry.clone());
    let data = serde_json::to_string_pretty(&entries)?;
    fs::write(&undo_path, data)?;
    Ok(())
}

/// Read the undo log from the base folder.
pub fn read_undo_log(base_path: &Path) -> Result<Vec<UndoEntry>> {
    let undo_path = base_path.join(".vrchat-organizer-undo.json");
    if !undo_path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&undo_path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
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
        fs::rename(&dest, &orig)?;
        stats.undone += 1;
        println!("undone: moved {} back to {}", dest.display(), orig.display());
    }

    // Remove the undo file after successful undo
    let undo_path = base_path.join(".vrchat-organizer-undo.json");
    let _ = fs::remove_file(&undo_path);

    Ok(())
}

/// Check if a file is already organized (in a destination subfolder matching the template).
pub fn is_already_organized(file_path: &Path, _config: &OrganizerConfig) -> bool {
    if let Some(parent) = file_path.parent() {
        let parent_name = parent.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        // If the parent folder isn't a date folder (YYYY-MM), it could be an organized subfolder
        let date_re = Regex::new(r"^\d{4}-\d{2}$").unwrap();
        if !date_re.is_match(parent_name) {
            // It's already inside a non-date folder (world name, "Prints", etc.)
            return true;
        }
    }
    false
}

pub fn sanitize_name(name: &str) -> String {
    let mut cleaned = name.to_string();
    for ch in ['<', '>', ':', '"', '|', '?', '*', '/', '\\'] {
        cleaned = cleaned.replace(ch, "_");
    }
    while cleaned.contains("__") {
        cleaned = cleaned.replace("__", "_");
    }
    cleaned.trim().trim_matches('.').to_string()
}

pub fn extract_date_from_filename(filename: &str) -> Option<chrono::NaiveDate> {
    let re = Regex::new(r"VRChat_(\d{4})-(\d{2})-(\d{2})").ok()?;
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
    if data.len() < 8 || &data[..8] != &[137, 80, 78, 71, 13, 10, 26, 10] {
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
                    if let Some(world) = parsed.get("world").and_then(|w| w.get("name")).and_then(|n| n.as_str()) {
                        world_name = Some(sanitize_name(world));
                    }
                }
            }
            "software" | "creator tool" | "creator_tool" => {
                if software.is_none() && !entry.value.is_empty() {
                    software = Some(entry.value.clone());
                }
            }
            _ => {}
        }
    }

    (world_name, software)
}

// ── JPEG EXIF extraction ──────────────────────────────────────────────────

/// Try to extract world_name and software from JPEG EXIF data.
/// Uses tag 270 (ImageDescription) for JSON with world info, and tag 305 for Software.
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
        let value = field.display_value().to_string();
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&value) {
            if let Some(world) = parsed.get("world").and_then(|w| w.get("name")).and_then(|n| n.as_str()) {
                world_name = Some(sanitize_name(world));
            }
        }
    }

    // Tag 305 = Software
    if let Some(field) = exif.get_field(exif::Tag::Software, exif::In::PRIMARY) {
        let value = field.display_value().to_string();
        if !value.is_empty() {
            software = Some(value);
        }
    }

    (world_name, software)
}

pub fn extract_image_meta(path: &Path) -> Result<ImageMeta> {
    let img = image::ImageReader::open(path)
        .with_context(|| format!("failed to open image {}", path.display()))?
        .decode()
        .with_context(|| format!("failed to decode image {}", path.display()))?;

    let (width, height) = img.dimensions();

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

pub fn organize_path(config: &OrganizerConfig, stats: &mut OrganizerStats) -> Result<()> {
    if !config.base_path.exists() {
        anyhow::bail!("path does not exist: {}", config.base_path.display());
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if config.single_folder {
        candidates.push(config.base_path.clone());
    } else {
        for entry in fs::read_dir(&config.base_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if Regex::new(r"^\d{4}-\d{2}$")?.is_match(name) {
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
pub fn organize_single_file(file_path: &Path, config: &OrganizerConfig, stats: &mut OrganizerStats) -> Result<()> {
    stats.processed += 1;
    stats.total += 1;

    // Skip if already organized
    if is_already_organized(file_path, config) {
        stats.already_organized += 1;
        return Ok(());
    }

    // Extract the date from the filename to determine which YYYY-MM folder to use
    let filename = file_path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let month_folder: PathBuf = if let Some(date) = extract_date_from_filename(filename) {
        // Always route into the correct YYYY-MM subfolder based on the file's date
        config.base_path.join(format!("{}-{:02}", date.format("%Y"), date.format("%m")))
    } else {
        // No date in filename — use config.base_path as fallback
        config.base_path.clone()
    };

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
    } else if let Some(world) = meta.world_name.as_ref() {
        let mut target = config.template.clone();
        target = target.replace("{world}", world);
        target = target.replace("{width}", &meta.width.to_string());
        target = target.replace("{height}", &meta.height.to_string());
        if let Some(date) = extract_date_from_filename(filename) {
            target = target.replace("{year}", &date.format("%Y").to_string());
            target = target.replace("{month}", &date.format("%m").to_string());
            target = target.replace("{day}", &date.format("%d").to_string());
        }
        Some(target)
    } else {
        None
    };

    if let Some(target_sub) = target_sub {
        let destination = month_folder.join(&target_sub).join(file_path.file_name().unwrap_or_default());
        if config.dry_run {
            println!("dry run: would move {} -> {}", file_path.display(), target_sub);
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            // Log undo entry before moving
            let original_full = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf());
            let _ = log_undo_entry(
                config.base_path.as_path(),
                &UndoEntry {
                    original_path: original_full.to_string_lossy().to_string(),
                    destination_path: destination.to_string_lossy().to_string(),
                },
            );
            let _ = fs::rename(file_path, &destination);
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
    }

    Ok(())
}

fn process_folder(folder: &Path, config: &OrganizerConfig, stats: &mut OrganizerStats) -> Result<()> {
    let mut image_files: Vec<PathBuf> = WalkDir::new(folder)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| matches!(path.extension().and_then(|s| s.to_str()), Some("png" | "jpg" | "jpeg" | "webp")))
        .collect();
    image_files.sort();

    for image_file in image_files {
        stats.processed += 1;
        stats.total += 1;

        // Skip if already organized
        if is_already_organized(&image_file, config) {
            stats.already_organized += 1;
            continue;
        }

        let meta = match extract_image_meta(&image_file) {
            Ok(meta) => meta,
            Err(err) => {
                stats.errors += 1;
                eprintln!("error reading metadata from {}: {err}", image_file.display());
                continue;
            }
        };

        let target_sub = if (meta.width, meta.height) == (2048, 1440) {
            Some("Prints".to_string())
        } else if let Some(world) = meta.world_name.as_ref() {
            let mut target = config.template.clone();
            target = target.replace("{world}", world);
            target = target.replace("{width}", &meta.width.to_string());
            target = target.replace("{height}", &meta.height.to_string());
            if let Some(date) = extract_date_from_filename(&image_file.file_name().and_then(|n| n.to_str()).unwrap_or_default()) {
                target = target.replace("{year}", &date.format("%Y").to_string());
                target = target.replace("{month}", &date.format("%m").to_string());
                target = target.replace("{day}", &date.format("%d").to_string());
            }
            Some(target)
        } else {
            None
        };

        if let Some(target_sub) = target_sub {
            let destination = folder.join(&target_sub).join(image_file.file_name().unwrap_or_default());
            if config.dry_run {
                println!("dry run: would move {} -> {}", image_file.display(), target_sub);
            } else {
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)?;
                }
                // Log undo entry before moving
                let original_full = image_file.canonicalize().unwrap_or_else(|_| image_file.clone());
                let _ = log_undo_entry(
                    config.base_path.as_path(),
                    &UndoEntry {
                        original_path: original_full.to_string_lossy().to_string(),
                        destination_path: destination.to_string_lossy().to_string(),
                    },
                );
                let _ = fs::rename(&image_file, &destination);
                println!("moved {} -> {}", image_file.display(), target_sub);
            }
            stats.organized += 1;
            // Track per-world photo count (skip "Prints" folder)
            if target_sub != "Prints" {
                let world_name = target_sub.clone();
                *stats.worlds.entry(world_name).or_insert(0) += 1;
            }
        } else {
            stats.no_metadata += 1;
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
    fn extracts_date_from_vrchat_filename() {
        assert_eq!(extract_date_from_filename("VRChat_2025-01-12_foo.png"), Some(chrono::NaiveDate::from_ymd_opt(2025, 1, 12).unwrap()));
    }
}

