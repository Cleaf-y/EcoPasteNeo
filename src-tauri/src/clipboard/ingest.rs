use super::{ClipboardPayload, ImageStore};
use crate::core::Result;
use crate::db::models::ClipboardItem;
use crate::settings::{Capture, Sensitive};

#[cfg(test)]
pub fn build_item(store: &ImageStore, payload: &ClipboardPayload) -> Result<Option<ClipboardItem>> {
    build_item_with_settings(
        store,
        payload,
        &Capture::default(),
        &Sensitive::default(),
        false,
    )
}

/// 将旧应用设置交给共享入库转换，不访问系统剪贴板。
pub fn build_item_with_settings(
    store: &ImageStore,
    payload: &ClipboardPayload,
    capture: &Capture,
    sensitive: &Sensitive,
    plain_only: bool,
) -> Result<Option<ClipboardItem>> {
    Ok(ecopaste_neo_core::clipboard::build_item_with_settings(
        store, payload, capture, sensitive, plain_only,
    )?)
}
