use anyhow::{Context, Result};
use chrono::TimeZone;
use fast_image_resize::{
    images::{Image as ResizeImage, ImageRef},
    pixels::PixelType,
    ResizeOptions, Resizer,
};
use flate2::read::ZlibDecoder;
use rayon::prelude::*;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError, TryLockError};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct CachedPhoto {
    size_bytes: u64,
    modified_secs: u64,
    fingerprint: Option<String>,
    photo: LibraryPhoto,
}

static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);
static MONTH_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
static ORGANIZATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const PLACEHOLDER_WORLD_PATTERNS: &[&str] = &[
    "capture",
    "screenshot",
    "vrchat_",
    "new folder",
    "unsorted",
    "unknown",
    ".png",
    ".jpg",
    ".jpeg",
    ".webp",
];

pub fn clear_cancel_request() {
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
}

pub fn request_cancel() {
    CANCEL_REQUESTED.store(true, Ordering::SeqCst);
}

fn cancel_requested() -> bool {
    CANCEL_REQUESTED.load(Ordering::SeqCst)
}

fn is_month_folder(name: &str) -> bool {
    MONTH_RE
        .get_or_init(|| Regex::new(r"^\d{4}-\d{2}$").unwrap())
        .is_match(name)
}

/// Classify a library photo using embedded metadata first and the recognized
/// on-disk layout as a fallback.
///
/// `relative` must be relative to the scan root. A file name is never a
/// classification: only a directory component can be a world. During a full
/// scan the fallback layout is `YYYY-MM/<world>/<file>`; during a month-root
/// scan it is `<world>/<file>`.
fn is_placeholder_world_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized.is_empty()
        || PLACEHOLDER_WORLD_PATTERNS.iter().any(|pattern| {
            normalized == *pattern
                || normalized.starts_with(pattern)
                || normalized.ends_with(pattern)
        })
}

