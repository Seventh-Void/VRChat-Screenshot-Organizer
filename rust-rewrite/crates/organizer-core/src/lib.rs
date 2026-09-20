use anyhow::{Context, Result};
use fast_image_resize::{images::Image as ResizeImage, pixels::PixelType, Resizer};
use flate2::read::ZlibDecoder;
use rayon::prelude::*;
use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, TryLockError};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct CachedPhoto {
    size_bytes: u64,
    modified_secs: u64,
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

fn load_cache(path: &Path) -> Result<HashMap<String, CachedPhoto>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let connection = Connection::open(path)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS photos (
            path TEXT PRIMARY KEY,
            size_bytes INTEGER NOT NULL,
            modified_secs INTEGER NOT NULL,
            world_name TEXT,
            captured_at INTEGER,
            participants TEXT NOT NULL,
            width INTEGER NOT NULL,
            height INTEGER NOT NULL,
            thumbnail_path TEXT NOT NULL
        );",
    )?;
    let mut statement = connection.prepare(
        "SELECT path,size_bytes,modified_secs,world_name,captured_at,participants,width,height,thumbnail_path FROM photos",
    )?;
    let mut rows = statement.query([])?;
    let mut result = HashMap::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let participants =
            serde_json::from_str(row.get::<_, String>(5)?.as_str()).unwrap_or_default();
        result.insert(
            path.clone(),
            CachedPhoto {
                size_bytes: row.get::<_, i64>(1)?.max(0) as u64,
                modified_secs: row.get::<_, i64>(2)?.max(0) as u64,
                photo: LibraryPhoto {
                    path,
                    thumbnail_path: row.get(8)?,
                    captured_at: row.get::<_, Option<i64>>(4)?.map(|value| value as u64),
                    world_name: row.get(3)?,
                    participants,
                    width: row.get::<_, i64>(6)?.max(0) as u32,
                    height: row.get::<_, i64>(7)?.max(0) as u32,
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
    let mut worlds: HashMap<String, Vec<LibraryPhoto>> = HashMap::new();
    let mut unorganized = Vec::new();
    for entry in cached
        .into_values()
        .filter(|entry| Path::new(&entry.photo.path).starts_with(base_path))
    {
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

fn save_cache(
    path: &Path,
    candidates: &[(Option<String>, LibraryPhoto)],
    files: &[(PathBuf, u64, u64, PathBuf)],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut connection = Connection::open(path)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS photos (
            path TEXT PRIMARY KEY,
            size_bytes INTEGER NOT NULL,
            modified_secs INTEGER NOT NULL,
            world_name TEXT,
            captured_at INTEGER,
            participants TEXT NOT NULL,
            width INTEGER NOT NULL,
            height INTEGER NOT NULL,
            thumbnail_path TEXT NOT NULL
        );",
    )?;
    let transaction = connection.transaction()?;
    for (_, photo) in candidates {
        let modified_secs = files
            .iter()
            .find(|(path, _, _, _)| path.to_string_lossy() == photo.path)
            .map(|(_, _, modified, _)| *modified)
            .unwrap_or_default();
        transaction.execute(
            "INSERT OR REPLACE INTO photos
             (path,size_bytes,modified_secs,world_name,captured_at,participants,width,height,thumbnail_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                photo.path,
                photo.size_bytes as i64,
                modified_secs as i64,
                photo.world_name,
                photo.captured_at.map(|value| value as i64),
                serde_json::to_string(&photo.participants)?,
                photo.width as i64,
                photo.height as i64,
                photo.thumbnail_path,
            ],
        )?;
    }
    let paths: std::collections::HashSet<&str> = candidates
        .iter()
        .map(|(_, photo)| photo.path.as_str())
        .collect();
    let mut stale = transaction.prepare("SELECT path FROM photos")?;
    let stale_paths: Vec<String> = stale
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?
        .into_iter()
        .filter(|path| !paths.contains(path.as_str()))
        .collect();
    drop(stale);
    for stale_path in stale_paths {
        transaction.execute("DELETE FROM photos WHERE path = ?1", [stale_path])?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn scan_library_details(base_path: &Path) -> Result<Vec<LibraryWorld>> {
    scan_library_details_with_options(base_path, false)
}

pub fn scan_library_details_with_options(
    base_path: &Path,
    scan_all_months: bool,
) -> Result<Vec<LibraryWorld>> {
    Ok(scan_library_partition_with_options(base_path, scan_all_months, None)?.0)
}

fn scan_library_partition_with_options(
    base_path: &Path,
    scan_all_months: bool,
    cache_path: Option<&Path>,
) -> Result<(Vec<LibraryWorld>, Vec<LibraryPhoto>)> {
    if !base_path.is_dir() {
        anyhow::bail!("path does not exist: {}", base_path.display());
    }

    let roots = library_roots(base_path, scan_all_months)?;
    let cached = cache_path.map(load_cache).transpose()?.unwrap_or_default();
    let mut candidates = Vec::new();
    let mut files = Vec::new();

    for root in roots {
        let walker = if !scan_all_months && root == *base_path {
            WalkDir::new(&root).max_depth(1)
        } else {
            WalkDir::new(&root)
        };
        for item in walker
            .into_iter()
            .filter_map(|item| item.ok())
            .filter(|item| item.file_type().is_file())
            .filter(|item| is_image_path(item.path()))
            .filter(|item| !is_ignored_path(item.path()))
        {
            let path = item.path().to_path_buf();
            let metadata = item.metadata()?;
            let modified_secs = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or_default();
            files.push((path, metadata.len(), modified_secs, root.clone()));
        }
    }
    let processed: Vec<_> = files
        .par_iter()
        .map(|(path, size_bytes, modified_secs, root)| {
            if let Some(cached) = cached.get(&path.to_string_lossy().into_owned()) {
                if cached.size_bytes == *size_bytes && cached.modified_secs == *modified_secs {
                    return Ok((
                        path.clone(),
                        *size_bytes,
                        *modified_secs,
                        root.clone(),
                        cached.photo.clone(),
                    ));
                }
            }
            let metadata = extract_image_meta(path).ok();
            let filesystem_captured_at = Some(*modified_secs);
            let captured_at = metadata
                .as_ref()
                .and_then(|meta| meta.captured_at)
                .or(filesystem_captured_at);
            if cancel_requested() {
                anyhow::bail!("scan cancelled");
            }
            let photo = LibraryPhoto {
                path: path.to_string_lossy().into_owned(),
                thumbnail_path: String::new(),
                captured_at,
                world_name: metadata.as_ref().and_then(|meta| meta.world_name.clone()),
                participants: metadata
                    .as_ref()
                    .map(|meta| meta.participants.clone())
                    .unwrap_or_default(),
                width: metadata.as_ref().map(|meta| meta.width).unwrap_or_default(),
                height: metadata
                    .as_ref()
                    .map(|meta| meta.height)
                    .unwrap_or_default(),
                size_bytes: *size_bytes,
            };
            Ok((
                path.clone(),
                *size_bytes,
                *modified_secs,
                root.clone(),
                photo,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    for (path, size_bytes, modified_secs, root, mut photo) in processed {
        if photo.thumbnail_path.is_empty() {
            photo.thumbnail_path = thumbnail_path_for(&path, base_path, Some(modified_secs))
                .to_string_lossy()
                .into_owned();
        }
        let relative = path.strip_prefix(&root).unwrap_or(&path);
        let world = classify_world(relative, photo.world_name.as_deref(), scan_all_months, 2);
        candidates.push((world, photo.clone()));
        if let Some(cache_path) = cache_path {
            // Cache writes are batched after classification below.
            let _ = (cache_path, size_bytes, modified_secs);
        }
    }
    if let Some(cache_path) = cache_path {
        save_cache(cache_path, &candidates, &files)?;
    }

    let mut worlds: HashMap<String, Vec<LibraryPhoto>> = HashMap::new();
    let mut unorganized_photos = Vec::new();
    for (world, photo) in candidates {
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
    let cache = base_path.join("vrchat-organizer-thumbnails");
    fs::create_dir_all(&cache)?;
    if target
        .metadata()
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
    {
        return Ok(target);
    }

    let thumbnail = match image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.decode().ok())
    {
        Some(image) => {
            let rgb = image.to_rgb8();
            let scale = (320.0 / rgb.width() as f32)
                .min(320.0 / rgb.height() as f32)
                .min(1.0);
            let width = ((rgb.width() as f32 * scale).round() as u32).max(1);
            let height = ((rgb.height() as f32 * scale).round() as u32).max(1);
            let source = ResizeImage::from_vec_u8(
                rgb.width(),
                rgb.height(),
                rgb.into_raw(),
                PixelType::U8x3,
            )?;
            let mut destination = ResizeImage::new(width, height, PixelType::U8x3);
            Resizer::new().resize(&source, &mut destination, None)?;
            image::RgbImage::from_raw(width, height, destination.into_vec())
                .map(image::DynamicImage::ImageRgb8)
                .context("thumbnail resize returned an invalid buffer")?
        }
        None => image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            320,
            180,
            image::Rgb([32, 26, 48]),
        )),
    };
    let temporary = target.with_extension("jpg.tmp");
    let mut output = fs::File::create(&temporary)?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 75);
    encoder.encode_image(&thumbnail)?;
    output.sync_all()?;
    fs::rename(temporary, &target)?;
    Ok(target)
}

pub fn generate_thumbnail(path: &Path, base_path: &Path) -> Result<PathBuf> {
    let modified = fs::metadata(path)?
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    thumbnail_for(path, base_path, modified)
}

pub fn thumbnail_path_for(path: &Path, base_path: &Path, modified: Option<u64>) -> PathBuf {
    let cache = base_path.join("vrchat-organizer-thumbnails");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "thumbnail-v2-320".hash(&mut hasher);
    path.to_string_lossy().hash(&mut hasher);
    modified.hash(&mut hasher);
    cache.join(format!("{:x}.jpg", hasher.finish()))
}

fn is_image_path(path: &Path) -> bool {
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

fn is_ignored_path(path: &Path) -> bool {
    path.components().any(|component| {
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
    let started = std::time::Instant::now();
    let entries = read_undo_log(base_path)?;
    if entries.is_empty() {
        anyhow::bail!("no undo entries found");
    }

    let mut remaining = Vec::new();
    for entry in &entries {
        let dest = PathBuf::from(&entry.destination_path);
        let orig = PathBuf::from(&entry.original_path);

        if !dest.exists() {
            // File was already moved or deleted; skip
            stats.errors += 1;
            remaining.push(entry.clone());
            continue;
        }

        if let Some(parent) = orig.parent() {
            fs::create_dir_all(parent)?;
        }
        if orig.exists() {
            stats.errors += 1;
            remaining.push(entry.clone());
            continue;
        }
        // Use the same cross-volume-safe move helper as organization.
        move_file(&dest, &orig)?;
        stats.undone += 1;
        if let Some(parent) = dest.parent() {
            let is_empty = fs::read_dir(parent)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if is_empty {
                let _ = fs::remove_dir(parent);
            }
        }
        println!(
            "undone: moved {} back to {}",
            dest.display(),
            orig.display()
        );
    }

    // Keep entries that could not be restored so the user can resolve the
    // conflict and retry instead of losing the undo history.
    let undo_path = base_path.join(".vrchat-organizer-undo.json");
    if remaining.is_empty() {
        let _ = fs::remove_file(&undo_path);
    } else {
        let data = serde_json::to_string_pretty(&remaining)?;
        fs::write(&undo_path, data)?;
    }

    log_activity(
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
    )
    .context("failed to write activity log")?;

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
    } else if is_windows_reserved_name(&cleaned) {
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

/// Read PNG text chunks without touching IDAT pixel data.
fn extract_png_text_chunks(path: &Path) -> Result<Vec<PngTextEntry>> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut signature = [0u8; 8];
    reader.read_exact(&mut signature)?;
    if signature != [137, 80, 78, 71, 13, 10, 26, 10] {
        anyhow::bail!("not a valid PNG file");
    }
    let mut entries = Vec::new();
    loop {
        let mut header = [0u8; 8];
        if reader.read_exact(&mut header).is_err() {
            break;
        }
        let chunk_len = u32::from_be_bytes(header[..4].try_into().unwrap()) as u64;
        let chunk_type = &header[4..8];
        if chunk_type == b"IDAT" {
            break;
        }
        if chunk_type == b"IEND" {
            break;
        }
        if chunk_type == b"tEXt" || chunk_type == b"zTXt" || chunk_type == b"iTXt" {
            if chunk_len > 16 * 1024 * 1024 {
                anyhow::bail!("PNG text chunk is too large");
            }
            let mut data = vec![0u8; chunk_len as usize];
            reader.read_exact(&mut data)?;
            if let Some(null_pos) = data.iter().position(|&byte| byte == 0) {
                let keyword = String::from_utf8_lossy(&data[..null_pos]).into_owned();
                let raw_value = &data[null_pos + 1..];
                let value = match chunk_type {
                    b"tEXt" => raw_value.iter().map(|&byte| byte as char).collect(),
                    b"zTXt" if !raw_value.is_empty() => {
                        let mut value = String::new();
                        let _ = ZlibDecoder::new(&raw_value[1..]).read_to_string(&mut value);
                        value
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
                            Some(text) => {
                                let mut value = String::new();
                                let _ = ZlibDecoder::new(text).read_to_string(&mut value);
                                value
                            }
                            None => String::new(),
                        }
                    }
                    _ => String::new(),
                };
                entries.push(PngTextEntry { keyword, value });
            }
        } else {
            reader.seek(SeekFrom::Current(chunk_len as i64))?;
        }
        reader.seek(SeekFrom::Current(4))?;
    }
    if entries.is_empty() {
        return extract_png_text_chunks_full(path);
    }
    Ok(entries)
}

fn extract_png_text_chunks_full(path: &Path) -> Result<Vec<PngTextEntry>> {
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
fn parse_png_entries(entries: &[PngTextEntry]) -> (Option<String>, Option<String>, Vec<String>) {
    let mut world_name: Option<String> = None;
    let mut software: Option<String> = None;
    let mut participants = Vec::new();

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
                    if let Some(players) =
                        parsed.get("players").and_then(|players| players.as_array())
                    {
                        participants = players
                            .iter()
                            .filter_map(|player| {
                                player
                                    .get("displayName")
                                    .and_then(|name| name.as_str())
                                    .map(str::trim)
                                    .filter(|name| !name.is_empty())
                                    .map(ToOwned::to_owned)
                            })
                            .collect();
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

    (world_name, software, participants)
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
                if let Ok(value) = std::str::from_utf8(first) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
                        if let Some(world) = parsed
                            .get("world")
                            .and_then(|w| w.get("name"))
                            .and_then(|n| n.as_str())
                        {
                            world_name = Some(sanitize_name(world));
                        }
                        if let Some(players) =
                            parsed.get("players").and_then(|players| players.as_array())
                        {
                            participants = players
                                .iter()
                                .filter_map(|player| {
                                    player
                                        .get("displayName")
                                        .and_then(|name| name.as_str())
                                        .map(str::trim)
                                        .filter(|name| !name.is_empty())
                                        .map(ToOwned::to_owned)
                                })
                                .collect();
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

    let (world_name, software, participants) = match ext.as_deref() {
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
        _ => (None, None, Vec::new()),
    };

    Ok(ImageMeta {
        world_name,
        participants,
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
    datetime.and_utc().timestamp().try_into().ok()
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
    if !config.dry_run {
        let undo_path = config.base_path.join(".vrchat-organizer-undo.json");
        let _ = fs::remove_file(undo_path);
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if config.single_folder || config.scan_all_months {
        // A full scan starts at the selected root so captures sitting beside
        // month folders are included as well.
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
        process_folder(&folder, config, stats)?;
    }

    if !config.single_folder && !config.scan_all_months {
        // Process root-level captures after the selected month traversal so a
        // moved root file cannot be discovered a second time in the same run.
        let mut root_files = Vec::new();
        for entry in fs::read_dir(&config.base_path)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && is_image_path(&entry.path())
                && !is_ignored_path(&entry.path())
            {
                root_files.push(entry.path());
            }
        }
        root_files.sort();
        for image_file in root_files {
            if cancel_requested() {
                anyhow::bail!("scan cancelled");
            }
            if let Err(error) = organize_single_file_unlocked(&image_file, config, stats) {
                eprintln!("error processing {}: {error}", image_file.display());
            }
        }
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
    organize_single_file_unlocked(file_path, config, stats)
}

fn organize_single_file_unlocked(
    file_path: &Path,
    config: &OrganizerConfig,
    stats: &mut OrganizerStats,
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
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
                println!("ensured folder: {}", parent.display());
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
            println!("moved {} -> {}", file_path.display(), destination.display());
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
    let mut image_files = Vec::new();
    for entry in WalkDir::new(folder) {
        if cancel_requested() {
            anyhow::bail!("scan cancelled");
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.file_type().is_file() {
            let path = entry.into_path();
            if is_image_path(&path) && !is_ignored_path(&path) {
                image_files.push(path);
            }
        }
    }
    image_files.sort();

    for image_file in image_files {
        if cancel_requested() {
            anyhow::bail!("scan cancelled");
        }
        // Delegate to organize_single_file which handles stats, metadata, template resolution, undo
        if let Err(e) = organize_single_file_unlocked(&image_file, config, stats) {
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
        let (world, _, _) = parse_png_entries(&entries);
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
    fn ignores_generated_thumbnail_cache() {
        assert!(is_ignored_path(Path::new(
            "/tmp/vrchat/vrchat-organizer-thumbnails/cover.jpg"
        )));
        assert!(is_ignored_path(Path::new(
            "/tmp/vrchat/2025-01/Prints/cover.png"
        )));
        assert!(!is_ignored_path(Path::new(
            "/tmp/vrchat/2025-01/Black Cat/cover.png"
        )));
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
        organize_path(&config, &mut stats).unwrap();
        assert!(read_undo_log(&dir).unwrap().is_empty());
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

        let (world, _software, participants) = parse_png_entries(&entries);
        assert_eq!(world.as_deref(), Some("Black Cat"));
        assert_eq!(participants, vec!["Alice".to_string(), "Bob".to_string()]);

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
