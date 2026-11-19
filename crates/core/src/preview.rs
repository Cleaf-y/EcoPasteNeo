//! Lightweight filesystem data for native history cards. No UI or clipboard access.

use std::path::{Path, PathBuf};

use crate::paths::DataPaths;

const THUMBNAIL_MAX: u32 = 300;

/// A persisted file reference, including useful information when the file is gone.
#[derive(Clone, Debug)]
pub struct FilePreview {
    pub path: PathBuf,
    pub name: String,
    pub is_directory: bool,
    pub exists: bool,
    pub size: Option<u64>,
}

/// Inspect only the requested prefix of a files record, preserving stored type hints.
pub fn file_previews(content: &str, file_types: Option<&str>, limit: usize) -> Vec<FilePreview> {
    let types: Vec<_> = file_types.unwrap_or_default().split(',').collect();
    content
        .lines()
        .filter(|line| !line.is_empty())
        .take(limit)
        .enumerate()
        .map(|(index, line)| {
            let path = PathBuf::from(line);
            let metadata = std::fs::metadata(&path).ok();
            FilePreview {
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| line.to_owned()),
                is_directory: metadata
                    .as_ref()
                    .map(|meta| meta.is_dir())
                    .unwrap_or(types.get(index) == Some(&"d")),
                exists: metadata.is_some(),
                size: metadata
                    .as_ref()
                    .filter(|meta| meta.is_file())
                    .map(|meta| meta.len()),
                path,
            }
        })
        .collect()
}

/// Resolve a hash-named image inside the Neo resource root and cache a small PNG.
///
/// Decoding runs on the caller's worker thread. Invalid names and linked paths are
/// refused, so a database record cannot redirect image reads outside this store.
pub fn image_thumbnail(paths: &DataPaths, file_name: &str) -> Option<PathBuf> {
    let source = image_resource_path(paths, "origin", file_name)?;
    let target = image_resource_path(paths, "thumbnails", file_name)?;
    if target.is_file() {
        return Some(target);
    }
    let mut reader = image::ImageReader::open(&source).ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let thumbnail = reader
        .decode()
        .ok()?
        .thumbnail(THUMBNAIL_MAX, THUMBNAIL_MAX);
    let parent = target.parent()?;
    std::fs::create_dir_all(parent).ok()?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).ok()?;
    thumbnail
        .write_to(&mut temporary, image::ImageFormat::Png)
        .ok()?;
    match temporary.persist_noclobber(&target) {
        Ok(_) => Some(target),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Some(target),
        Err(_) => None,
    }
}

/// Reject linked components before following a persisted resource reference.
fn has_symlink(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let mut current = root.to_path_buf();
    if std::fs::symlink_metadata(&current).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return true;
    }
    for part in relative.components() {
        current.push(part);
        if std::fs::symlink_metadata(&current).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return true;
        }
    }
    false
}

/// A verified original image, never a card thumbnail.
#[derive(Debug)]
pub struct OriginalImage {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

/// Resolve an original using the same filename and linked-path checks as thumbnails.
pub fn image_original(paths: &DataPaths, file_name: &str) -> Option<OriginalImage> {
    let path = image_resource_path(paths, "origin", file_name)?;
    let (width, height) = image::image_dimensions(&path).ok()?;
    Some(OriginalImage {
        path,
        width,
        height,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum DragFilesError {
    #[error("History record no longer exists")]
    RecordMissing,
    #[error("All source files are missing")]
    FilesMissing,
    #[error("Original image is missing or unavailable")]
    ImageMissing,
    #[error("This record cannot be dragged as files")]
    Unsupported,
    #[error("Sensitive content is hidden")]
    Sensitive,
}

/// Resolve current file references on a worker without decoding drag-preview images.
/// Images must be verified originals in this Neo store, never cached thumbnails.
#[cfg(feature = "sqlx")]
pub fn drag_files(
    paths: &DataPaths,
    item: &crate::db::models::ClipboardItem,
) -> Result<Vec<PathBuf>, DragFilesError> {
    use crate::db::models::ClipboardKind;
    if item.is_sensitive {
        return Err(DragFilesError::Sensitive);
    }
    match item.kind {
        ClipboardKind::Files => {
            let files: Vec<_> =
                file_previews(&item.content, item.file_types.as_deref(), usize::MAX)
                    .into_iter()
                    .filter(|file| file.exists)
                    .map(|file| file.path)
                    .collect();
            if files.is_empty() {
                return Err(DragFilesError::FilesMissing);
            }
            Ok(files)
        }
        ClipboardKind::Image => image_original(paths, &item.content)
            .map(|image| vec![image.path])
            .ok_or(DragFilesError::ImageMissing),
        ClipboardKind::Text => Err(DragFilesError::Unsupported),
    }
}

/// Validate a single image reference before allowing a filesystem read or write.
fn image_resource_path(paths: &DataPaths, directory: &str, file_name: &str) -> Option<PathBuf> {
    let hash = file_name.strip_suffix(".png")?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let resources = paths.resources_dir();
    let path = resources
        .join("clipboard-images")
        .join(directory)
        .join(&hash[..2])
        .join(file_name);
    (!has_symlink(&resources, &path)).then_some(path)
}

/// The display payload contains no original sensitive content.
#[derive(Debug)]
pub enum PreviewContent {
    Text(String),
    Image(Option<OriginalImage>),
    Files(Vec<FilePreview>),
    Hidden,
}

/// Prepare a full record for display on a worker, enforcing masking before resource access.
#[cfg(feature = "sqlx")]
pub fn record_content(
    paths: &DataPaths,
    item: &crate::db::models::ClipboardItem,
    redact_secrets: bool,
) -> PreviewContent {
    use crate::{content::ClipboardSubKind, db::models::ClipboardKind};

    if item.is_sensitive && redact_secrets {
        return PreviewContent::Hidden;
    }
    match item.kind {
        ClipboardKind::Text => {
            let text = match item.sub_kind {
                Some(ClipboardSubKind::Html | ClipboardSubKind::Rtf) => {
                    item.search_text.as_deref().unwrap_or_default()
                }
                _ => &item.content,
            };
            PreviewContent::Text(text.to_owned())
        }
        ClipboardKind::Image => PreviewContent::Image(image_original(paths, &item.content)),
        ClipboardKind::Files => PreviewContent::Files(file_previews(
            &item.content,
            item.file_types.as_deref(),
            usize::MAX,
        )),
    }
}
