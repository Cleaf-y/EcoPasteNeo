use serde::{Deserialize, Serialize};

/// 剪贴板内容类型采集开关。关闭后监听与手动读取都不入库对应类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Capture {
    pub text: bool,
    pub html: bool,
    pub rtf: bool,
    pub image: bool,
    pub files: bool,
    /// 文本最大收录大小，单位 MB。`0` = 不限制。
    pub max_text_mb: u32,
    /// 图片最大收录大小，单位 MB。`0` = 不限制。
    pub max_image_mb: u32,
    /// 剪贴板同时提供多种表示时的采集优先级。
    pub order: Vec<CaptureKind>,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            text: true,
            html: true,
            rtf: true,
            image: true,
            files: true,
            max_text_mb: 4,
            max_image_mb: 100,
            order: CaptureKind::default_order(),
        }
    }
}

impl Capture {
    /// 返回文本最大收录字节数；`None` 表示不限制。
    pub fn max_text_bytes(&self) -> Option<u64> {
        mb_to_bytes(self.max_text_mb)
    }

    /// 返回图片最大收录字节数；`None` 表示不限制。
    pub fn max_image_bytes(&self) -> Option<u64> {
        mb_to_bytes(self.max_image_mb)
    }

    /// 返回去重且补齐缺失项后的采集顺序，避免配置文件里手改出重复项后影响读取。
    pub fn ordered_kinds(&self) -> Vec<CaptureKind> {
        let mut order = Vec::new();
        for kind in self
            .order
            .iter()
            .copied()
            .chain(CaptureKind::default_order())
        {
            if !order.contains(&kind) {
                order.push(kind);
            }
        }

        order
    }

    /// 判断某个采集类型当前是否开启。
    pub fn is_enabled(&self, kind: CaptureKind) -> bool {
        match kind {
            CaptureKind::Text => self.text,
            CaptureKind::Html => self.html,
            CaptureKind::Rtf => self.rtf,
            CaptureKind::Image => self.image,
            CaptureKind::Files => self.files,
        }
    }
}

/// 把用户设置的 MB 值转换为字节阈值；`0` 表示不限。
fn mb_to_bytes(mb: u32) -> Option<u64> {
    if mb == 0 {
        return None;
    }

    Some(u64::from(mb) * 1024 * 1024)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CaptureKind {
    Files,
    Image,
    Html,
    Rtf,
    Text,
}

impl CaptureKind {
    /// 默认顺序保持历史硬编码语义：文件 > 图片 > HTML > RTF > 纯文本。
    pub fn default_order() -> Vec<Self> {
        vec![Self::Files, Self::Image, Self::Html, Self::Rtf, Self::Text]
    }
}

/// 隐私保护设置。命中规则的内容可分别控制是否收录、是否脱敏展示。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Sensitive {
    /// 命中高置信密钥 / Token 时是否保存到历史记录。
    pub collect_secrets: bool,
    /// 已保存的敏感内容是否在列表与预览中脱敏展示。
    pub redact_secrets: bool,
}

impl Default for Sensitive {
    fn default() -> Self {
        Self {
            collect_secrets: true,
            redact_secrets: true,
        }
    }
}

/// 应用过滤规则。
/// `excluded_app_ids` 命中复制来源时，对应剪贴板内容不入库。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Filters {
    pub excluded_app_ids: Vec<String>,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            excluded_app_ids: default_excluded_app_ids(),
        }
    }
}

fn default_excluded_app_ids() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        // 系统级密码 / 密钥工具：用户从这里复制的几乎都是敏感凭据，默认不入库。
        // - com.apple.keychainaccess：钥匙串访问
        // - com.apple.Passwords：macOS 15 起的「密码」App
        vec![
            "com.apple.keychainaccess".to_owned(),
            "com.apple.Passwords".to_owned(),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        // Windows 无系统内置的密码管理 App（凭据管理器是 Control Panel 子项，不会作为复制来源）。
        // 第三方密码管理器（1Password / Bitwarden / KeePass 等）因人而异，留给用户在 UI 勾选。
        Vec::new()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn macos_defaults_keep_sensitive_system_apps_excluded() {
        let ids = default_excluded_app_ids();

        assert!(ids.contains(&"com.apple.keychainaccess".to_owned()));
        assert!(ids.contains(&"com.apple.Passwords".to_owned()));
    }
}
