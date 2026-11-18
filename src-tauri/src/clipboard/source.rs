use crate::db::models::Platform;

#[derive(Debug, Clone)]
pub struct FrontmostApp {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub icon_png: Option<Vec<u8>>,
}

/// 同步记录来源信息，图标提取留在旧应用适配器。
pub fn detect_frontmost() -> Option<FrontmostApp> {
    let source = ecopaste_neo_core::clipboard::source::detect_frontmost()?;
    let icon_png = source
        .path
        .as_deref()
        .and_then(|path| super::icon::icon_png(path, None));
    Some(FrontmostApp {
        id: source.id,
        name: source.name,
        platform: source.platform,
        icon_png,
    })
}
