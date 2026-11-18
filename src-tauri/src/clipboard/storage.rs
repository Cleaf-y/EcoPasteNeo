use crate::core::Result;
use ecopaste_neo_core::clipboard::ImageStore as CoreImageStore;
use std::ops::Deref;
#[cfg(test)]
use std::path::PathBuf;
use tauri::AppHandle;

/// 保留旧应用的数据目录解析和 Tauri State 类型；图片实现由 core 提供。
#[derive(Clone)]
pub struct ImageStore(CoreImageStore);

impl ImageStore {
    pub fn new(app: &AppHandle) -> Result<Self> {
        Ok(Self(CoreImageStore::from_resources_dir(
            &crate::core::paths::resources_dir(app)?,
        )))
    }

    #[cfg(test)]
    pub(crate) fn for_test(images_root: PathBuf) -> Self {
        Self(CoreImageStore::from_resources_dir(
            images_root.parent().expect("image root has a parent"),
        ))
    }

    /// 切换目录后更新共享图片存储的位置。
    pub fn rebase(&self, app: &AppHandle) -> Result<()> {
        self.0
            .rebase_resources_dir(&crate::core::paths::resources_dir(app)?);
        Ok(())
    }
}

impl Deref for ImageStore {
    type Target = CoreImageStore;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
