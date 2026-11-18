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
    let hash = file_name.strip_suffix(".png")?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let store = paths.resources_dir().join("clipboard-images");
    let source = store.join("origin").join(&hash[..2]).join(file_name);
    let target = store.join("thumbnails").join(&hash[..2]).join(file_name);
    let resources = paths.resources_dir();
    if has_symlink(&resources, &source) || has_symlink(&resources, &target) {
        return None;
    }
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