/// Classify a photo into a world only when the folder is a credible world
/// group. Both metadata and path fallback require at least two sibling images
/// and a non-placeholder world name.
pub fn classify_world(
    relative: &Path,
    metadata_world: Option<&str>,
    scan_all_months: bool,
    world_file_count: usize,
) -> Option<String> {
    if let Some(world) = metadata_world.map(str::trim).filter(|world| {
        !is_placeholder_world_name(world) && *world != "Prints" && !world.starts_with('.')
    }) {
        if world_file_count >= 2 {
            return Some(world.to_owned());
        }
        return None;
    }

    let components: Vec<String> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect();
    if components.len() < 2 {
        return None;
    }

    if scan_all_months {
        return components
            .first()
            .filter(|month| is_month_folder(month))
            .and_then(|_| components.get(1))
            .filter(|name| {
                world_file_count >= 2
                    && !is_placeholder_world_name(name)
                    && *name != "Prints"
                    && !name.starts_with('.')
            })
            .cloned();
    }

    components
        .first()
        .filter(|name| {
            world_file_count >= 2
                && !is_placeholder_world_name(name)
                && *name != "Prints"
                && !name.starts_with('.')
        })
        .cloned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMeta {
    pub world_name: Option<String>,
    pub participants: Vec<String>,
    pub tagged_participants: Vec<String>,
    pub captured_at: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub software: Option<String>,
}

/// Return the currently organized screenshots grouped by world folder.
pub fn scan_library(base_path: &Path) -> Result<OrganizerStats> {
    scan_library_with_options(base_path, false)
}

pub fn scan_library_with_options(
    base_path: &Path,
    scan_all_months: bool,
) -> Result<OrganizerStats> {
    scan_library_with_cache(base_path, scan_all_months, None)
}

pub fn scan_library_with_cache(
    base_path: &Path,
    scan_all_months: bool,
    cache_path: Option<&Path>,
) -> Result<OrganizerStats> {
    let (details, unorganized_photos) =
        scan_library_partition_with_options(base_path, scan_all_months, cache_path)?;
    let mut stats = OrganizerStats::default();
    for world in &details {
        stats.total += world.photos.len();
        stats.worlds.insert(world.name.clone(), world.photos.len());
    }
    stats.total += unorganized_photos.len();
    stats.world_details = details;
    stats.unorganized_photos = unorganized_photos;
    Ok(stats)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPhoto {
    pub path: String,
    pub thumbnail_path: String,
    pub captured_at: Option<u64>,
    pub world_name: Option<String>,
    pub participants: Vec<String>,
    #[serde(default)]
    pub tagged_participants: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryWorld {
    pub name: String,
    pub photos: Vec<LibraryPhoto>,
    pub last_capture: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPerson {
    pub id: i64,
    pub display_name: String,
    pub thumbnail_path: Option<String>,
    pub screenshot_count: u64,
    pub last_seen: Option<u64>,
    pub favourite_world: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPersonPhoto {
    pub path: String,
    pub crop_path: String,
    pub world_name: Option<String>,
    pub captured_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionalPersonTag {
    pub id: i64,
    pub screenshot_path: String,
    pub person_name: String,
    pub crop_path: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub world_name: Option<String>,
    pub captured_at: Option<u64>,
    pub confidence: f32,
}

fn crops_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("avatar-crops")
}

/// People database: manually drawn person boxes only. Version 2 removed the
/// old AI recognition tables; their crops are deleted, drawn-box crops
/// (`manual-*.jpg`) are kept.
fn recognition_schema(connection: &mut Connection, db_path: &Path) -> Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version >= 2 {
        return Ok(());
    }
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS avatar_people (
             id INTEGER PRIMARY KEY,
             display_name TEXT NOT NULL UNIQUE,
             thumbnail_path TEXT,
             aliases TEXT NOT NULL DEFAULT '[]',
             created_at INTEGER NOT NULL,
             last_seen INTEGER,
             confidence REAL NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS positional_person_tags (
             id INTEGER PRIMARY KEY,
             screenshot_path TEXT NOT NULL,
             person_name TEXT NOT NULL,
             crop_path TEXT NOT NULL,
             x INTEGER NOT NULL,
             y INTEGER NOT NULL,
             width INTEGER NOT NULL,
             height INTEGER NOT NULL,
             image_width INTEGER NOT NULL,
             image_height INTEGER NOT NULL,
             world_name TEXT,
             captured_at INTEGER,
             confidence REAL NOT NULL DEFAULT 1,
             created_at INTEGER NOT NULL,
             UNIQUE(screenshot_path, person_name, x, y, width, height)
         );
         CREATE INDEX IF NOT EXISTS idx_positional_tags_screenshot ON positional_person_tags(screenshot_path);
         CREATE INDEX IF NOT EXISTS idx_positional_tags_person ON positional_person_tags(person_name);
         DROP TABLE IF EXISTS avatar_samples;
         DROP TABLE IF EXISTS avatar_detections;
         DROP TABLE IF EXISTS avatar_scan_cache;
         DROP TABLE IF EXISTS person_avatar_links;
         DROP TABLE IF EXISTS avatar_profiles;
         DROP TABLE IF EXISTS recognition_settings;
         DELETE FROM avatar_people
             WHERE display_name NOT IN (SELECT person_name FROM positional_person_tags);
         PRAGMA user_version = 2;",
    )?;
    transaction.commit()?;
    let crops = crops_dir(db_path);
    if refuse_symlink(&crops).is_ok() {
        for entry in fs::read_dir(crops).into_iter().flatten().flatten() {
            let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
            let manual = entry.file_name().to_string_lossy().starts_with("manual-");
            if is_file && !manual {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

fn recognition_connection(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut connection = Connection::open(db_path)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    recognition_schema(&mut connection, db_path)?;
    Ok(connection)
}

/// Decode with size limits so a crafted image cannot exhaust memory.
fn decode_image(path: &Path) -> Result<image::DynamicImage> {
    let mut reader = image::ImageReader::open(path)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 << 20);
    reader.limits(limits);
    Ok(reader.decode()?)
}

/// Resize `rgb` (or the `crop` region of it) to exactly `width`×`height`.
fn resize_rgb(
    rgb: &image::RgbImage,
    crop: Option<(u32, u32, u32, u32)>,
    width: u32,
    height: u32,
) -> Result<image::RgbImage> {
    let source = ImageRef::new(rgb.width(), rgb.height(), rgb.as_raw(), PixelType::U8x3)?;
    let mut destination = ResizeImage::new(width, height, PixelType::U8x3);
    let options =
        crop.map(|(x, y, w, h)| ResizeOptions::new().crop(x as f64, y as f64, w as f64, h as f64));
    Resizer::new().resize(&source, &mut destination, options.as_ref())?;
    image::RgbImage::from_raw(width, height, destination.into_vec())
        .context("resize returned an invalid buffer")
}

/// Save the `(x, y, width, height)` region scaled to fit 320×420.
fn write_avatar_crop(
    rgb: &image::RgbImage,
    bounds: (u32, u32, u32, u32),
    path: &Path,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        refuse_symlink(parent)?;
        fs::create_dir_all(parent)?;
    }
    let scale = (320.0 / bounds.2 as f32).min(420.0 / bounds.3 as f32);
    let width = ((bounds.2 as f32 * scale).round() as u32).max(1);
    let height = ((bounds.3 as f32 * scale).round() as u32).max(1);
    resize_rgb(rgb, Some(bounds), width, height)?.save(path)?;
    Ok(())
}

/// Everyone with at least one drawn person box.
pub fn avatar_people(db_path: &Path) -> Result<Vec<AvatarPerson>> {
    let connection = recognition_connection(db_path)?;
    let mut statement = connection.prepare(
        "SELECT p.id,p.display_name,
                (SELECT crop_path FROM positional_person_tags WHERE person_name=p.display_name
                 ORDER BY created_at DESC LIMIT 1),
                (SELECT COUNT(DISTINCT screenshot_path) FROM positional_person_tags
                 WHERE person_name=p.display_name),
                (SELECT MAX(captured_at) FROM positional_person_tags WHERE person_name=p.display_name),
                (SELECT world_name FROM positional_person_tags
                 WHERE person_name=p.display_name AND world_name IS NOT NULL
                 GROUP BY world_name ORDER BY COUNT(*) DESC LIMIT 1) AS favourite_world
         FROM avatar_people p
         WHERE EXISTS (SELECT 1 FROM positional_person_tags WHERE person_name=p.display_name)
         ORDER BY 5 DESC, p.display_name",
    )?;
    let result = statement
        .query_map([], |row| {
            Ok(AvatarPerson {
                id: row.get(0)?,
                display_name: row.get(1)?,
                thumbnail_path: row.get(2)?,
                screenshot_count: row.get::<_, i64>(3)? as u64,
                last_seen: row.get(4)?,
                favourite_world: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(result)
}

pub fn avatar_person_photos(db_path: &Path, person_id: i64) -> Result<Vec<AvatarPersonPhoto>> {
    let connection = recognition_connection(db_path)?;
    let mut statement = connection.prepare(
        "SELECT screenshot_path,crop_path,world_name,captured_at
         FROM positional_person_tags
         WHERE person_name=(SELECT display_name FROM avatar_people WHERE id=?)
         ORDER BY captured_at DESC, screenshot_path",
    )?;
    let photos = statement
        .query_map([person_id], |row| {
            Ok(AvatarPersonPhoto {
                path: row.get(0)?,
                crop_path: row.get(1)?,
                world_name: row.get(2)?,
                captured_at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(photos)
}

fn manual_crop_path(
    db_path: &Path,
    screenshot_path: &Path,
    bounds: (u32, u32, u32, u32),
) -> PathBuf {
    let cache = crops_dir(db_path);
    let mut hasher = Sha256::new();
    hasher.update(screenshot_path.to_string_lossy().as_bytes());
    hasher.update(bounds.0.to_le_bytes());
    hasher.update(bounds.1.to_le_bytes());
    hasher.update(bounds.2.to_le_bytes());
    hasher.update(bounds.3.to_le_bytes());
    cache.join(format!("manual-{:x}.jpg", hasher.finalize()))
}

pub fn positional_person_tags(
    db_path: &Path,
    screenshot_path: &Path,
) -> Result<Vec<PositionalPersonTag>> {
    let connection = recognition_connection(db_path)?;
    let mut statement = connection.prepare(
        "SELECT id,screenshot_path,person_name,crop_path,x,y,width,height,image_width,image_height,
                world_name,captured_at,confidence
         FROM positional_person_tags
         WHERE screenshot_path=?
         ORDER BY id",
    )?;
    let tags = statement
        .query_map([screenshot_path.to_string_lossy().to_string()], |row| {
            Ok(PositionalPersonTag {
                id: row.get(0)?,
                screenshot_path: row.get(1)?,
                person_name: row.get(2)?,
                crop_path: row.get(3)?,
                x: row.get::<_, i64>(4)? as u32,
                y: row.get::<_, i64>(5)? as u32,
                width: row.get::<_, i64>(6)? as u32,
                height: row.get::<_, i64>(7)? as u32,
                image_width: row.get::<_, i64>(8)? as u32,
                image_height: row.get::<_, i64>(9)? as u32,
                world_name: row.get(10)?,
                captured_at: row.get(11)?,
                confidence: row.get(12)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tags)
}

pub fn save_positional_person_tag(
    db_path: &Path,
    screenshot_path: &Path,
    person_name: &str,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<PositionalPersonTag> {
    let person_name = person_name.trim();
    if person_name.is_empty() || person_name.chars().count() > 80 {
        anyhow::bail!("person name must be 1-80 characters");
    }
    if width == 0 || height == 0 {
        anyhow::bail!("person bounding box must have a size");
    }
    let image = decode_image(screenshot_path)
        .with_context(|| format!("failed to decode screenshot {}", screenshot_path.display()))?
        .into_rgb8();
    let (image_width, image_height) = image.dimensions();
    if x >= image_width
        || y >= image_height
        || width > image_width.saturating_sub(x)
        || height > image_height.saturating_sub(y)
    {
        anyhow::bail!("person bounding box is outside the screenshot");
    }

    let meta = extract_image_meta(screenshot_path)?;
    let crop_path = manual_crop_path(db_path, screenshot_path, (x, y, width, height));
    write_avatar_crop(&image, (x, y, width, height), &crop_path)?;

    let connection = recognition_connection(db_path)?;
    connection.execute(
        "INSERT INTO avatar_people(display_name,created_at,last_seen,confidence)
         VALUES(?,?,?,1)
         ON CONFLICT(display_name) DO UPDATE SET last_seen=excluded.last_seen,
         confidence=MAX(avatar_people.confidence,excluded.confidence)",
        params![
            person_name,
            chrono::Utc::now().timestamp(),
            meta.captured_at
        ],
    )?;
    connection.execute(
        "INSERT INTO positional_person_tags(
             screenshot_path,person_name,crop_path,x,y,width,height,image_width,image_height,
             world_name,captured_at,confidence,created_at
         ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(screenshot_path,person_name,x,y,width,height)
         DO UPDATE SET crop_path=excluded.crop_path,world_name=excluded.world_name,
             captured_at=excluded.captured_at,confidence=excluded.confidence",
        params![
            screenshot_path.to_string_lossy(),
            person_name,
            crop_path.to_string_lossy(),
            x,
            y,
            width,
            height,
            image_width,
            image_height,
            meta.world_name,
            meta.captured_at,
            1.0_f32,
            chrono::Utc::now().timestamp()
        ],
    )?;
    let tag_id: i64 = connection.query_row(
        "SELECT id FROM positional_person_tags
         WHERE screenshot_path=? AND person_name=? AND x=? AND y=? AND width=? AND height=?",
        params![
            screenshot_path.to_string_lossy(),
            person_name,
            x,
            y,
            width,
            height
        ],
        |row| row.get(0),
    )?;
    Ok(PositionalPersonTag {
        id: tag_id,
        screenshot_path: screenshot_path.to_string_lossy().into_owned(),
        person_name: person_name.to_owned(),
        crop_path: crop_path.to_string_lossy().into_owned(),
        x,
        y,
        width,
        height,
        image_width,
        image_height,
        world_name: meta.world_name,
        captured_at: meta.captured_at,
        confidence: 1.0,
    })
}

pub fn delete_positional_person_tag(db_path: &Path, tag_id: i64) -> Result<()> {
    let connection = recognition_connection(db_path)?;
    let deleted = connection.execute("DELETE FROM positional_person_tags WHERE id=?", [tag_id])?;
    if deleted == 0 {
        anyhow::bail!("person tag does not exist");
    }
    Ok(())
}

pub fn update_positional_person_tag(
    db_path: &Path,
    tag_id: i64,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<PositionalPersonTag> {
    let connection = recognition_connection(db_path)?;
    let (screenshot_path, person_name): (String, String) = connection.query_row(
        "SELECT screenshot_path,person_name FROM positional_person_tags WHERE id=?",
        [tag_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    // Save first so a failed update never loses the existing tag.
    let updated = save_positional_person_tag(
        db_path,
        Path::new(&screenshot_path),
        &person_name,
        x,
        y,
        width,
        height,
    )?;
    if updated.id != tag_id {
        connection.execute("DELETE FROM positional_person_tags WHERE id=?", [tag_id])?;
    }
    Ok(updated)
}

const TAG_STORE_FILE: &str = ".vrchat-organizer-tags.json";
const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const ORGANIZER_TAG_PREFIX: &[u8] = b"VRChat Organizer Participants\0";
static TAG_STORE_LOCK: Mutex<()> = Mutex::new(());
const MAX_PARTICIPANTS: usize = 256;

fn is_valid_person_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name.chars().count() <= 80
}

fn tag_store_path(base_path: &Path) -> PathBuf {
    base_path.join(TAG_STORE_FILE)
}

/// Tag-store key: SHA-256 of the PNG signature and every chunk up to and
/// including IEND, minus the organizer's own participant tEXt chunk. Streams
/// the file; the output must stay byte-identical or every stored tag is lost.
fn normalized_png_fingerprint(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let length = reader.get_ref().metadata()?.len();
    let mut signature = [0u8; 8];
    if length < 8 || reader.read_exact(&mut signature).is_err() || signature != PNG_SIGNATURE {
        anyhow::bail!("person tagging currently supports PNG screenshots only");
    }
    let mut hasher = Sha256::new();
    hasher.update(signature);
    let mut offset = 8_u64;
    while offset + 12 <= length {
        let mut header = [0u8; 8];
        reader.read_exact(&mut header)?;
        let chunk_len = u32::from_be_bytes(header[..4].try_into().unwrap()) as u64;
        let end = offset + 12 + chunk_len;
        if end > length {
            anyhow::bail!("malformed PNG: chunk exceeds file size");
        }
        let mut prefix = vec![0u8; chunk_len.min(ORGANIZER_TAG_PREFIX.len() as u64) as usize];
        reader.read_exact(&mut prefix)?;
        let rest = chunk_len + 4 - prefix.len() as u64;
        if &header[4..] == b"tEXt" && prefix == ORGANIZER_TAG_PREFIX {
            reader.seek_relative(rest as i64)?;
        } else {
            hasher.update(header);
            hasher.update(&prefix);
            if std::io::copy(&mut (&mut reader).take(rest), &mut hasher)? != rest {
                anyhow::bail!("PNG changed while it was read");
            }
        }
        offset = end;
        if &header[4..] == b"IEND" {
            break;
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn load_tag_store(base_path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let path = tag_store_path(base_path);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    match serde_json::from_str::<HashMap<String, Vec<String>>>(&fs::read_to_string(&path)?) {
        Ok(mut store) => {
            // Drop anything the app itself would never have written.
            store.retain(|key, names| {
                names.retain(|name| is_valid_person_name(name));
                names.truncate(MAX_PARTICIPANTS);
                key.len() == 64
                    && key.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && !names.is_empty()
            });
            Ok(store)
        }
        Err(error) => {
            let backup = path.with_extension(format!("json.corrupt.{}", temp_suffix()));
            fs::rename(&path, &backup).with_context(|| {
                format!(
                    "tag store is corrupt ({error}) and could not be backed up to {}",
                    backup.display()
                )
            })?;
            eprintln!(
                "tag store is corrupt; preserved original at {}",
                backup.display()
            );
            Ok(HashMap::new())
        }
    }
}

fn save_tag_store(base_path: &Path, store: &HashMap<String, Vec<String>>) -> Result<()> {
    write_atomic(
        &tag_store_path(base_path),
        &serde_json::to_vec_pretty(store)?,
    )
}

fn lock_tag_store() -> std::sync::MutexGuard<'static, ()> {
    TAG_STORE_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

pub fn set_photo_tags(
    base_path: &Path,
    path: &Path,
    participants: &[String],
) -> Result<Vec<String>> {
    let key = normalized_png_fingerprint(path)?;
    let _guard = lock_tag_store();
    store_photo_tags(base_path, key, participants)
}

/// Replace one photo's tags. Callers must hold `TAG_STORE_LOCK`.
fn store_photo_tags(base_path: &Path, key: String, participants: &[String]) -> Result<Vec<String>> {
    let mut store = load_tag_store(base_path)?;
    let mut names = Vec::new();
    for participant in participants {
        let name = participant.trim();
        if name.is_empty() || name.chars().count() > 80 {
            anyhow::bail!("participant name must be 1-80 characters");
        }
        if !names
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(name))
        {
            names.push(name.to_owned());
        }
    }
    if names.len() > MAX_PARTICIPANTS {
        anyhow::bail!("a photo can have at most {MAX_PARTICIPANTS} people");
    }
    if names.is_empty() {
        store.remove(&key);
    } else {
        store.insert(key, names.clone());
    }
    save_tag_store(base_path, &store)?;
    Ok(names)
}

pub fn tag_photo_participant_in_store(
    base_path: &Path,
    path: &Path,
    participant: &str,
) -> Result<Vec<String>> {
    let key = normalized_png_fingerprint(path)?;
    let _guard = lock_tag_store();
    let mut names = load_tag_store(base_path)?.remove(&key).unwrap_or_default();
    if !names
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(participant.trim()))
    {
        names.push(participant.trim().to_owned());
    }
    store_photo_tags(base_path, key, &names)
}

fn ensure_cache_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS cache_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;
    let schema_version = connection
        .query_row(
            "SELECT value FROM cache_metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if schema_version.as_deref() != Some("2") {
        connection.execute_batch("DROP TABLE IF EXISTS photos;")?;
        connection.execute(
            "INSERT OR REPLACE INTO cache_metadata (key, value) VALUES ('schema_version', '2')",
            [],
        )?;
    }
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS photos (
            path TEXT PRIMARY KEY,
            size_bytes INTEGER NOT NULL,
            modified_secs INTEGER NOT NULL,
            world_name TEXT,
            captured_at INTEGER,
            participants TEXT NOT NULL,
            tagged_participants TEXT NOT NULL DEFAULT '[]',
            fingerprint TEXT,
            width INTEGER NOT NULL,
            height INTEGER NOT NULL,
            thumbnail_path TEXT NOT NULL
        );",
    )?;
    for statement in [
        "ALTER TABLE photos ADD COLUMN tagged_participants TEXT NOT NULL DEFAULT '[]'",
        "ALTER TABLE photos ADD COLUMN fingerprint TEXT",
    ] {
        if let Err(error) = connection.execute_batch(statement) {
            if !error.to_string().contains("duplicate column name") {
                return Err(error.into());
            }
        }
    }
    Ok(())
}

fn load_cache(path: &Path) -> Result<HashMap<String, CachedPhoto>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let connection = Connection::open(path)?;
    ensure_cache_schema(&connection)?;
    let mut statement = connection.prepare(
        "SELECT path,size_bytes,modified_secs,fingerprint,world_name,captured_at,participants,tagged_participants,width,height,thumbnail_path FROM photos",
    )?;
    let mut rows = statement.query([])?;
    let mut result = HashMap::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let participants =
            serde_json::from_str(row.get::<_, String>(6)?.as_str()).unwrap_or_default();
        let tagged_participants =
            serde_json::from_str(row.get::<_, String>(7)?.as_str()).unwrap_or_default();
        result.insert(
            path.clone(),
            CachedPhoto {
                size_bytes: row.get::<_, i64>(1)?.max(0) as u64,
                modified_secs: row.get::<_, i64>(2)?.max(0) as u64,
                fingerprint: row.get(3)?,
                photo: LibraryPhoto {
                    path,
                    thumbnail_path: row.get(10)?,
                    captured_at: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
                    world_name: row.get(4)?,
                    participants,
                    tagged_participants,
                    width: row.get::<_, i64>(8)?.max(0) as u32,
                    height: row.get::<_, i64>(9)?.max(0) as u32,
                    size_bytes: row.get::<_, i64>(1)?.max(0) as u64,
                },
            },
        );
    }
    Ok(result)
}

pub fn cached_library(
    path: &Path,
    base_path: &Path,
    scan_all_months: bool,
) -> Result<Option<OrganizerStats>> {
    if !path.exists() {
        return Ok(None);
    }
    let cached = load_cache(path)?;
    if cached.is_empty() {
        return Ok(None);
    }
    let roots = library_roots(base_path, scan_all_months)?;
    let mut worlds: HashMap<String, Vec<LibraryPhoto>> = HashMap::new();
    let mut unorganized = Vec::new();
    for entry in cached.into_values().filter(|entry| {
        let photo_path = Path::new(&entry.photo.path);
        photo_path.starts_with(base_path) && roots.iter().any(|root| photo_path.starts_with(root))
    }) {
        let relative = Path::new(&entry.photo.path)
            .strip_prefix(base_path)
            .unwrap_or_else(|_| Path::new(""));
        let cached_relative = if !scan_all_months {
            let components: Vec<_> = relative.components().collect();
            if components
                .first()
                .and_then(|component| component.as_os_str().to_str())
                .map(is_month_folder)
                .unwrap_or(false)
            {
                components[1..].iter().collect::<PathBuf>()
            } else {
                relative.to_path_buf()
            }
        } else {
            relative.to_path_buf()
        };
        if let Some(world) = classify_world(
            &cached_relative,
            entry.photo.world_name.as_deref(),
            scan_all_months,
            2,
        ) {
            worlds.entry(world).or_default().push(entry.photo);
        } else {
            unorganized.push(entry.photo);
        }
    }
    let mut stats = OrganizerStats::default();
    for (name, mut photos) in worlds {
        if photos.len() < 2 {
            unorganized.append(&mut photos);
            continue;
        }
        photos.sort_by_key(|photo| std::cmp::Reverse(photo.captured_at.unwrap_or(0)));
        stats.total += photos.len();
        stats.worlds.insert(name.clone(), photos.len());
        stats.world_details.push(LibraryWorld {
            name,
            last_capture: photos.first().and_then(|photo| photo.captured_at),
            photos,
        });
    }
    unorganized.sort_by_key(|photo| std::cmp::Reverse(photo.captured_at.unwrap_or(0)));
    stats.total += unorganized.len();
    stats.unorganized_photos = unorganized;
    stats
        .world_details
        .sort_by_key(|world| std::cmp::Reverse(world.last_capture.unwrap_or(0)));
    Ok(Some(stats))
}

/// (world, photo, modified_secs, tag-store fingerprint) for one scanned file.
type ScannedPhoto = (Option<String>, LibraryPhoto, u64, Option<String>);

/// Write new or changed rows and drop rows for files that vanished from the
/// roots just scanned (rows for other months/libraries are kept).
fn save_cache(
    path: &Path,
    candidates: &[ScannedPhoto],
    cached: &HashMap<String, CachedPhoto>,
    roots: &[PathBuf],
    base_path: &Path,
    scan_all_months: bool,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut connection = Connection::open(path)?;
    ensure_cache_schema(&connection)?;
    let transaction = connection.transaction()?;
    {
        let mut insert = transaction.prepare(
            "INSERT OR REPLACE INTO photos
             (path,size_bytes,modified_secs,world_name,captured_at,participants,tagged_participants,fingerprint,width,height,thumbnail_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )?;
        for (_, photo, modified_secs, fingerprint) in candidates {
            let unchanged = cached.get(&photo.path).is_some_and(|entry| {
                entry.size_bytes == photo.size_bytes
                    && entry.modified_secs == *modified_secs
                    && entry.fingerprint == *fingerprint
                    && entry.photo.tagged_participants == photo.tagged_participants
            });
            if unchanged {
                continue;
            }
            insert.execute(params![
                photo.path,
                photo.size_bytes as i64,
                *modified_secs as i64,
                photo.world_name,
                photo.captured_at.map(|value| value as i64),
                serde_json::to_string(&photo.participants)?,
                serde_json::to_string(&photo.tagged_participants)?,
                fingerprint,
                photo.width as i64,
                photo.height as i64,
                photo.thumbnail_path,
            ])?;
        }
        let scanned: std::collections::HashSet<&str> = candidates
            .iter()
            .map(|(_, photo, _, _)| photo.path.as_str())
            .collect();
        let mut delete = transaction.prepare("DELETE FROM photos WHERE path = ?1")?;
        for stale in cached
            .keys()
            .filter(|path| !scanned.contains(path.as_str()))
        {
            let stale_path = Path::new(stale);
            let in_scanned_root = roots.iter().any(|root| {
                if !scan_all_months && root == base_path {
                    stale_path.parent() == Some(root.as_path())
                } else {
                    stale_path.starts_with(root)
                }
            });
            if in_scanned_root {
                delete.execute([stale])?;
            }
        }
    }
    transaction.commit()?;
    Ok(())
}

fn scan_library_partition_with_options(
    base_path: &Path,
    scan_all_months: bool,
    cache_path: Option<&Path>,
) -> Result<(Vec<LibraryWorld>, Vec<LibraryPhoto>)> {
    if !base_path.is_dir() {
        anyhow::bail!("path does not exist: {}", base_path.display());
    }

    let scan_started = std::time::SystemTime::now();
    let roots = library_roots(base_path, scan_all_months)?;
    let cached = cache_path.map(load_cache).transpose()?.unwrap_or_default();
    let tag_store = load_tag_store(base_path)?;
    let mut files = Vec::new();
    for root in &roots {
        // Only the chosen library folder itself may be a symlink.
        let walker = WalkDir::new(root).follow_root_links(root == base_path);
        let walker = if !scan_all_months && root == base_path {
            walker.max_depth(1)
        } else {
            walker
        };
        for item in walker
            .into_iter()
            .filter_map(|item| item.ok())
            .filter(|item| item.file_type().is_file())
            .filter(|item| is_image_path(item.path()))
            .filter(|item| !is_ignored_path(item.path(), base_path))
        {
            let Ok(metadata) = item.metadata() else {
                continue;
            };
            let modified_secs = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or_default();
            files.push((item.into_path(), metadata.len(), modified_secs, root));
        }
    }
    let candidates: Vec<ScannedPhoto> = files
        .par_iter()
        .map(|(path, size_bytes, modified_secs, root)| {
            let hit = cached.get(path.to_string_lossy().as_ref()).filter(|entry| {
                entry.size_bytes == *size_bytes && entry.modified_secs == *modified_secs
            });
            let (fingerprint, mut photo) = if let Some(entry) = hit {
                let fingerprint = entry
                    .fingerprint
                    .clone()
                    .or_else(|| normalized_png_fingerprint(path).ok());
                let mut photo = entry.photo.clone();
                if let Some(key) = fingerprint.as_ref() {
                    photo.tagged_participants = tag_store.get(key).cloned().unwrap_or_default();
                }
                (fingerprint, photo)
            } else {
                let metadata = extract_image_meta(path).ok();
                let fingerprint = normalized_png_fingerprint(path).ok();
                let mut photo = LibraryPhoto {
                    path: path.to_string_lossy().into_owned(),
                    thumbnail_path: String::new(),
                    captured_at: metadata
                        .as_ref()
                        .and_then(|meta| meta.captured_at)
                        .or(Some(*modified_secs)),
                    world_name: metadata.as_ref().and_then(|meta| meta.world_name.clone()),
                    participants: metadata
                        .as_ref()
                        .map(|meta| meta.participants.clone())
                        .unwrap_or_default(),
                    tagged_participants: metadata
                        .as_ref()
                        .map(|meta| meta.tagged_participants.clone())
                        .unwrap_or_default(),
                    width: metadata.as_ref().map(|meta| meta.width).unwrap_or_default(),
                    height: metadata
                        .as_ref()
                        .map(|meta| meta.height)
                        .unwrap_or_default(),
                    size_bytes: *size_bytes,
                };
                if let Some(tags) = fingerprint.as_ref().and_then(|key| tag_store.get(key)) {
                    photo.tagged_participants = tags.clone();
                }
                (fingerprint, photo)
            };
            if photo.thumbnail_path.is_empty() {
                photo.thumbnail_path = thumbnail_path_for(path, base_path, Some(*modified_secs))
                    .to_string_lossy()
                    .into_owned();
            }
            let relative = path.strip_prefix(root).unwrap_or(path);
            let world = classify_world(relative, photo.world_name.as_deref(), scan_all_months, 2);
            (world, photo, *modified_secs, fingerprint)
        })
        .collect();

    // Move tags still embedded in legacy PNG metadata into the tag store.
    let legacy: Vec<(&String, &Vec<String>)> = candidates
        .iter()
        .filter_map(|(_, photo, _, fingerprint)| {
            Some((fingerprint.as_ref()?, &photo.tagged_participants))
        })
        .filter(|(key, tags)| !tags.is_empty() && !tag_store.contains_key(*key))
        .collect();
    if !legacy.is_empty() {
        let _guard = lock_tag_store();
        let mut store = load_tag_store(base_path)?;
        let before = store.len();
        for (key, tags) in legacy {
            store.entry(key.clone()).or_insert_with(|| tags.clone());
        }
        if store.len() != before {
            save_tag_store(base_path, &store)?;
        }
    }
    if let Some(cache_path) = cache_path {
        save_cache(
            cache_path,
            &candidates,
            &cached,
            &roots,
            base_path,
            scan_all_months,
        )?;
    }
    if scan_all_months {
        remove_orphan_thumbnails(base_path, &candidates, scan_started);
    }

    let mut worlds: HashMap<String, Vec<LibraryPhoto>> = HashMap::new();
    let mut unorganized_photos = Vec::new();
    for (world, photo, _, _) in candidates {
        if let Some(world) = world {
            worlds.entry(world).or_default().push(photo);
        } else {
            unorganized_photos.push(photo);
        }
    }
    let mut qualifying_worlds = HashMap::new();
    for (name, photos) in worlds {
        if photos.len() >= 2 {
            qualifying_worlds.insert(name, photos);
        } else {
            unorganized_photos.extend(photos);
        }
    }
    let mut result: Vec<LibraryWorld> = qualifying_worlds
        .into_iter()
        .map(|(name, mut photos)| {
            photos.sort_by_key(|photo| std::cmp::Reverse(photo.captured_at.unwrap_or(0)));
            let last_capture = photos.first().and_then(|photo| photo.captured_at);
            LibraryWorld {
                name,
                photos,
                last_capture,
            }
        })
        .collect();
    result.sort_by_key(|world| std::cmp::Reverse(world.last_capture.unwrap_or(0)));
    unorganized_photos.sort_by_key(|photo| std::cmp::Reverse(photo.captured_at.unwrap_or(0)));
    Ok((result, unorganized_photos))
}

/// Delete cached thumbnails no scanned photo refers to any more (moved,
/// edited or deleted photos). Only valid after a scan of every month.
fn remove_orphan_thumbnails(
    base_path: &Path,
    candidates: &[ScannedPhoto],
    scan_started: std::time::SystemTime,
) {
    let cache = base_path.join(THUMBNAIL_DIR);
    if candidates.is_empty() || refuse_symlink(&cache).is_err() {
        return;
    }
    let referenced: std::collections::HashSet<&std::ffi::OsStr> = candidates
        .iter()
        .filter_map(|(_, photo, _, _)| Path::new(&photo.thumbnail_path).file_name())
        .collect();
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        // Thumbnails written while the scan ran may belong to newer photos.
        let older = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified < scan_started);
        let path = entry.path();
        if older
            && path.extension().is_some_and(|extension| extension == "jpg")
            && path
                .file_name()
                .is_some_and(|name| !referenced.contains(name))
        {
            let _ = fs::remove_file(path);
        }
    }
}

fn library_roots(base_path: &Path, scan_all_months: bool) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for entry in fs::read_dir(base_path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && is_month_folder(&entry.file_name().to_string_lossy()) {
            roots.push(entry.path());
        }
    }
    if scan_all_months {
        // Scan from the selected root so root-level captures and every month
        // folder are indexed in one traversal.
        return Ok(vec![base_path.to_path_buf()]);
    }
    let has_month_folders = !roots.is_empty();
    if !has_month_folders {
        return Ok(vec![base_path.to_path_buf()]);
    }
    roots.sort();
    roots.dedup();
    if let Some(latest) = roots.pop() {
        roots.clear();
        roots.push(base_path.to_path_buf());
        roots.push(latest);
    }
    Ok(roots)
}

fn thumbnail_for(path: &Path, base_path: &Path, modified: Option<u64>) -> Result<PathBuf> {
    let target = thumbnail_path_for(path, base_path, modified);
    let cache = base_path.join(THUMBNAIL_DIR);
    refuse_symlink(&cache)?;
    fs::create_dir_all(&cache)?;
    if target
        .metadata()
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
    {
        return Ok(target);
    }

    let thumbnail = match decode_image(path).ok() {
        Some(image) => {
            let rgb = image.into_rgb8();
            let scale = (320.0 / rgb.width() as f32)
                .min(320.0 / rgb.height() as f32)
                .min(1.0);
            let width = ((rgb.width() as f32 * scale).round() as u32).max(1);
            let height = ((rgb.height() as f32 * scale).round() as u32).max(1);
            image::DynamicImage::ImageRgb8(resize_rgb(&rgb, None, width, height)?)
        }
        None => image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            320,
            180,
            image::Rgb([32, 26, 48]),
        )),
    };
    let temporary = target.with_extension(format!("jpg.tmp.{}", temp_suffix()));
    let written = (|| -> Result<()> {
        let mut output = fs::File::create(&temporary)?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 75)
            .encode_image(&thumbnail)?;
        output.sync_all()?;
        fs::rename(&temporary, &target)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written.map(|()| target)
}

pub fn generate_thumbnail(path: &Path, base_path: &Path) -> Result<PathBuf> {
    let modified = fs::metadata(path)?
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    thumbnail_for(path, base_path, modified)
}

const THUMBNAIL_DIR: &str = "vrchat-organizer-thumbnails";

pub fn thumbnail_path_for(path: &Path, base_path: &Path, modified: Option<u64>) -> PathBuf {
    let cache = base_path.join("vrchat-organizer-thumbnails");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "thumbnail-v2-320".hash(&mut hasher);
    path.to_string_lossy().hash(&mut hasher);
    modified.hash(&mut hasher);
    cache.join(format!("{:x}.jpg", hasher.finish()))
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            )
        })
        .unwrap_or(false)
}

pub fn is_ignored_path(path: &Path, base_path: &Path) -> bool {
    let relative = path.strip_prefix(base_path).unwrap_or(path);
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name == "Prints" || name == "vrchat-organizer-thumbnails" || name.starts_with('.')
    })
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
#[serde(rename_all = "camelCase")]
pub struct OrganizerStats {
    pub processed: usize,
    pub organized: usize,
    pub already_organized: usize,
    pub no_metadata: usize,
    pub errors: usize,
    pub undone: usize,
    pub total: usize,
    pub worlds: HashMap<String, usize>,
    #[serde(default)]
    pub world_details: Vec<LibraryWorld>,
    #[serde(default)]
    pub unorganized_photos: Vec<LibraryPhoto>,
    pub preview: Vec<String>,
    #[serde(default)]
    pub organized_photos: Vec<OrganizedPhoto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizedPhoto {
    pub photo_name: String,
    pub folder_name: String,
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
    write_atomic(&path, serde_json::to_string_pretty(&entries)?.as_bytes())
}

pub fn read_activity_log(base_path: &Path) -> Result<Vec<ActivityEntry>> {
    let path = base_path.join(".vrchat-organizer-activity.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&fs::read_to_string(&path)?)
        .with_context(|| format!("activity log is corrupt: {}", path.display()))
}

const UNDO_FILE: &str = ".vrchat-organizer-undo.json";

/// Unique suffix for temporary files written next to their destination.
fn temp_suffix() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos());
    format!(
        "{}.{nanos}{:04}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed) % 10_000
    )
}

/// Refuse to write app files through a symlink planted in the library.
fn refuse_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        anyhow::bail!("refusing to write through symlink {}", path.display());
    }
    Ok(())
}

/// Create a brand-new file (never opens an existing file or symlink).
fn create_new_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Replace `path` with `data` via a fresh temp file + rename.
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    refuse_symlink(path)?;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp.{}", temp_suffix()));
    let tmp_path = path.with_file_name(name);
    let written = create_new_file(&tmp_path)
        .and_then(|mut file| file.write_all(data))
        .and_then(|()| fs::rename(&tmp_path, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    Ok(written?)
}

/// Write the undo log as JSON Lines; `raw` lines (unparseable) are kept as-is.
fn write_undo_log(undo_path: &Path, entries: &[UndoEntry], raw: &[String]) -> Result<()> {
    let mut data = Vec::new();
    for entry in entries {
        serde_json::to_writer(&mut data, entry)?;
        data.push(b'\n');
    }
    for line in raw {
        data.extend_from_slice(line.as_bytes());
        data.push(b'\n');
    }
    write_atomic(undo_path, &data)
}

/// Append one entry (a JSON line) to the undo log in the base folder. A
/// legacy JSON-array log is converted to JSON Lines first.
pub fn log_undo_entry(base_path: &Path, entry: &UndoEntry) -> Result<()> {
    let undo_path = base_path.join(UNDO_FILE);
    refuse_symlink(&undo_path)?;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(&undo_path)?;
    let mut line = Vec::new();
    if file.metadata()?.len() > 0 {
        let mut byte = [0u8];
        file.read_exact(&mut byte)?;
        if byte[0] == b'[' {
            drop(file);
            write_undo_log(&undo_path, &read_undo_log(base_path)?, &[])?;
            return log_undo_entry(base_path, entry);
        }
        // Never glue a new entry onto a line torn by a crash.
        file.seek(std::io::SeekFrom::End(-1))?;
        file.read_exact(&mut byte)?;
        if byte[0] != b'\n' {
            line.push(b'\n');
        }
    }
    serde_json::to_writer(&mut line, entry)?;
    line.push(b'\n');
    file.write_all(&line)?;
    Ok(())
}

/// Undo entries plus any unparseable (e.g. crash-torn) raw lines.
fn read_undo_entries(base_path: &Path) -> Result<(Vec<UndoEntry>, Vec<String>)> {
    let undo_path = base_path.join(UNDO_FILE);
    if !undo_path.exists() {
        return Ok(Default::default());
    }
    let data = fs::read_to_string(&undo_path)?;
    if data.trim_start().starts_with('[') {
        let entries = serde_json::from_str(&data)
            .with_context(|| format!("undo log is corrupt: {}", undo_path.display()))?;
        return Ok((entries, Vec::new()));
    }
    let (mut entries, mut raw) = (Vec::new(), Vec::new());
    for line in data.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => raw.push(line.to_owned()),
        }
    }
    Ok((entries, raw))
}

/// Read the undo log (JSON Lines, or a legacy JSON array) from the base folder.
pub fn read_undo_log(base_path: &Path) -> Result<Vec<UndoEntry>> {
    Ok(read_undo_entries(base_path)?.0)
}

/// True if `path` lies under the canonical `base` with no symlink among its
/// existing components below `base`.
fn inside_without_symlinks(base: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(base) else {
        return false;
    };
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    true
}

/// Move one undo entry back, refusing anything that would leave the library.
fn restore_undo_entry(base: &Path, entry: &UndoEntry) -> Result<()> {
    let dest = Path::new(&entry.destination_path);
    let orig = Path::new(&entry.original_path);
    let parent = orig.parent().context("original path has no parent")?;
    if !inside_without_symlinks(base, dest)
        || !inside_without_symlinks(base, orig)
        || !dest.canonicalize()?.starts_with(base)
    {
        anyhow::bail!("entry points outside the library");
    }
    if fs::symlink_metadata(orig).is_ok() {
        anyhow::bail!("original path is occupied");
    }
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(base) {
        anyhow::bail!("entry points outside the library");
    }
    move_file(dest, orig)
}

/// Undo the last organization run by moving files back to their original paths.
/// Entries that cannot be restored (or point outside `base_path`) stay in the
/// log for a later retry; nothing is ever overwritten.
pub fn undo_organization(base_path: &Path, stats: &mut OrganizerStats) -> Result<()> {
    let started = std::time::Instant::now();
    let (entries, raw) = read_undo_entries(base_path)?;
    if entries.is_empty() {
        anyhow::bail!("no undo entries found");
    }
    let base = base_path.canonicalize()?;
    stats.errors += raw.len();

    let mut remaining = Vec::new();
    for entry in entries {
        if let Err(error) = restore_undo_entry(&base, &entry) {
            eprintln!(
                "could not restore {} to {}: {error:#}",
                entry.destination_path, entry.original_path
            );
            stats.errors += 1;
            remaining.push(entry);
            continue;
        }
        stats.undone += 1;
        if let Some(parent) = Path::new(&entry.destination_path).parent() {
            let _ = fs::remove_dir(parent);
        }
        println!(
            "undone: moved {} back to {}",
            entry.destination_path, entry.original_path
        );
    }

    let undo_path = base_path.join(UNDO_FILE);
    if remaining.is_empty() && raw.is_empty() {
        let _ = fs::remove_file(&undo_path);
    } else {
        write_undo_log(&undo_path, &remaining, &raw)?;
    }

    let logged = log_activity(
        base_path,
        &ActivityEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            operation: "undo".to_string(),
            status: if stats.errors == 0 {
                "success".to_string()
            } else {
                "warning".to_string()
            },
            duration_ms: started.elapsed().as_millis(),
            files_affected: stats.undone,
            details: format!("restored={}, errors={}", stats.undone, stats.errors),
        },
    );
    if let Err(error) = logged {
        eprintln!("warning: could not write activity log: {error:#}");
    }
    Ok(())
}

/// Move `src` to `dst` without ever replacing an existing `dst`. On any error
/// the source is left exactly as it was and nothing is left at `dst`.
pub fn move_file(src: &Path, dst: &Path) -> anyhow::Result<()> {
    match fs::hard_link(src, dst) {
        Ok(()) => remove_source(src, dst),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(error.into()),
        // Hard links unsupported (FAT/exFAT) or another device: copy instead.
        Err(_) => copy_file_no_clobber(src, dst),
    }
}

/// Final step of a move: drop the source name, or undo the new name if that fails.
fn remove_source(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::remove_file(src).map_err(|error| {
        let _ = fs::remove_file(dst);
        error.into()
    })
}

/// Only `.<image name>.copying.<pid>.<n>` (as written below) counts as our temp file,
/// so cleanup can never delete a user's own file.
fn is_copy_temp_name(name: &str) -> bool {
    let Some((image, suffix)) = name
        .strip_prefix('.')
        .and_then(|rest| rest.rsplit_once(".copying."))
    else {
        return false;
    };
    let mut parts = suffix.split('.');
    let numeric = |part: Option<&str>| {
        part.is_some_and(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
    };
    numeric(parts.next())
        && numeric(parts.next())
        && parts.next().is_none()
        && is_image_path(Path::new(image))
}

fn copy_file_no_clobber(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let parent = dst
        .parent()
        .ok_or_else(|| anyhow::anyhow!("destination has no parent"))?;
    let tmp = parent.join(format!(
        ".{}.copying.{}",
        dst.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("screenshot"),
        temp_suffix()
    ));
    let result = (|| -> anyhow::Result<()> {
        let mut output = create_new_file(&tmp)?;
        let copied = std::io::copy(&mut fs::File::open(src)?, &mut output)?;
        output.sync_all()?;
        output.set_permissions(fs::metadata(src)?.permissions())?;
        drop(output);
        if copied != fs::metadata(src)?.len() || fs::metadata(&tmp)?.len() != copied {
            anyhow::bail!("copy of {} is incomplete", src.display());
        }
        match fs::hard_link(&tmp, dst) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(error.into()),
            Err(_) => {
                // ponytail: exists-check + rename has a tiny race window; only
                // used where the filesystem has no hard links at all.
                if fs::symlink_metadata(dst).is_ok() {
                    anyhow::bail!("destination already exists: {}", dst.display());
                }
                Ok(fs::rename(&tmp, dst)?)
            }
        }
    })();
    let _ = fs::remove_file(&tmp);
    result?;
    remove_source(src, dst)
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
    } else if is_windows_reserved_name(&cleaned)
        || is_month_folder(&cleaned)
        || cleaned.eq_ignore_ascii_case("Prints")
        || cleaned.eq_ignore_ascii_case(THUMBNAIL_DIR)
    {
        // Reserved by the app's own layout (a world named "2025-01" would
        // otherwise be read as a month folder and nest deeper every run).
        format!("{cleaned}_")
    } else {
        cleaned
    }
}

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default();
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
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

/// Inflate a zTXt/iTXt payload to at most `limit` bytes.
fn inflate_text(compressed: &[u8], limit: u64) -> String {
    let mut value = String::new();
    let _ = ZlibDecoder::new(compressed)
        .take(limit)
        .read_to_string(&mut value);
    value
}

/// Per-chunk cap; a crafted PNG cannot exhaust memory through text chunks.
const MAX_INFLATED_TEXT: u64 = 1 << 20;
const MAX_TEXT_TOTAL: u64 = 4 << 20;
const MAX_TEXT_CHUNKS: usize = 64;
const TEXT_KEYWORDS: [&str; 7] = [
    "description",
    "xml:com.adobe.xmp",
    "comment",
    "software",
    "creator tool",
    "creator_tool",
    "vrchat organizer participants",
];

/// Read PNG text chunks. VRChat writes them before IDAT; other writers may
/// put them after it, so pixel data is seeked over until text is found.
fn extract_png_text_chunks(path: &Path) -> Result<Vec<PngTextEntry>> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut signature = [0u8; 8];
    reader.read_exact(&mut signature)?;
    if signature != PNG_SIGNATURE {
        anyhow::bail!("not a valid PNG file");
    }
    let mut entries = Vec::new();
    let (mut text_chunks, mut budget) = (0, MAX_TEXT_TOTAL);
    loop {
        let mut header = [0u8; 8];
        if reader.read_exact(&mut header).is_err() {
            break;
        }
        let chunk_len = u32::from_be_bytes(header[..4].try_into().unwrap()) as u64;
        let chunk_type = &header[4..8];
        if chunk_type == b"IEND" || (chunk_type == b"IDAT" && !entries.is_empty()) {
            break;
        }
        if chunk_type == b"tEXt" || chunk_type == b"zTXt" || chunk_type == b"iTXt" {
            if chunk_len > 16 * 1024 * 1024 {
                anyhow::bail!("PNG text chunk is too large");
            }
            text_chunks += 1;
            if text_chunks > MAX_TEXT_CHUNKS || budget == 0 {
                break;
            }
            // Keywords are at most 79 bytes; only read and inflate known ones.
            let mut data = vec![0u8; chunk_len.min(80) as usize];
            reader.read_exact(&mut data)?;
            let null_pos = data.iter().position(|&byte| byte == 0).filter(|&end| {
                chunk_len <= MAX_INFLATED_TEXT
                    && TEXT_KEYWORDS.iter().any(|known| {
                        known.eq_ignore_ascii_case(&String::from_utf8_lossy(&data[..end]))
                    })
            });
            if null_pos.is_some() {
                let head = data.len();
                data.resize(chunk_len as usize, 0);
                reader.read_exact(&mut data[head..])?;
            } else {
                reader.seek_relative((chunk_len - data.len() as u64) as i64)?;
            }
            if let Some(null_pos) = null_pos {
                let keyword = String::from_utf8_lossy(&data[..null_pos]).into_owned();
                let raw_value = &data[null_pos + 1..];
                let value = match chunk_type {
                    b"tEXt" => String::from_utf8(raw_value.to_vec())
                        .unwrap_or_else(|_| raw_value.iter().map(|&byte| byte as char).collect()),
                    b"zTXt" if !raw_value.is_empty() => {
                        inflate_text(&raw_value[1..], budget.min(MAX_INFLATED_TEXT))
                    }
                    b"iTXt" if raw_value.len() >= 2 => {
                        let after_flags = &raw_value[2..];
                        let after_lang = after_flags
                            .iter()
                            .position(|&byte| byte == 0)
                            .map(|index| &after_flags[index + 1..]);
                        let text = after_lang.and_then(|value| {
                            value
                                .iter()
                                .position(|&byte| byte == 0)
                                .map(|index| &value[index + 1..])
                        });
                        match text {
                            Some(text) if raw_value[0] == 0 => {
                                String::from_utf8_lossy(text).into_owned()
                            }
                            Some(text) => inflate_text(text, budget.min(MAX_INFLATED_TEXT)),
                            None => String::new(),
                        }
                    }
                    _ => String::new(),
                };
                budget = budget.saturating_sub(value.len() as u64);
                entries.push(PngTextEntry { keyword, value });
            }
        } else {
            reader.seek_relative(chunk_len as i64)?;
        }
        reader.seek_relative(4)?;
    }
    Ok(entries)
}

const MAX_WORLD_NAME: usize = 100;

/// World name and player names from VRChat's JSON description (capped);
/// `None` players means the JSON has no `players` list.
fn parse_vrchat_description(value: &str) -> Option<(Option<String>, Option<Vec<String>>)> {
    let parsed: serde_json::Value = serde_json::from_str(value).ok()?;
    let world = parsed
        .pointer("/world/name")
        .and_then(|name| name.as_str())
        .map(|name| sanitize_name(&name.chars().take(MAX_WORLD_NAME).collect::<String>()));
    let players = parsed
        .get("players")
        .and_then(|players| players.as_array())
        .map(|players| {
            players
                .iter()
                .filter_map(|player| player.get("displayName")?.as_str())
                .map(str::trim)
                .filter(|name| is_valid_person_name(name))
                .take(MAX_PARTICIPANTS)
                .map(ToOwned::to_owned)
                .collect()
        });
    Some((world, players))
}

/// World name from VRChat's own XMP (no player list there). VRChat swaps
/// filename-unsafe punctuation for lookalikes (`|` → `｜`, `.` → `․`); map them
/// back so these photos share a folder with VRCX-tagged ones of the same world.
fn parse_vrchat_xmp(xmp: &str) -> Option<String> {
    static XMP_WORLD_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = XMP_WORLD_RE.get_or_init(|| {
        Regex::new(
            r#"<vrc:WorldDisplayName>([^<]*)</vrc:WorldDisplayName>|vrc:WorldDisplayName="([^"]*)""#,
        )
        .unwrap()
    });
    let captures = re.captures(xmp)?;
    let raw = captures.get(1).or(captures.get(2))?.as_str();
    let name: String = raw
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
        .chars()
        .map(|ch| match ch {
            'ǃ' => '!',
            '․' => '.',
            '˸' => ':',
            '⁄' => '/',
            '‚' => ',',
            // Fullwidth ASCII punctuation; fullwidth letters/digits left alone.
            '\u{FF01}'..='\u{FF5E}' if !ch.is_alphanumeric() => {
                char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch)
            }
            _ => ch,
        })
        .take(MAX_WORLD_NAME)
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| sanitize_name(name))
}

/// Parse world name and software from a list of PNG text entries.
fn parse_png_entries(
    entries: &[PngTextEntry],
) -> (Option<String>, Option<String>, Vec<String>, Vec<String>) {
    let mut world_name: Option<String> = None;
    let mut xmp_world: Option<String> = None;
    let mut software: Option<String> = None;
    let mut participants = Vec::new();
    let mut tagged_participants: Vec<String> = Vec::new();

    for entry in entries {
        match entry.keyword.to_lowercase().as_str() {
            "xml:com.adobe.xmp" if xmp_world.is_none() => {
                xmp_world = parse_vrchat_xmp(&entry.value);
            }
            "description" | "comment" => {
                if world_name.is_some() {
                    continue;
                }
                if let Some((world, players)) = parse_vrchat_description(&entry.value) {
                    world_name = world.or(world_name);
                    participants = players.unwrap_or(participants);
                }
            }
            "vrchat organizer participants" => {
                if let Ok(names) = serde_json::from_str::<Vec<String>>(&entry.value) {
                    for name in names {
                        let name = name.trim();
                        if is_valid_person_name(name)
                            && tagged_participants.len() < MAX_PARTICIPANTS
                            && !tagged_participants
                                .iter()
                                .any(|existing| existing.eq_ignore_ascii_case(name))
                        {
                            tagged_participants.push(name.to_owned());
                        }
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

    // VRCX's JSON wins (it also lists players); XMP fills in when VRCX is
    // absent or wrote an empty world name.
    let world_name = match world_name {
        Some(name) if name == "Unnamed World" => xmp_world.or(Some(name)),
        None => xmp_world,
        name => name,
    };
    (world_name, software, participants, tagged_participants)
}

// ── JPEG EXIF extraction ──────────────────────────────────────────────────

/// Try to extract world_name and software from JPEG EXIF data.
/// Uses tag 270 (ImageDescription) for JSON with world info, and tag 305 for Software.
///
/// NOTE: `kamadak-exif`'s `display_value()` wraps string fields in quotes, which would
/// break JSON parsing. We access the raw ASCII bytes directly instead.
fn extract_exif_meta(path: &Path) -> (Option<String>, Option<String>, Vec<String>) {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None, Vec::new()),
    };
    let mut reader = std::io::BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut reader) {
        Ok(e) => e,
        Err(_) => return (None, None, Vec::new()),
    };

    let mut world_name: Option<String> = None;
    let mut software: Option<String> = None;
    let mut participants = Vec::new();

    // Tag 270 = ImageDescription — often contains JSON with world info
    if let Some(field) = exif.get_field(exif::Tag::ImageDescription, exif::In::PRIMARY) {
        // Use raw ASCII bytes instead of display_value() to avoid quoting issues
        if let exif::Value::Ascii(bytes) = &field.value {
            if let Some(first) = bytes.first() {
                if let Some((world, players)) = std::str::from_utf8(first)
                    .ok()
                    .and_then(parse_vrchat_description)
                {
                    world_name = world;
                    participants = players.unwrap_or_default();
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

    (world_name, software, participants)
}

pub fn extract_image_meta(path: &Path) -> Result<ImageMeta> {
    // Determine file extension
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase());
    let (width, height) = if ext.as_deref() == Some("png") {
        let mut reader = BufReader::new(fs::File::open(path)?);
        let mut signature = [0u8; 8];
        let mut ihdr = [0u8; 25];
        reader.read_exact(&mut signature)?;
        reader.read_exact(&mut ihdr)?;
        if &ihdr[4..8] != b"IHDR" {
            anyhow::bail!("PNG is missing IHDR");
        }
        (
            u32::from_be_bytes(ihdr[8..12].try_into().unwrap()),
            u32::from_be_bytes(ihdr[12..16].try_into().unwrap()),
        )
    } else {
        image::ImageReader::open(path)
            .with_context(|| format!("failed to open image {}", path.display()))?
            .into_dimensions()
            .with_context(|| format!("failed to read image dimensions for {}", path.display()))?
    };

    let (world_name, software, participants, tagged_participants) = match ext.as_deref() {
        Some("jpg") | Some("jpeg") => {
            // Try EXIF for JPEG files
            {
                let (world, software, participants) = extract_exif_meta(path);
                (world, software, participants, Vec::new())
            }
        }
        Some("png") => {
            // Try PNG text chunks
            let entries = extract_png_text_chunks(path).unwrap_or_default();
            parse_png_entries(&entries)
        }
        // For WebP and other formats, we can only get dimensions
        _ => (None, None, Vec::new(), Vec::new()),
    };

    Ok(ImageMeta {
        world_name,
        participants,
        tagged_participants,
        captured_at: path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(extract_capture_timestamp),
        width,
        height,
        software,
    })
}

fn extract_capture_timestamp(filename: &str) -> Option<u64> {
    static CAPTURE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = CAPTURE_RE.get_or_init(|| {
        Regex::new(r"VRChat_(\d{4}-\d{2}-\d{2})_(\d{2})-(\d{2})-(\d{2})(?:\.(\d{1,3}))?").unwrap()
    });
    let captures = re.captures(filename)?;
    let date = captures.get(1)?.as_str();
    let hour = captures.get(2)?.as_str().parse::<u32>().ok()?;
    let minute = captures.get(3)?.as_str().parse::<u32>().ok()?;
    let second = captures.get(4)?.as_str().parse::<u32>().ok()?;
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let datetime = date.and_hms_opt(hour, minute, second)?;
    chrono::Local
        .from_local_datetime(&datetime)
        .single()
        .map(|value| value.timestamp())
        .and_then(|value| value.try_into().ok())
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
    let organization_lock = ORGANIZATION_LOCK.get_or_init(|| Mutex::new(()));
    let _organization_guard = loop {
        match organization_lock.try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::WouldBlock) => {
                if cancel_requested() {
                    anyhow::bail!("scan cancelled");
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(TryLockError::Poisoned(_)) => {
                anyhow::bail!("organization lock is poisoned");
            }
        }
    };
    if cancel_requested() {
        anyhow::bail!("scan cancelled");
    }
    let started = std::time::Instant::now();
    validate_template(&config.template)?;
    if !config.base_path.exists() {
        anyhow::bail!("path does not exist: {}", config.base_path.display());
    }
    // The previous run's undo history is replaced at this run's first real move.
    let mut fresh_undo = !config.dry_run;

    let mut candidates: Vec<PathBuf> = Vec::new();
    if config.single_folder || config.scan_all_months {
        // A full scan starts at the selected root so captures sitting beside
        // month folders are included as well.
        candidates.push(config.base_path.clone());
    } else {
        for entry in fs::read_dir(&config.base_path)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if is_month_folder(name) {
                    candidates.push(path);
                }
            }
        }
        candidates.sort();
        if let Some(last) = candidates.last() {
            candidates = vec![last.clone()];
        }
        if candidates.is_empty() {
            // A picked folder may contain screenshots directly (including a
            // month folder selected on its own), so scan it when no month
            // hierarchy exists below the selected path.
            candidates.push(config.base_path.clone());
        }
    }

    for folder in candidates {
        if cancel_requested() {
            anyhow::bail!("scan cancelled");
        }
        process_folder(&folder, config, stats, &mut fresh_undo)?;
    }

    if !config.single_folder && !config.scan_all_months {
        // Process root-level captures after the selected month traversal so a
        // moved root file cannot be discovered a second time in the same run.
        let mut root_files = Vec::new();
        for entry in fs::read_dir(&config.base_path)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && is_image_path(&entry.path())
                && !is_ignored_path(&entry.path(), &config.base_path)
            {
                root_files.push(entry.path());
            }
        }
        root_files.sort();
        for image_file in root_files {
            if cancel_requested() {
                anyhow::bail!("scan cancelled");
            }
            if let Err(error) =
                organize_single_file_unlocked(&image_file, config, stats, &mut fresh_undo)
            {
                eprintln!("error processing {}: {error:#}", image_file.display());
            }
        }
    }

    let status = if stats.errors == 0 {
        "success"
    } else {
        "warning"
    };
    let logged = log_activity(
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
    );
    // The moves already happened and are undoable; a log failure is not fatal.
    if let Err(error) = logged {
        eprintln!("warning: could not write activity log: {error:#}");
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
    let organization_lock = ORGANIZATION_LOCK.get_or_init(|| Mutex::new(()));
    let _organization_guard = loop {
        match organization_lock.try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::WouldBlock) => {
                if cancel_requested() {
                    anyhow::bail!("scan cancelled");
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(TryLockError::Poisoned(_)) => {
                anyhow::bail!("organization lock is poisoned");
            }
        }
    };
    organize_single_file_unlocked(file_path, config, stats, &mut false)
}

/// `fresh_undo` is true until the first move of a new `organize_path` run,
/// which replaces the previous run's undo log.
fn organize_single_file_unlocked(
    file_path: &Path,
    config: &OrganizerConfig,
    stats: &mut OrganizerStats,
    fresh_undo: &mut bool,
) -> Result<()> {
    if cancel_requested() {
        anyhow::bail!("scan cancelled");
    }
    validate_template(&config.template)?;
    stats.processed += 1;
    stats.total += 1;

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

    // A capture without a date in its filename is placed directly under
    // base/world rather than base/YYYY-MM/world. Recognize that layout too,
    // otherwise a recursive watcher would see its own move as new work.
    if let Some(target_sub) = &target_sub {
        let expected_parent = month_folder.join(target_sub);
        if file_path.parent() == Some(expected_parent.as_path()) {
            stats.already_organized += 1;
            return Ok(());
        }
    }

    if let Some(target_sub) = target_sub {
        println!("detected world: {} ({})", target_sub, file_path.display());
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
            let parent = destination.parent().unwrap_or(&month_folder);
            let new_dirs: Vec<PathBuf> = [parent, month_folder.as_path()]
                .into_iter()
                .filter(|dir| !dir.exists())
                .map(Path::to_path_buf)
                .collect();
            let moved = (|| -> Result<()> {
                fs::create_dir_all(parent)?;
                let original_full = file_path
                    .canonicalize()
                    .unwrap_or_else(|_| file_path.to_path_buf());
                // Canonicalize the parent folder (the destination file doesn't
                // exist yet) so the undo log is stable even if the base path
                // contains symlinks or is later resolved differently.
                let dest_full = parent
                    .canonicalize()
                    .unwrap_or_else(|_| parent.to_path_buf())
                    .join(destination.file_name().unwrap_or_default());
                move_file(file_path, &destination)?;
                let logged = (|| -> Result<()> {
                    if *fresh_undo {
                        match fs::remove_file(config.base_path.join(UNDO_FILE)) {
                            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                                return Err(error.into())
                            }
                            _ => *fresh_undo = false,
                        }
                    }
                    log_undo_entry(
                        &config.base_path,
                        &UndoEntry {
                            original_path: original_full.to_string_lossy().to_string(),
                            destination_path: dest_full.to_string_lossy().to_string(),
                        },
                    )
                })();
                // Never leave a move that cannot be undone: put the file back.
                if let Err(error) = logged {
                    return match move_file(&destination, file_path) {
                        Ok(()) => Err(error.context("could not record undo entry; move rolled back")),
                        Err(rollback) => Err(error.context(format!(
                            "could not record undo entry and could not move the file back: {rollback:#}"
                        ))),
                    };
                }
                Ok(())
            })();
            if let Err(error) = moved {
                stats.errors += 1;
                for dir in &new_dirs {
                    let _ = fs::remove_dir(dir);
                }
                return Err(error.context(format!(
                    "failed to move {} -> {}",
                    file_path.display(),
                    destination.display()
                )));
            }
            println!("moved {} -> {}", file_path.display(), destination.display());
        }
        stats.organized += 1;
        stats.organized_photos.push(OrganizedPhoto {
            photo_name: filename.to_string(),
            folder_name: target_sub.clone(),
        });
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
    fresh_undo: &mut bool,
) -> Result<()> {
    let mut image_files = Vec::new();
    for entry in WalkDir::new(folder).follow_root_links(folder == config.base_path) {
        if cancel_requested() {
            anyhow::bail!("scan cancelled");
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.file_type().is_file() {
            let path = entry.into_path();
            if is_image_path(&path) && !is_ignored_path(&path, &config.base_path) {
                image_files.push(path);
            } else if !config.dry_run
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_copy_temp_name)
            {
                // Leftover from an interrupted cross-device copy; its source is intact.
                let _ = fs::remove_file(&path);
            }
        }
    }
    image_files.sort();

    for image_file in image_files {
        if cancel_requested() {
            anyhow::bail!("scan cancelled");
        }
        // Delegate to organize_single_file which handles stats, metadata, template resolution, undo
        if let Err(e) = organize_single_file_unlocked(&image_file, config, stats, fresh_undo) {
            eprintln!("error processing {}: {e:#}", image_file.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(data);
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        chunk.extend_from_slice(&crc.sum().to_be_bytes());
        chunk
    }

    /// A real PNG with an optional VRChat-style Description tEXt chunk after IHDR.
    fn vrchat_png(world: Option<&str>, width: u32, height: u32) -> Vec<u8> {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            width,
            height,
            image::Rgb([90, 120, 160]),
        ))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
        if let Some(world) = world {
            let text = format!(
                "Description\0{}",
                serde_json::json!({"world": {"name": world}, "players": [{"displayName": "Alice"}]})
            );
            png.splice(33..33, png_chunk(b"tEXt", text.as_bytes()));
        }
        png
    }

    /// Handcrafted PNG byte layouts covering every branch of the fingerprint.
    fn fingerprint_fixtures() -> Vec<Vec<u8>> {
        let mut base = PNG_SIGNATURE.to_vec();
        base.extend(png_chunk(b"IHDR", &[0, 0, 0, 4, 0, 0, 0, 4, 8, 2, 0, 0, 0]));
        base.extend(png_chunk(b"IDAT", b"pixel data"));
        base.extend(png_chunk(b"IEND", b""));
        let mut tagged = base.clone();
        let mut tag = ORGANIZER_TAG_PREFIX.to_vec();
        tag.extend_from_slice(br#"["Alice"]"#);
        tagged.splice(33..33, png_chunk(b"tEXt", &tag));
        let mut short_text = base.clone();
        short_text.splice(33..33, png_chunk(b"tEXt", b"VRChat\0x"));
        let mut trailing = base.clone();
        trailing.extend_from_slice(&[1; 40]);
        let mut no_iend = base[..base.len() - 12].to_vec();
        no_iend.extend_from_slice(&[7; 5]);
        let mut truncated = base.clone();
        truncated.truncate(base.len() - 20);
        let mut huge = PNG_SIGNATURE.to_vec();
        huge.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        huge.extend_from_slice(b"tEXtabcdefgh");
        vec![
            base,
            tagged,
            short_text,
            trailing,
            no_iend,
            truncated,
            huge,
            PNG_SIGNATURE.to_vec(),
            b"\xFF\xD8\xFF\xE0 jpeg".to_vec(),
            Vec::new(),
        ]
    }

    /// Expected values were produced by the original whole-file
    /// implementation; any change here wipes every user's stored tags.
    #[test]
    fn png_fingerprint_is_byte_identical_to_original() {
        let dir = std::env::temp_dir().join(format!("vrchat-organizer-fp-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let plain = Some("8e841a212b1afc6790f531cce55baa9f57e8187dd2961ed1e3e0c8a983b93972");
        let expected = [
            plain,
            plain,
            Some("612d0bc6f7337281a89881a7dda41aec347d746649c8b2991ea4798c65d8ee42"),
            plain,
            Some("561cd354362372520a7e78183dfb9bc84869d64f0f058fbb541634897fef3e58"),
            None,
            None,
            Some("4c4b6a3be1314ab86138bef4314dde022e600960d8689a2c8f8631802d20dab6"),
            None,
            None,
        ];
        for (index, (bytes, expected)) in
            fingerprint_fixtures().into_iter().zip(expected).enumerate()
        {
            let path = dir.join(format!("{index}.png"));
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                normalized_png_fingerprint(&path).ok().as_deref(),
                expected,
                "fixture {index}"
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    fn fresh_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vrchat-organizer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn real_run(base_path: &Path) -> OrganizerConfig {
        OrganizerConfig {
            base_path: base_path.to_path_buf(),
            dry_run: false,
            scan_all_months: false,
            single_folder: false,
            template: "{world}".to_string(),
        }
    }

    /// Relative paths of every non-hidden file below `dir`.
    fn tree(dir: &Path) -> Vec<String> {
        let mut files: Vec<String> = WalkDir::new(dir)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| {
                entry
                    .path()
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .filter(|path| !path.starts_with('.'))
            .collect();
        files.sort();
        files
    }

    #[test]
    fn organizes_into_month_world_layout_and_undo_restores_it() {
        let dir = fresh_dir("e2e");
        let month = dir.join("2026-10");
        fs::create_dir_all(&month).unwrap();
        let root_photo = "VRChat_2026-09-20_21-00-00.000_8x8.png";
        let month_photo = "VRChat_2026-10-07_12-00-00.000_8x8.png";
        let print = "VRChat_2026-10-07_12-30-00.000_2048x1440.png";
        let plain = "VRChat_2026-10-07_13-00-00.000_8x8.png";
        fs::write(dir.join(root_photo), vrchat_png(Some("Root World"), 8, 8)).unwrap();
        fs::write(
            month.join(month_photo),
            vrchat_png(Some("Month: World"), 8, 8),
        )
        .unwrap();
        fs::write(month.join(print), vrchat_png(None, 2048, 1440)).unwrap();
        fs::write(month.join(plain), vrchat_png(None, 8, 8)).unwrap();
        let before = tree(&dir);

        let mut stats = OrganizerStats::default();
        organize_path(&real_run(&dir), &mut stats).unwrap();
        assert_eq!(
            (stats.organized, stats.no_metadata, stats.errors),
            (3, 1, 0)
        );
        assert_eq!(
            tree(&dir),
            vec![
                format!("2026-09/Root World/{root_photo}"),
                format!("2026-10/Month_ World/{month_photo}"),
                format!("2026-10/Prints/{print}"),
                format!("2026-10/{plain}"),
            ]
        );
        assert_eq!(read_undo_log(&dir).unwrap().len(), 3);

        let mut undo = OrganizerStats::default();
        undo_organization(&dir, &mut undo).unwrap();
        assert_eq!((undo.undone, undo.errors), (3, 0));
        assert_eq!(tree(&dir), before);
        assert!(read_undo_log(&dir).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_file_never_replaces_an_existing_destination() {
        let dir = fresh_dir("no-clobber");
        let (src, dst) = (dir.join("a.png"), dir.join("b.png"));
        fs::write(&src, "source").unwrap();
        fs::write(&dst, "someone else").unwrap();
        assert!(move_file(&src, &dst).is_err());
        assert!(copy_file_no_clobber(&src, &dst).is_err());
        assert_eq!(fs::read_to_string(&src).unwrap(), "source");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "someone else");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            2,
            "no temp file left behind"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_fallback_moves_or_leaves_source_untouched() {
        let dir = fresh_dir("copy-move");
        let src = dir.join("a.png");
        fs::write(&src, "pixels").unwrap();
        // Simulated copy failure: the destination folder does not exist.
        assert!(copy_file_no_clobber(&src, &dir.join("missing").join("a.png")).is_err());
        assert_eq!(fs::read_to_string(&src).unwrap(), "pixels");
        copy_file_no_clobber(&src, &dir.join("b.png")).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(dir.join("b.png")).unwrap(), "pixels");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no temp file left behind"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Organizes `name` (written into `<dir>/2026-10/`) and asserts it was left
    /// exactly where and as it was, with nothing created for it.
    fn assert_left_alone(dir: &Path, name: &str, bytes: &[u8]) {
        let photo = dir.join("2026-10").join(name);
        let mut stats = OrganizerStats::default();
        organize_path(&real_run(dir), &mut stats).unwrap();
        assert_eq!(stats.organized, 0, "{name}");
        assert_eq!(fs::read(&photo).unwrap(), bytes, "{name}");
        assert!(read_undo_log(dir).unwrap().is_empty(), "{name}");
    }

    #[test]
    fn unreadable_images_are_left_alone() {
        let dir = fresh_dir("corrupt");
        fs::create_dir_all(dir.join("2026-10")).unwrap();
        let mut corrupt = PNG_SIGNATURE.to_vec();
        corrupt.extend_from_slice(b"\0\0\0\x0dIHDRgarbage");
        for (name, bytes) in [("corrupt.png", corrupt), ("empty.png", Vec::new())] {
            fs::write(dir.join("2026-10").join(name), &bytes).unwrap();
            assert_left_alone(&dir, name, &bytes);
            assert_eq!(tree(&dir), vec![format!("2026-10/{name}")]);
            fs::remove_file(dir.join("2026-10").join(name)).unwrap();
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn read_only_destination_leaves_photo_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = fresh_dir("read-only");
        let world = dir.join("2026-10").join("World");
        fs::create_dir_all(&world).unwrap();
        let name = "VRChat_2026-10-07_12-00-00.000_8x8.png";
        let bytes = vrchat_png(Some("World"), 8, 8);
        fs::write(dir.join("2026-10").join(name), &bytes).unwrap();
        fs::set_permissions(&world, fs::Permissions::from_mode(0o555)).unwrap();
        assert_left_alone(&dir, name, &bytes);
        fs::set_permissions(&world, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(fs::read_dir(&world).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_log_failure_rolls_the_move_back() {
        let dir = fresh_dir("undo-fail");
        fs::create_dir_all(dir.join("2026-10")).unwrap();
        // A directory where the undo log should be makes every append fail.
        fs::create_dir_all(dir.join(UNDO_FILE)).unwrap();
        let name = "VRChat_2026-10-07_12-00-00.000_8x8.png";
        let bytes = vrchat_png(Some("World"), 8, 8);
        fs::write(dir.join("2026-10").join(name), &bytes).unwrap();
        let mut stats = OrganizerStats::default();
        organize_path(&real_run(&dir), &mut stats).unwrap();
        assert_eq!((stats.organized, stats.errors), (0, 1));
        assert_eq!(fs::read(dir.join("2026-10").join(name)).unwrap(), bytes);
        assert!(
            !dir.join("2026-10").join("World").exists(),
            "new world folder removed"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_log_appends_lines_converts_legacy_and_skips_foreign_entries() {
        let dir = fresh_dir("undo-jsonl");
        let base = dir.canonicalize().unwrap();
        let moved = base.join("moved.png");
        fs::write(&moved, "pixels").unwrap();
        let inside = UndoEntry {
            original_path: base.join("original.png").display().to_string(),
            destination_path: moved.display().to_string(),
        };
        let outside = UndoEntry {
            original_path: std::env::temp_dir()
                .join("elsewhere.png")
                .display()
                .to_string(),
            destination_path: moved.display().to_string(),
        };
        fs::write(
            base.join(UNDO_FILE),
            serde_json::to_string_pretty(&[&outside]).unwrap(),
        )
        .unwrap();
        log_undo_entry(&base, &inside).unwrap();
        let data = fs::read_to_string(base.join(UNDO_FILE)).unwrap();
        assert_eq!(
            data.lines().count(),
            2,
            "legacy array converted to JSON Lines"
        );
        assert_eq!(read_undo_log(&base).unwrap().len(), 2);

        let mut stats = OrganizerStats::default();
        undo_organization(&base, &mut stats).unwrap();
        assert_eq!((stats.undone, stats.errors), (1, 1));
        assert_eq!(
            fs::read_to_string(base.join("original.png")).unwrap(),
            "pixels"
        );
        let remaining = read_undo_log(&base).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].original_path, outside.original_path);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compressed_text_is_capped() {
        use flate2::{write::ZlibEncoder, Compression};
        let dir = fresh_dir("zlib-cap");
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&vec![b'a'; 4 << 20]).unwrap();
        let mut ztxt = b"Description\0\0".to_vec();
        ztxt.extend(encoder.finish().unwrap());
        let mut png = vrchat_png(None, 2, 2);
        png.splice(33..33, png_chunk(b"zTXt", &ztxt));
        let path = dir.join("bomb.png");
        fs::write(&path, png).unwrap();
        let entries = extract_png_text_chunks(&path).unwrap();
        assert_eq!(entries[0].value.len() as u64, MAX_INFLATED_TEXT);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_after_idat_is_found() {
        let dir = fresh_dir("text-after-idat");
        let mut png = vrchat_png(None, 2, 2);
        let text = br#"Description{"world":{"name":"Late World"}}"#.to_vec();
        let mut text = text;
        text.insert(11, 0);
        let iend = png.len() - 12;
        png.splice(iend..iend, png_chunk(b"tEXt", &text));
        let path = dir.join("late.png");
        fs::write(&path, png).unwrap();
        assert_eq!(
            extract_image_meta(&path).unwrap().world_name.as_deref(),
            Some("Late World")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_writes_only_changed_rows_and_keeps_other_months() {
        let dir = fresh_dir("cache-rows");
        for month in ["2026-09", "2026-10"] {
            let world = dir.join(month).join("World");
            fs::create_dir_all(&world).unwrap();
            for index in 0..2 {
                fs::write(
                    world.join(format!("{index}.png")),
                    vrchat_png(Some("World"), 4, 4),
                )
                .unwrap();
            }
        }
        let cache = dir.join("library.sqlite");
        scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        let connection = Connection::open(&cache).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE writes(n INTEGER);
                 CREATE TRIGGER count_writes AFTER INSERT ON photos BEGIN INSERT INTO writes VALUES(1); END;",
            )
            .unwrap();
        let count = |sql: &str| {
            connection
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .unwrap()
        };
        scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM writes"), 0);
        // A latest-month scan must not drop the other month's rows.
        scan_library_with_cache(&dir, false, Some(&cache)).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM photos"), 4);
        fs::remove_file(dir.join("2026-10/World/0.png")).unwrap();
        scan_library_with_cache(&dir, false, Some(&cache)).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM photos"), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_tag_writes_keep_every_tag() {
        let dir = fresh_dir("tag-race");
        let photos: Vec<PathBuf> = (0..2)
            .map(|index| {
                let path = dir.join(format!("{index}.png"));
                fs::write(&path, vrchat_png(Some(&format!("W{index}")), 2, 2)).unwrap();
                path
            })
            .collect();
        std::thread::scope(|scope| {
            for photo in &photos {
                let dir = &dir;
                scope.spawn(move || {
                    for index in 0..15 {
                        tag_photo_participant_in_store(dir, photo, &format!("P{index}")).unwrap();
                    }
                });
            }
        });
        let store = load_tag_store(&dir).unwrap();
        assert_eq!(store.len(), 2);
        assert!(store.values().all(|names| names.len() == 15));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn people_come_from_drawn_boxes() {
        let dir = fresh_dir("people-counts");
        let db = dir.join("avatar.sqlite");
        let photos: Vec<PathBuf> = ["2026-10-07_12-00-00", "2026-10-08_12-00-00"]
            .iter()
            .map(|stamp| {
                let path = dir.join(format!("VRChat_{stamp}.000_64x64.png"));
                fs::write(&path, vrchat_png(Some("Pool"), 64, 64)).unwrap();
                path
            })
            .collect();
        save_positional_person_tag(&db, &photos[0], "Alice", 0, 0, 10, 10).unwrap();
        save_positional_person_tag(&db, &photos[0], "Alice", 20, 20, 10, 10).unwrap();
        save_positional_person_tag(&db, &photos[1], "Alice", 0, 0, 10, 10).unwrap();
        let bob = save_positional_person_tag(&db, &photos[1], "Bob", 5, 5, 10, 10).unwrap();
        delete_positional_person_tag(&db, bob.id).unwrap();
        let people = avatar_people(&db).unwrap();
        assert_eq!(people.len(), 1, "people without boxes are hidden");
        assert_eq!(people[0].display_name, "Alice");
        assert_eq!(people[0].screenshot_count, 2);
        assert_eq!(people[0].favourite_world.as_deref(), Some("Pool"));
        assert!(people[0].last_seen.is_some());
        assert!(Path::new(people[0].thumbnail_path.as_ref().unwrap()).is_file());
        assert_eq!(avatar_person_photos(&db, people[0].id).unwrap().len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_drops_ai_data_but_keeps_drawn_boxes() {
        let dir = fresh_dir("people-migration");
        let db = dir.join("avatar-recognition.sqlite");
        let crops = dir.join("avatar-crops");
        fs::create_dir_all(&crops).unwrap();
        fs::write(crops.join("manual-1.jpg"), "drawn").unwrap();
        fs::write(crops.join("0123abcd.jpg"), "ai").unwrap();
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE avatar_people(id INTEGER PRIMARY KEY, display_name TEXT NOT NULL UNIQUE,
                     thumbnail_path TEXT, aliases TEXT NOT NULL DEFAULT '[]', created_at INTEGER NOT NULL,
                     last_seen INTEGER, confidence REAL NOT NULL DEFAULT 0);
                 CREATE TABLE avatar_samples(id INTEGER PRIMARY KEY, person_id INTEGER REFERENCES avatar_people(id));
                 CREATE TABLE avatar_detections(id INTEGER PRIMARY KEY, person_id INTEGER REFERENCES avatar_people(id));
                 CREATE TABLE avatar_scan_cache(screenshot_path TEXT PRIMARY KEY);
                 CREATE TABLE avatar_profiles(id INTEGER PRIMARY KEY, person_id INTEGER REFERENCES avatar_people(id));
                 CREATE TABLE person_avatar_links(person_id INTEGER, avatar_id INTEGER REFERENCES avatar_profiles(id));
                 CREATE TABLE recognition_settings(key TEXT PRIMARY KEY, value TEXT);
                 CREATE TABLE positional_person_tags(id INTEGER PRIMARY KEY, screenshot_path TEXT NOT NULL,
                     person_name TEXT NOT NULL, crop_path TEXT NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL,
                     width INTEGER NOT NULL, height INTEGER NOT NULL, image_width INTEGER NOT NULL,
                     image_height INTEGER NOT NULL, world_name TEXT, captured_at INTEGER,
                     confidence REAL NOT NULL DEFAULT 1, created_at INTEGER NOT NULL,
                     UNIQUE(screenshot_path, person_name, x, y, width, height));
                 INSERT INTO avatar_people(id,display_name,created_at) VALUES(1,'Alice',0),(2,'Learned Only',0);
                 INSERT INTO avatar_samples VALUES(1,2);
                 INSERT INTO avatar_detections VALUES(1,2);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO positional_person_tags(screenshot_path,person_name,crop_path,x,y,width,height,
                     image_width,image_height,created_at) VALUES('a.png','Alice',?,0,0,1,1,1,1,0)",
                [crops.join("manual-1.jpg").to_string_lossy()],
            )
            .unwrap();
        drop(connection);
        for _ in 0..2 {
            let people = avatar_people(&db).unwrap();
            assert_eq!(people.len(), 1);
            assert_eq!(people[0].display_name, "Alice");
            assert_eq!(
                positional_person_tags(&db, Path::new("a.png"))
                    .unwrap()
                    .len(),
                1
            );
        }
        let connection = Connection::open(&db).unwrap();
        let tables: Vec<String> = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(tables, vec!["avatar_people", "positional_person_tags"]);
        let names = fs::read_dir(&crops).unwrap().count();
        assert_eq!(names, 1);
        assert!(crops.join("manual-1.jpg").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn positional_tag_survives_a_failed_update() {
        let dir = fresh_dir("positional");
        let db = dir.join("avatar.sqlite");
        let photo = dir.join("VRChat_2026-10-07_12-00-00.000_64x64.png");
        fs::write(&photo, vrchat_png(Some("World"), 64, 64)).unwrap();
        let tag = save_positional_person_tag(&db, &photo, "Alice", 4, 4, 20, 20).unwrap();
        assert!(update_positional_person_tag(&db, tag.id, 60, 60, 20, 20).is_err());
        assert_eq!(positional_person_tags(&db, &photo).unwrap().len(), 1);
        let moved = update_positional_person_tag(&db, tag.id, 8, 8, 20, 20).unwrap();
        let tags = positional_person_tags(&db, &photo).unwrap();
        assert_eq!((tags.len(), tags[0].id, tags[0].x), (1, moved.id, 8));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recognizes_only_our_copy_temp_names() {
        assert!(is_copy_temp_name(".VRChat_a.png.copying.123.456"));
        assert!(is_copy_temp_name(&format!(
            ".a.jpg.copying.{}",
            temp_suffix()
        )));
        assert!(!is_copy_temp_name(".a.png.copying.123"));
        assert!(!is_copy_temp_name(".notes.txt.copying.1.2"));
        assert!(!is_copy_temp_name("a.png.copying.1.2"));
        assert!(!is_copy_temp_name(".a.png.copying.1.x"));
    }

    #[test]
    fn undo_history_is_only_replaced_once_the_old_log_is_gone() {
        let dir = fresh_dir("fresh-undo");
        let month = dir.join("2026-10");
        fs::create_dir_all(&month).unwrap();
        let photos: Vec<PathBuf> = (0..2)
            .map(|index| {
                let path = month.join(format!("VRChat_2026-10-07_12-00-0{index}.000_8x8.png"));
                fs::write(&path, vrchat_png(Some("World"), 8, 8)).unwrap();
                path
            })
            .collect();
        // The old log cannot be removed: the move is rolled back and the
        // next move must still try to replace the history.
        fs::create_dir_all(dir.join(UNDO_FILE).join("blocker")).unwrap();
        let (config, mut stats, mut fresh) = (real_run(&dir), OrganizerStats::default(), true);
        assert!(
            organize_single_file_unlocked(&photos[0], &config, &mut stats, &mut fresh).is_err()
        );
        assert!(fresh && photos[0].is_file());
        fs::remove_dir_all(dir.join(UNDO_FILE)).unwrap();
        log_undo_entry(
            &dir,
            &UndoEntry {
                original_path: "old".to_string(),
                destination_path: "old".to_string(),
            },
        )
        .unwrap();
        organize_single_file_unlocked(&photos[1], &config, &mut stats, &mut fresh).unwrap();
        assert!(!fresh);
        let log = read_undo_log(&dir).unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].original_path.ends_with("12-00-01.000_8x8.png"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_chunk_flood_is_bounded() {
        use flate2::{write::ZlibEncoder, Compression};
        let dir = fresh_dir("text-flood");
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&vec![b'a'; 1 << 20]).unwrap();
        let mut ztxt = b"Description\0\0".to_vec();
        ztxt.extend(encoder.finish().unwrap());
        let bomb = png_chunk(b"zTXt", &ztxt);
        let mut png = vrchat_png(None, 2, 2);
        let flood: Vec<u8> = (0..5000).flat_map(|_| bomb.iter().copied()).collect();
        png.splice(33..33, flood);
        let path = dir.join("flood.png");
        fs::write(&path, png).unwrap();
        let entries = extract_png_text_chunks(&path).unwrap();
        let total: usize = entries.iter().map(|entry| entry.value.len()).sum();
        assert!(entries.len() <= MAX_TEXT_CHUNKS);
        assert!(total as u64 <= MAX_TEXT_TOTAL, "{total}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn metadata_and_tag_store_are_capped() {
        let players: Vec<_> = (0..300)
            .map(|index| serde_json::json!({ "displayName": format!("P{index}") }))
            .chain([serde_json::json!({ "displayName": "x".repeat(81) })])
            .collect();
        let json = serde_json::json!({ "world": { "name": "w".repeat(150) }, "players": players });
        let (world, players) = parse_vrchat_description(&json.to_string()).unwrap();
        assert_eq!(world.unwrap().chars().count(), MAX_WORLD_NAME);
        let players = players.unwrap();
        assert_eq!(players.len(), MAX_PARTICIPANTS);
        assert!(players.iter().all(|name| name.len() <= 80));

        let dir = fresh_dir("tag-store-validate");
        let good = "a".repeat(64);
        fs::write(
            dir.join(TAG_STORE_FILE),
            serde_json::json!({ good.clone(): ["Alice", "", "y".repeat(81)], "not-a-hash": ["Bob"] })
                .to_string(),
        )
        .unwrap();
        let store = load_tag_store(&dir).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store[&good], vec!["Alice"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn world_named_like_a_month_organizes_once() {
        let dir = fresh_dir("month-world");
        let month = dir.join("2026-10");
        fs::create_dir_all(&month).unwrap();
        let name = "VRChat_2026-10-07_12-00-00.000_8x8.png";
        fs::write(month.join(name), vrchat_png(Some("2025-01"), 8, 8)).unwrap();
        let mut first = OrganizerStats::default();
        organize_path(&real_run(&dir), &mut first).unwrap();
        assert_eq!(tree(&dir), vec![format!("2026-10/2025-01_/{name}")]);
        let mut second = OrganizerStats::default();
        organize_path(&real_run(&dir), &mut second).unwrap();
        assert_eq!((second.organized, second.already_organized), (0, 1));
        assert_eq!(sanitize_name("prints"), "prints_");
        assert_eq!(sanitize_name(THUMBNAIL_DIR), format!("{THUMBNAIL_DIR}_"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_keeps_torn_lines() {
        let dir = fresh_dir("undo-torn");
        let base = dir.canonicalize().unwrap();
        fs::write(base.join("moved.png"), "pixels").unwrap();
        let entry = UndoEntry {
            original_path: base.join("original.png").display().to_string(),
            destination_path: base.join("moved.png").display().to_string(),
        };
        fs::write(
            base.join(UNDO_FILE),
            format!(
                "{{\"original_pa\n{}\n",
                serde_json::to_string(&entry).unwrap()
            ),
        )
        .unwrap();
        let mut stats = OrganizerStats::default();
        undo_organization(&base, &mut stats).unwrap();
        assert_eq!((stats.undone, stats.errors), (1, 1));
        assert_eq!(
            fs::read_to_string(base.join(UNDO_FILE)).unwrap(),
            "{\"original_pa\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn activity_log_failure_does_not_fail_a_run() {
        let dir = fresh_dir("activity-fail");
        fs::create_dir_all(dir.join(".vrchat-organizer-activity.json")).unwrap();
        let mut stats = OrganizerStats::default();
        organize_path(&real_run(&dir), &mut stats).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decode_refuses_oversized_images() {
        let dir = fresh_dir("decode-limits");
        let mut png = PNG_SIGNATURE.to_vec();
        let mut ihdr = 20_000_u32.to_be_bytes().repeat(2);
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        png.extend(png_chunk(b"IHDR", &ihdr));
        png.extend(png_chunk(b"IDAT", b"x"));
        png.extend(png_chunk(b"IEND", b""));
        let path = dir.join("huge.png");
        fs::write(&path, png).unwrap();
        assert!(decode_image(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphan_thumbnails_are_removed_only_when_safe() {
        let dir = fresh_dir("orphans");
        let thumbs = dir.join(THUMBNAIL_DIR);
        fs::create_dir_all(&thumbs).unwrap();
        let stale = thumbs.join("stale.jpg");
        fs::write(&stale, "old").unwrap();
        fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        // No photos found: nothing is deleted.
        scan_library_with_options(&dir, true).unwrap();
        assert!(stale.exists());
        fs::write(dir.join("photo.png"), vrchat_png(None, 2, 2)).unwrap();
        let fresh = thumbs.join("fresh.jpg");
        fs::write(&fresh, "new").unwrap();
        // `fresh` is dated after this scan starts only if written during it;
        // give it a future mtime to stand in for that.
        fs::File::options()
            .write(true)
            .open(&fresh)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        scan_library_with_options(&dir, true).unwrap();
        assert!(!stale.exists());
        assert!(fresh.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_never_redirect_writes_or_moves() {
        use std::os::unix::fs::symlink;
        let dir = fresh_dir("symlinks");
        let outside = fresh_dir("symlinks-outside");
        let base = dir.canonicalize().unwrap();

        // Undo refuses an original path through a symlinked folder.
        symlink(&outside, base.join("link")).unwrap();
        fs::write(base.join("moved.png"), "pixels").unwrap();
        log_undo_entry(
            &base,
            &UndoEntry {
                original_path: base.join("link/escaped.png").display().to_string(),
                destination_path: base.join("moved.png").display().to_string(),
            },
        )
        .unwrap();
        let mut stats = OrganizerStats::default();
        undo_organization(&base, &mut stats).unwrap();
        assert_eq!((stats.undone, stats.errors), (0, 1));
        assert!(base.join("moved.png").is_file());
        assert!(!outside.join("escaped.png").exists());
        assert_eq!(read_undo_log(&base).unwrap().len(), 1);

        // App files and the thumbnail folder are never written through symlinks.
        let target = outside.join("target.json");
        fs::write(&target, "untouched").unwrap();
        fs::remove_file(base.join(UNDO_FILE)).unwrap();
        symlink(&target, base.join(UNDO_FILE)).unwrap();
        symlink(&target, base.join(TAG_STORE_FILE)).unwrap();
        let entry = UndoEntry {
            original_path: "a".to_string(),
            destination_path: "b".to_string(),
        };
        assert!(log_undo_entry(&base, &entry).is_err());
        assert!(save_tag_store(&base, &HashMap::new()).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "untouched");
        symlink(&outside, base.join(THUMBNAIL_DIR)).unwrap();
        fs::write(base.join("photo.png"), vrchat_png(None, 2, 2)).unwrap();
        assert!(generate_thumbnail(&base.join("photo.png"), &base).is_err());

        // A symlinked month folder is not organized.
        let month = outside.join("month");
        fs::create_dir_all(&month).unwrap();
        let photo = month.join("VRChat_2026-10-07_12-00-00.000_8x8.png");
        fs::write(&photo, vrchat_png(Some("World"), 8, 8)).unwrap();
        symlink(&month, base.join("2026-10")).unwrap();
        fs::remove_file(base.join(UNDO_FILE)).unwrap();
        let mut stats = OrganizerStats::default();
        organize_path(&real_run(&base), &mut stats).unwrap();
        assert!(photo.is_file());
        assert_eq!(stats.organized, 0);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }

    fn proc_io_wchar() -> u64 {
        fs::read_to_string("/proc/self/io")
            .ok()
            .and_then(|io| {
                io.lines()
                    .find_map(|line| line.strip_prefix("wchar: ")?.trim().parse().ok())
            })
            .unwrap_or_default()
    }

    /// Synthetic 5,000-photo library benchmark. Run with
    /// `cargo test --release -p organizer-core perf_harness -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn perf_harness_5000_photos() {
        let dir =
            std::env::temp_dir().join(format!("vrchat-organizer-perf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for index in 0..5000 {
            let month = 1 + index % 5;
            let folder = if index % 10 == 0 {
                dir.clone()
            } else {
                dir.join(format!("2026-{month:02}"))
            };
            fs::create_dir_all(&folder).unwrap();
            let png = vrchat_png(Some(&format!("World {}", index % 50)), 64, 36);
            fs::write(
                folder.join(format!(
                    "VRChat_2026-{month:02}-{:02}_10-{:02}-{:02}.{:03}_64x36.png",
                    1 + index % 28,
                    index / 60 % 60,
                    index % 60,
                    index % 1000
                )),
                png,
            )
            .unwrap();
        }
        let cache = dir.join("library.sqlite");
        let started = Instant::now();
        assert_eq!(
            scan_library_with_cache(&dir, true, Some(&cache))
                .unwrap()
                .total,
            5000
        );
        let cold = started.elapsed();
        let started = Instant::now();
        assert_eq!(
            scan_library_with_cache(&dir, true, Some(&cache))
                .unwrap()
                .total,
            5000
        );
        let warm = started.elapsed();
        let config = OrganizerConfig {
            base_path: dir.clone(),
            dry_run: false,
            scan_all_months: true,
            single_folder: false,
            template: "{world}".to_string(),
        };
        let mut stats = OrganizerStats::default();
        let written = proc_io_wchar();
        let started = Instant::now();
        organize_path(&config, &mut stats).unwrap();
        let organize = started.elapsed();
        let written = proc_io_wchar() - written;
        let undo_size = fs::metadata(dir.join(".vrchat-organizer-undo.json"))
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        let mut undo_stats = OrganizerStats::default();
        let started = Instant::now();
        undo_organization(&dir, &mut undo_stats).unwrap();
        let undo = started.elapsed();
        println!(
            "perf: cold_scan={cold:?} warm_scan={warm:?} organize={organize:?} organized={} \
             bytes_written_during_organize={written} undo_log_bytes={undo_size} undo={undo:?} undone={} {}",
            stats.organized,
            undo_stats.undone,
            fs::read_to_string("/proc/self/status")
                .unwrap_or_default()
                .lines()
                .find(|line| line.starts_with("VmHWM"))
                .unwrap_or_default()
        );
        assert_eq!(stats.organized, 5000);
        assert_eq!(undo_stats.undone, 5000);
        let _ = fs::remove_dir_all(&dir);
    }

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
    fn sanitizes_windows_reserved_names() {
        assert_eq!(sanitize_name("CON"), "CON_");
        assert_eq!(sanitize_name("com1.gallery"), "com1.gallery_");
        assert_eq!(sanitize_name("LPT9"), "LPT9_");
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
    fn classifies_metadata_before_path_fallback() {
        assert_eq!(
            classify_world(
                Path::new("2026-09/Folder/capture.png"),
                Some("Metadata World"),
                true,
                2
            ),
            Some("Metadata World".to_string())
        );
    }

    #[test]
    fn metadata_world_requires_two_photos() {
        let path = Path::new("2026-09/Folder/capture.png");
        assert_eq!(classify_world(path, Some("Metadata World"), true, 1), None);
        assert_eq!(
            classify_world(path, Some("Metadata World"), true, 2),
            Some("Metadata World".to_string())
        );
    }

    #[test]
    fn rejects_metadata_placeholders_case_insensitively_and_with_whitespace() {
        let path = Path::new("capture.png");
        for name in [
            " capture ",
            "SCREENSHOT",
            "VRChat_2026-09-20",
            "Unknown.PNG",
            "photo.jpg",
            "image.webp",
        ] {
            assert_eq!(classify_world(path, Some(name), false, 2), None, "{name}");
        }
    }

    #[test]
    fn accepts_unicode_metadata_world_names() {
        assert_eq!(
            classify_world(
                Path::new("世界/capture.png"),
                Some("  夜の世界 🌙  "),
                false,
                2
            ),
            Some("夜の世界 🌙".to_string())
        );
    }

    #[test]
    fn parsed_metadata_obeys_photo_count_qualification() {
        let entries = vec![PngTextEntry {
            keyword: "Description".to_string(),
            value: r#"{"world":{"name":"真实世界 🌏"}}"#.to_string(),
        }];
        let (world, _, _, _) = parse_png_entries(&entries);
        let path = Path::new("2026-09/Folder/capture.png");
        assert_eq!(classify_world(path, world.as_deref(), true, 1), None);
        assert_eq!(
            classify_world(path, world.as_deref(), true, 2),
            Some("真实世界 🌏".to_string())
        );
    }

    #[test]
    fn never_classifies_a_root_filename_as_a_world() {
        assert_eq!(
            classify_world(Path::new("VRChat_2026-09-20_capture.png"), None, true, 2),
            None
        );
        assert_eq!(
            classify_world(Path::new("capture.png"), None, false, 2),
            None
        );
    }

    #[test]
    fn classifies_only_known_world_folder_layouts() {
        assert_eq!(
            classify_world(Path::new("2026-09/Black Cat/capture.png"), None, true, 2),
            Some("Black Cat".to_string())
        );
        assert_eq!(
            classify_world(Path::new("Black Cat/capture.png"), None, false, 2),
            Some("Black Cat".to_string())
        );
        assert_eq!(
            classify_world(Path::new("New folder/capture.png"), None, true, 2),
            None
        );
    }

    #[test]
    fn requires_at_least_two_files_for_path_fallback() {
        assert_eq!(
            classify_world(Path::new("2026-09/Black Cat/capture.png"), None, true, 1),
            None
        );
    }

    #[test]
    fn ignores_reserved_and_filename_like_fallback_names() {
        assert_eq!(
            classify_world(Path::new("2026-09/Prints/capture.png"), None, true, 2),
            None
        );
        assert_eq!(
            classify_world(Path::new(".hidden/capture.png"), None, false, 2),
            None
        );
        assert_eq!(
            classify_world(Path::new("2026-09/VRChat_2026-01-01_a.png"), None, true, 2),
            None
        );
        assert_eq!(
            classify_world(Path::new("2026-09/photo.png"), None, true, 2),
            None
        );
        assert_eq!(
            classify_world(Path::new("2026-09/capture.png"), None, true, 2),
            None
        );
    }

    #[test]
    fn resolve_template_replaces_all_variables() {
        let meta = ImageMeta {
            world_name: Some("Black Cat".to_string()),
            participants: Vec::new(),
            tagged_participants: Vec::new(),
            captured_at: None,
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
            participants: Vec::new(),
            tagged_participants: Vec::new(),
            captured_at: None,
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
            participants: Vec::new(),
            tagged_participants: Vec::new(),
            captured_at: None,
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
    fn ignores_generated_thumbnail_cache() {
        assert!(is_ignored_path(
            Path::new("/tmp/vrchat/vrchat-organizer-thumbnails/cover.jpg"),
            Path::new("/tmp/vrchat")
        ));
        assert!(is_ignored_path(
            Path::new("/tmp/vrchat/2025-01/Prints/cover.png"),
            Path::new("/tmp/vrchat")
        ));
        assert!(!is_ignored_path(
            Path::new("/tmp/vrchat/2025-01/Black Cat/cover.png"),
            Path::new("/tmp/vrchat")
        ));
        assert!(!is_ignored_path(
            Path::new("/home/user/.local/share/Steam/Pictures/VRChat/cover.png"),
            Path::new("/home/user/.local/share/Steam/Pictures/VRChat")
        ));
    }

    #[test]
    fn new_real_run_replaces_previous_undo_history() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-undo-reset-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let old_entry = UndoEntry {
            original_path: dir.join("old.png").display().to_string(),
            destination_path: dir.join("old-destination.png").display().to_string(),
        };
        log_undo_entry(&dir, &old_entry).unwrap();
        let config = OrganizerConfig {
            base_path: dir.clone(),
            dry_run: false,
            scan_all_months: false,
            single_folder: true,
            template: "{world}".to_string(),
        };
        let mut stats = OrganizerStats::default();
        // A run that moves nothing keeps the previous history...
        organize_path(&config, &mut stats).unwrap();
        assert_eq!(read_undo_log(&dir).unwrap().len(), 1);
        // ...and the first real move of a new run replaces it.
        let photo = dir.join("VRChat_2026-09-20_10-00-00.000_8x8.png");
        fs::write(&photo, vrchat_png(Some("World"), 8, 8)).unwrap();
        organize_path(&config, &mut stats).unwrap();
        let log = read_undo_log(&dir).unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0]
            .original_path
            .ends_with("VRChat_2026-09-20_10-00-00.000_8x8.png"));
        let _ = std::fs::remove_dir_all(&dir);
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
    fn vrchat_xmp_world_name_is_read_and_unmangled() {
        // Shape copied from a real VRChat 2026 screenshot (iTXt, uncompressed).
        let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description><xmp:CreatorTool>VRChat</xmp:CreatorTool></rdf:Description><rdf:Description xmlns:vrc="http://ns.vrchat.com/vrc/1.0/"><vrc:WorldID>wrld_x</vrc:WorldID><vrc:WorldDisplayName>The Pool Parlor ｜ 8 Ball Pool</vrc:WorldDisplayName></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let mut itxt = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
        itxt.extend_from_slice(xmp.as_bytes());
        let mut png = vrchat_png(None, 2, 2);
        png.splice(33..33, png_chunk(b"iTXt", &itxt));
        let dir = fresh_dir("xmp-world");
        let path = dir.join("VRChat_2026-10-07_23-32-04.272_7680x4320.png");
        fs::write(&path, png).unwrap();
        let meta = extract_image_meta(&path).unwrap();
        // Same folder name VRCX's "The Pool Parlor | 8 Ball Pool" sanitizes to.
        assert_eq!(
            meta.world_name.as_deref(),
            Some("The Pool Parlor _ 8 Ball Pool")
        );
        assert!(meta.participants.is_empty());
        let _ = fs::remove_dir_all(&dir);

        for (raw, expected) in [
            (
                "Break ＃20 - Hotbox （update July․19）",
                Some("Break #20 - Hotbox (update July.19)"),
            ),
            (
                "Furality Sylva˸ Lahaina Springs",
                Some("Furality Sylva_ Lahaina Springs"),
            ),
            ("［ HOME ］ YTS‚ Quest ǃ", Some("[ HOME ] YTS, Quest !")),
            (
                "Last Generation⁄最後の世代",
                Some("Last Generation_最後の世代"),
            ),
            (
                "Ｔｏｋｙｏ ２ &amp; Cat&apos;s",
                Some("Ｔｏｋｙｏ ２ & Cat's"),
            ),
            ("   ", None),
        ] {
            let xmp = format!("<vrc:WorldDisplayName>{raw}</vrc:WorldDisplayName>");
            assert_eq!(parse_vrchat_xmp(&xmp).as_deref(), expected, "{raw}");
        }
        // Lightroom rewrites XMP into attribute form.
        assert_eq!(
            parse_vrchat_xmp(r#"<rdf:Description vrc:WorldDisplayName="Cozy Cabin"/>"#).as_deref(),
            Some("Cozy Cabin")
        );

        // VRCX JSON wins over XMP; XMP replaces VRCX's empty world name.
        let entry = |keyword: &str, value: &str| PngTextEntry {
            keyword: keyword.to_string(),
            value: value.to_string(),
        };
        let xmp = || {
            entry(
                "XML:com.adobe.xmp",
                "<vrc:WorldDisplayName>From XMP</vrc:WorldDisplayName>",
            )
        };
        let (world, ..) = parse_png_entries(&[
            xmp(),
            entry("Description", r#"{"world":{"name":"From VRCX"}}"#),
        ]);
        assert_eq!(world.as_deref(), Some("From VRCX"));
        let (world, ..) =
            parse_png_entries(&[entry("Description", r#"{"world":{"name":""}}"#), xmp()]);
        assert_eq!(world.as_deref(), Some("From XMP"));
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

        // tEXt chunk with world and participant metadata.
        let mut text = b"Description\0".to_vec();
        text.extend_from_slice(
            br#"{"world":{"name":"Black Cat"},"players":[{"displayName":"Alice"},{"displayName":"Bob"}]}"#,
        );
        png.extend_from_slice(&(text.len() as u32).to_be_bytes());
        png.extend_from_slice(b"tEXt");
        png.extend_from_slice(&text);
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

        let (world, _software, participants, tagged_participants) = parse_png_entries(&entries);
        assert_eq!(world.as_deref(), Some("Black Cat"));
        assert_eq!(participants, vec!["Alice".to_string(), "Bob".to_string()]);
        assert!(tagged_participants.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stored_tags_survive_reload_rescan_move_and_edit() {
        let dir =
            std::env::temp_dir().join(format!("vrchat-organizer-tag-store-{}", std::process::id()));
        let world = dir.join("2026-08").join("The Pool Parlor _ 8 Ball Pool");
        std::fs::create_dir_all(&world).unwrap();
        let first = world.join("VRChat_2026-08-30_21-30-00.000_1920x1080.png");
        let second = world.join("VRChat_2026-08-30_21-31-00.000_1920x1080.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .save(&first)
            .unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([11, 20, 30, 255]))
            .save(&second)
            .unwrap();

        assert_eq!(
            tag_photo_participant_in_store(&dir, &first, "Ćóâl").unwrap(),
            vec!["Ćóâl"]
        );
        assert_eq!(
            tag_photo_participant_in_store(&dir, &first, "Floki-AutumnFox").unwrap(),
            vec!["Ćóâl", "Floki-AutumnFox"]
        );
        let cache = dir.join("library.sqlite");
        let first_scan = scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        let photo = first_scan
            .world_details
            .iter()
            .flat_map(|world| &world.photos)
            .find(|photo| photo.path == first.to_string_lossy())
            .unwrap();
        assert_eq!(photo.tagged_participants, vec!["Ćóâl", "Floki-AutumnFox"]);

        // A fresh scan represents an app restart and a cache rebuild.
        let restarted = scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        assert_eq!(
            restarted
                .world_details
                .iter()
                .flat_map(|world| &world.photos)
                .find(|photo| photo.path == first.to_string_lossy())
                .unwrap()
                .tagged_participants,
            vec!["Ćóâl", "Floki-AutumnFox"]
        );

        let moved = world.join("moved.png");
        std::fs::rename(&first, &moved).unwrap();
        let after_move = scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        let moved_photo = after_move
            .world_details
            .iter()
            .flat_map(|world| &world.photos)
            .find(|photo| photo.path == moved.to_string_lossy())
            .unwrap();
        assert_eq!(
            moved_photo.tagged_participants,
            vec!["Ćóâl", "Floki-AutumnFox"]
        );

        assert_eq!(
            set_photo_tags(&dir, &moved, &["Floki-AutumnFox".to_string()]).unwrap(),
            vec!["Floki-AutumnFox"]
        );
        assert_eq!(
            set_photo_tags(&dir, &moved, &[]).unwrap(),
            Vec::<String>::new()
        );
        assert!(scan_library_with_cache(&dir, true, Some(&cache))
            .unwrap()
            .world_details
            .iter()
            .flat_map(|world| &world.photos)
            .find(|photo| photo.path == moved.to_string_lossy())
            .unwrap()
            .tagged_participants
            .is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_legacy_embedded_tags_and_preserves_them_in_separate_store() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-tag-migration-{}",
            std::process::id()
        ));
        let world = dir.join("2026-08").join("The Pool Parlor");
        std::fs::create_dir_all(&world).unwrap();
        let path = world.join("VRChat_2026-08-30_21-30-00.000_1920x1080.png");
        let mut png = vrchat_png(None, 2, 2);
        let mut text = ORGANIZER_TAG_PREFIX.to_vec();
        text.extend_from_slice(r#"["Ćóâl"]"#.as_bytes());
        png.splice(33..33, png_chunk(b"tEXt", &text));
        fs::write(&path, png).unwrap();
        let cache = dir.join("library.sqlite");
        let stats = scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        let photo = stats
            .unorganized_photos
            .iter()
            .find(|photo| photo.path == path.to_string_lossy())
            .unwrap();
        assert_eq!(photo.tagged_participants, vec!["Ćóâl"]);
        let store = fs::read_to_string(dir.join(TAG_STORE_FILE)).unwrap();
        assert!(store.contains("Ćóâl"));
        assert_eq!(load_tag_store(&dir).unwrap().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_rebuild_keeps_tags_and_records_fingerprint() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-cache-tags-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("VRChat_2026-08-30_21-30-00.000_1920x1080.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .save(&path)
            .unwrap();
        set_photo_tags(&dir, &path, &["Alice".to_string()]).unwrap();
        let cache = dir.join("library.sqlite");
        scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        let connection = Connection::open(&cache).unwrap();
        let fingerprint: Option<String> = connection
            .query_row(
                "SELECT fingerprint FROM photos WHERE path = ?1",
                [path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(fingerprint.is_some());
        let rebuilt = scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        let photo = rebuilt
            .unorganized_photos
            .iter()
            .find(|photo| photo.path == path.to_string_lossy())
            .unwrap();
        assert_eq!(photo.tagged_participants, vec!["Alice"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_tag_store_is_backed_up_before_recovery() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-corrupt-tags-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(TAG_STORE_FILE);
        fs::write(&path, "{not json").unwrap();
        assert!(load_tag_store(&dir).unwrap().is_empty());
        let backup = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| path.to_string_lossy().contains(".corrupt."));
        assert!(backup.is_some());
        save_tag_store(&dir, &HashMap::new()).unwrap();
        assert!(path.is_file());
        assert!(!dir
            .join(format!("{TAG_STORE_FILE}.tmp.{}", std::process::id()))
            .exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_images_share_tags_but_edited_content_gets_new_identity() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-fingerprint-edge-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.png");
        let second = dir.join("second.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .save(&first)
            .unwrap();
        fs::copy(&first, &second).unwrap();
        set_photo_tags(&dir, &first, &["Alice".to_string()]).unwrap();
        let identical = scan_library_with_options(&dir, true).unwrap();
        assert!(identical
            .unorganized_photos
            .iter()
            .all(|photo| photo.tagged_participants == vec!["Alice"]));
        image::RgbaImage::from_pixel(2, 2, image::Rgba([11, 20, 30, 255]))
            .save(&first)
            .unwrap();
        let edited = scan_library_with_options(&dir, true).unwrap();
        assert!(edited
            .unorganized_photos
            .iter()
            .find(|photo| photo.path == first.to_string_lossy())
            .unwrap()
            .tagged_participants
            .is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_cold_and_cached_scan_timings_for_1001_photos() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-performance-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let fixture = dir.join("fixture.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .save(&fixture)
            .unwrap();
        for index in 0..1001 {
            fs::copy(
                &fixture,
                dir.join(format!("VRChat_2026-08-30_{index:04}.png")),
            )
            .unwrap();
        }
        fs::remove_file(fixture).unwrap();
        let cache = dir.join("library.sqlite");
        let cold_start = std::time::Instant::now();
        scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        let cold = cold_start.elapsed();
        let cached_start = std::time::Instant::now();
        scan_library_with_cache(&dir, true, Some(&cache)).unwrap();
        let cached = cached_start.elapsed();
        println!(
            "performance fixture: photos=1001 cold_scan={cold:?} existing_cache_scan={cached:?}"
        );
        assert!(cache.is_file());
        let _ = fs::remove_dir_all(&dir);
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

    #[test]
    fn library_scan_includes_images_at_selected_folder_root() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-test-library-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("VRChat_2025-01-12_foo.png");
        image::RgbImage::new(1, 1).save(&image_path).unwrap();

        let stats = scan_library(&dir).unwrap();

        assert!(stats.world_details.is_empty());
        assert_eq!(stats.unorganized_photos.len(), 1);
        assert_eq!(stats.total, 1);
        assert_eq!(
            stats.unorganized_photos[0].path,
            image_path.to_string_lossy()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn organize_scan_includes_root_images_when_month_folders_exist() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-test-root-organize-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("2026-09")).unwrap();
        let image_path = dir.join("VRChat_2026-09-20_root.png");
        image::RgbImage::new(1, 1).save(&image_path).unwrap();

        let config = OrganizerConfig {
            base_path: dir.clone(),
            dry_run: true,
            scan_all_months: false,
            single_folder: false,
            template: "{world}".to_string(),
        };
        let mut stats = OrganizerStats::default();
        organize_path(&config, &mut stats).unwrap();

        assert_eq!(stats.processed, 1);
        assert_eq!(stats.no_metadata, 1);
        assert!(image_path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_library_scan_includes_root_images_with_month_folders() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-test-root-library-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("2026-09").join("World")).unwrap();
        image::RgbImage::new(1, 1)
            .save(dir.join("root-capture.png"))
            .unwrap();
        image::RgbImage::new(1, 1)
            .save(dir.join("2026-09").join("World").join("dated-capture.png"))
            .unwrap();
        image::RgbImage::new(1, 1)
            .save(
                dir.join("2026-09")
                    .join("World")
                    .join("dated-capture-2.png"),
            )
            .unwrap();

        let stats = scan_library(&dir).unwrap();

        assert_eq!(stats.world_details.len(), 1);
        assert_eq!(stats.world_details[0].name, "World");
        assert_eq!(stats.world_details[0].photos.len(), 2);
        assert_eq!(stats.unorganized_photos.len(), 1);
        assert_eq!(stats.total, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_scan_caches_thumbnails_for_special_character_paths() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-test-special-path-{}",
            std::process::id()
        ));
        let world_dir = dir.join("A [strange] 世界");
        std::fs::create_dir_all(&world_dir).unwrap();
        let image_path = world_dir.join("capture with spaces [1].png");
        image::RgbImage::new(16, 9).save(&image_path).unwrap();

        let stats = scan_library_with_options(&dir, true).unwrap();
        let photo = &stats.unorganized_photos[0];

        assert!(stats.world_details.is_empty());
        assert_eq!(photo.path, image_path.to_string_lossy());
        assert_eq!(photo.world_name, None);
        generate_thumbnail(&image_path, &dir).unwrap();
        assert!(Path::new(&photo.thumbnail_path).is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn full_library_scan_uses_date_folder_world_fallback_and_ignores_arbitrary_folder_names() {
        let dir = std::env::temp_dir().join(format!(
            "vrchat-organizer-test-library-layout-{}",
            std::process::id()
        ));
        let dated_world = dir.join("2026-09").join("Known World");
        let arbitrary = dir.join("New folder");
        std::fs::create_dir_all(&dated_world).unwrap();
        std::fs::create_dir_all(&arbitrary).unwrap();
        let dated_image = dated_world.join("capture.png");
        let dated_image_two = dated_world.join("capture-2.png");
        let arbitrary_image = arbitrary.join("capture.png");
        image::RgbImage::new(1, 1).save(&dated_image).unwrap();
        image::RgbImage::new(1, 1).save(&dated_image_two).unwrap();
        image::RgbImage::new(1, 1).save(&arbitrary_image).unwrap();

        let stats = scan_library_with_options(&dir, true).unwrap();

        assert!(stats
            .world_details
            .iter()
            .any(|world| world.name == "Known World"));
        assert!(!stats
            .world_details
            .iter()
            .any(|world| world.name == "Unsorted"));
        assert_eq!(stats.world_details[0].photos.len(), 2);
        assert_eq!(stats.unorganized_photos.len(), 1);
        assert_eq!(stats.total, 3);
        assert_eq!(
            stats
                .world_details
                .iter()
                .map(|world| world.photos.len())
                .sum::<usize>()
                + stats.unorganized_photos.len(),
            stats.total
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
