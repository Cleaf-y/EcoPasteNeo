/// The first native panel ships both supported UI languages; Chinese is default.
#[derive(Clone, Copy, Debug, Default)]
pub enum Locale {
    #[default]
    ZhCn,
    EnUs,
}

impl Locale {
    fn text(self, zh: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::ZhCn => zh,
            Self::EnUs => en,
        }
    }
    pub fn show(self) -> &'static str {
        self.text("显示历史记录", "Show history")
    }
    pub fn quit(self) -> &'static str {
        self.text("退出", "Quit")
    }
    pub fn search(self) -> &'static str {
        self.text("搜索历史记录", "Search history")
    }
    pub fn paste(self) -> &'static str {
        self.text("粘贴", "Paste")
    }
    pub fn paste_plain(self) -> &'static str {
        self.text("纯文本粘贴", "Paste plain")
    }
    pub fn pin_panel(self) -> &'static str {
        self.text("固定窗口", "Keep open")
    }
    pub fn unpin_panel(self) -> &'static str {
        self.text("取消固定", "Unpin window")
    }
    pub fn permission_required(self) -> &'static str {
        self.text("请在系统设置 → 隐私与安全性 → 辅助功能中授权此应用，然后重试", "Allow this application in System Settings → Privacy & Security → Accessibility, then retry")
    }
    pub fn target_missing(self) -> &'static str {
        self.text(
            "请先将光标放到目标应用，再唤出面板",
            "Place the cursor in the target application, then show the panel",
        )
    }
    pub fn capture_failed(self) -> &'static str {
        self.text(
            "剪贴板捕获或保存失败",
            "Could not capture or save clipboard content",
        )
    }
    pub fn paste_failed(self) -> &'static str {
        self.text("粘贴失败", "Paste failed")
    }
    pub fn preview(self) -> &'static str {
        self.text("预览 ⌘P", "Preview ⌘P")
    }
    pub fn preview_title(self) -> &'static str {
        self.text("记录预览", "Record preview")
    }
    pub fn close_preview(self) -> &'static str {
        self.text("关闭预览", "Close preview")
    }
    pub fn preview_failed(self) -> &'static str {
        self.text("记录不存在或无法预览", "Record is missing or unavailable")
    }
    pub fn sensitive_hidden(self) -> &'static str {
        self.text("敏感内容已隐藏", "Sensitive content hidden")
    }
    pub fn original_image(self) -> &'static str {
        self.text("原图", "Original image")
    }
    pub fn empty_content(self) -> &'static str {
        self.text("无可显示的内容", "No content to display")
    }
    pub fn hint(self) -> &'static str {
        self.text(
            "⌃⌥⌘V 唤出 · ↑↓ 选择 · Enter 粘贴 · Esc 隐藏",
            "⌃⌥⌘V Show · ↑↓ Select · Enter Paste · Esc Hide",
        )
    }
    pub fn loading(self) -> &'static str {
        self.text("正在加载…", "Loading…")
    }
    pub fn more(self) -> &'static str {
        self.text("加载更多", "Load more")
    }
    pub fn retry(self) -> &'static str {
        self.text("重试", "Retry")
    }
    pub fn empty(self) -> &'static str {
        self.text("暂无历史记录", "No history yet")
    }
    pub fn no_results(self) -> &'static str {
        self.text("没有匹配的记录", "No matching records")
    }
    pub fn failed(self) -> &'static str {
        self.text("无法读取历史记录", "Could not load history")
    }
    pub fn image(self) -> &'static str {
        self.text("图片", "Image")
    }
    pub fn image_missing(self) -> &'static str {
        self.text("图片预览不可用", "Image preview unavailable")
    }
    pub fn text_kind(self) -> &'static str {
        self.text("文本", "Text")
    }
    pub fn files(self) -> &'static str {
        self.text("文件", "Files")
    }
    pub fn folder(self) -> &'static str {
        self.text("文件夹", "Folder")
    }
    pub fn missing(self) -> &'static str {
        self.text("已不存在", "Missing")
    }
    pub fn pinned(self) -> &'static str {
        self.text("置顶", "Pinned")
    }
    pub fn count(self, loaded: usize, total: i64) -> String {
        match self {
            Self::ZhCn => format!("已加载 {loaded} / {total} 条"),
            Self::EnUs => format!("{loaded} of {total} loaded"),
        }
    }
    pub fn files_count(self, count: usize) -> String {
        match self {
            Self::ZhCn => format!("{count} 个文件"),
            Self::EnUs => format!("{count} files"),
        }
    }
}
