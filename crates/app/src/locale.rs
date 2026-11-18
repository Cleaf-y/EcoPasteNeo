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
    pub fn settings_title(self) -> &'static str {
        self.text("设置", "Settings")
    }
    pub fn settings_hint(self) -> &'static str {
        self.text("更改自动保存，下一次复制即生效。暂停采集不会影响已有历史记录的查询和粘贴。", "Changes save automatically and apply to the next copy. Pausing capture keeps history search and paste available.")
    }
    pub fn capture_enabled(self) -> &'static str {
        self.text("采集剪贴板", "Capture clipboard")
    }
    pub fn capture_type(
        self,
        kind: ecopaste_neo_core::clipboard::settings::CaptureKind,
    ) -> &'static str {
        use ecopaste_neo_core::clipboard::settings::CaptureKind;
        match kind {
            CaptureKind::Text => self.text("纯文本", "Plain text"),
            CaptureKind::Html => "HTML",
            CaptureKind::Rtf => "RTF",
            CaptureKind::Image => self.image(),
            CaptureKind::Files => self.files(),
        }
    }
    pub fn saving(self) -> &'static str {
        self.text("正在保存…", "Saving…")
    }
    pub fn settings_error(self, unavailable: bool, error: &str) -> String {
        let label = if unavailable {
            self.text(
                "无法读取设置，已暂停采集。请修复设置文件后重试",
                "Settings unavailable; capture paused. Repair the settings file and retry",
            )
        } else {
            self.text(
                "保存失败，仍使用上次保存的设置",
                "Could not save; previous settings remain active",
            )
        };
        format!("{label}: {error}")
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
    pub fn favorite(self) -> &'static str {
        self.text("已收藏", "Favorite")
    }
    pub fn favorite_action(self, current: bool) -> &'static str {
        if current {
            self.text("取消收藏", "Unfavorite")
        } else {
            self.text("收藏", "Favorite")
        }
    }
    pub fn pin_action(self, current: bool) -> &'static str {
        if current {
            self.text("取消置顶", "Unpin record")
        } else {
            self.text("置顶", "Pin record")
        }
    }
    pub fn edit_note(self) -> &'static str {
        self.text("编辑备注", "Edit note")
    }
    pub fn clear_note(self) -> &'static str {
        self.text("清空备注", "Clear note")
    }
    pub fn delete_record(self) -> &'static str {
        self.text("删除记录", "Delete record")
    }
    pub fn delete_confirmation(self) -> &'static str {
        self.text("删除这条记录？此操作无法撤销。文件记录所指向的原文件会保留。", "Delete this record? This cannot be undone. Original files referenced by file records are kept.")
    }
    pub fn cancel(self) -> &'static str {
        self.text("取消", "Cancel")
    }
    pub fn save(self) -> &'static str {
        self.text("保存", "Save")
    }
    pub fn management_failed(self) -> &'static str {
        self.text(
            "无法修改记录，请重试",
            "Could not update record. Please retry",
        )
    }
    pub fn image_cleanup_failed(self) -> &'static str {
        self.text(
            "记录已删除，图片资源清理未完成",
            "Record deleted; image resource cleanup is incomplete",
        )
    }
    pub fn drag_error(self, reason: Option<&ecopaste_neo_core::preview::DragFilesError>) -> String {
        use ecopaste_neo_core::preview::DragFilesError;
        match reason {
            Some(DragFilesError::FilesMissing) => self.text(
                "源文件均已不存在，无法拖出",
                "All source files are missing; cannot drag",
            ),
            Some(DragFilesError::ImageMissing) => self.text(
                "原图不存在或不可用，无法拖出",
                "Original image is missing or unavailable; cannot drag",
            ),
            Some(DragFilesError::Sensitive) => self.sensitive_hidden(),
            Some(DragFilesError::RecordMissing) => {
                self.text("记录已不存在，无法拖出", "Record is missing; cannot drag")
            }
            _ => self.text(
                "无法拖出文件，请重新按住卡片并拖动",
                "Could not drag files. Press and drag the card again",
            ),
        }
        .into()
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
