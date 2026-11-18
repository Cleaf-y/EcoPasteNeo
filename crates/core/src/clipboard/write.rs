//! Shared writeback formats. Preparing a write never touches the system clipboard.

use super::{CaptureError, ImageStore, Result};
use crate::db::{
    items::content_hash,
    models::{ClipboardItem, ClipboardKind, ClipboardSubKind},
};
use crate::writeback::WritebackGuard;

#[derive(Debug, PartialEq)]
pub enum WritePayload {
    Text {
        plain: String,
        rich: Option<(ClipboardSubKind, String)>,
    },
    Image(Vec<u8>),
    Files(Vec<String>),
}

pub struct PreparedWrite {
    pub payload: WritePayload,
    fingerprints: Vec<String>,
}

/// Preserve the existing independent text and file-path paste preferences.
pub fn should_write_plain_for_paste(
    force: bool,
    kind: ClipboardKind,
    text: bool,
    files: bool,
) -> bool {
    force || kind == ClipboardKind::Text && text || kind == ClipboardKind::Files && files
}

impl PreparedWrite {
    /// Resolve full stored content and validate resources before native mutation.
    pub fn new(store: &ImageStore, item: &ClipboardItem, plain: bool) -> Result<Self> {
        let (payload, fingerprints) = match item.kind {
            ClipboardKind::Text => {
                let rich = (!plain
                    && matches!(
                        item.sub_kind,
                        Some(ClipboardSubKind::Html | ClipboardSubKind::Rtf)
                    ))
                .then(|| (item.sub_kind.unwrap(), item.content.clone()));
                let text = if plain || rich.is_some() {
                    item.search_text.as_ref().unwrap_or(&item.content)
                } else {
                    &item.content
                }
                .clone();
                let mut hashes = vec![content_hash(ClipboardKind::Text, text.trim())];
                if let Some((_, value)) = &rich {
                    hashes.push(content_hash(ClipboardKind::Text, value));
                }
                (WritePayload::Text { plain: text, rich }, hashes)
            }
            ClipboardKind::Files if plain => (
                WritePayload::Text {
                    plain: item.content.clone(),
                    rich: None,
                },
                vec![content_hash(ClipboardKind::Text, item.content.trim())],
            ),
            ClipboardKind::Files => {
                let paths: Vec<_> = item
                    .content
                    .lines()
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned)
                    .collect();
                if paths.is_empty() {
                    return Err(CaptureError::Clipboard("no files to write".into()));
                }
                let hash = content_hash(ClipboardKind::Files, &paths.join("\n"));
                (WritePayload::Files(paths), vec![hash])
            }
            ClipboardKind::Image => {
                let name = &item.content;
                if name.len() != 68
                    || !name.ends_with(".png")
                    || !name.as_bytes()[..64].iter().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(CaptureError::Clipboard("invalid stored image name".into()));
                }
                let bytes = std::fs::read(store.origin_path(name)).map_err(anyhow::Error::from)?;
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                    .map_err(anyhow::Error::from)?;
                let name = format!("{}.png", blake3::hash(&bytes).to_hex());
                (
                    WritePayload::Image(bytes),
                    vec![content_hash(ClipboardKind::Image, &name)],
                )
            }
        };
        Ok(Self {
            payload,
            fingerprints,
        })
    }

    /// Register every representation the configured reader may select before writing.
    pub fn suppress(&self, guard: &WritebackGuard) {
        for hash in &self.fingerprints {
            guard.suppress(hash.clone());
        }
    }

    /// Native handles are created and released within this synchronous call.
    #[cfg(feature = "system-clipboard")]
    pub fn write(&self, guard: &WritebackGuard) -> Result<()> {
        use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
        let ctx = ClipboardContext::new().map_err(clip_err)?;
        let values = match &self.payload {
            WritePayload::Text { plain, rich } => {
                let mut values = vec![ClipboardContent::Text(plain.clone())];
                match rich {
                    Some((ClipboardSubKind::Html, value)) => {
                        values.push(ClipboardContent::Html(value.clone()))
                    }
                    Some((ClipboardSubKind::Rtf, value)) => {
                        values.push(ClipboardContent::Rtf(value.clone()))
                    }
                    _ => {}
                }
                values
            }
            WritePayload::Files(paths) => {
                self.suppress(guard);
                return ctx.set_files(paths.clone()).map_err(clip_err);
            }
            WritePayload::Image(bytes) => {
                #[cfg(target_os = "macos")]
                {
                    vec![ClipboardContent::Other("public.png".into(), bytes.clone())]
                }
                #[cfg(target_os = "windows")]
                {
                    use clipboard_rs::common::RustImage;
                    let image = clipboard_rs::RustImageData::from_bytes(bytes).map_err(clip_err)?;
                    let encoded = image.to_png().map_err(clip_err)?;
                    let name = format!("{}.png", blake3::hash(encoded.get_bytes()).to_hex());
                    guard.suppress(content_hash(ClipboardKind::Image, &name));
                    vec![ClipboardContent::Image(image)]
                }
            }
        };
        self.suppress(guard);
        ctx.set(values).map_err(clip_err)
    }
}

#[cfg(feature = "system-clipboard")]
fn clip_err(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::Clipboard(error.to_string())
}

/// Legacy and native applications use exactly the same representation selection.
#[cfg(feature = "system-clipboard")]
pub fn write_to_clipboard(
    store: &ImageStore,
    guard: &WritebackGuard,
    item: &ClipboardItem,
    plain: bool,
) -> Result<()> {
    PreparedWrite::new(store, item, plain)?.write(guard)
}
