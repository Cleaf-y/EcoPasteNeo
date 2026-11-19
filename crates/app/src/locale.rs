/// The first native panel ships both supported UI languages; Chinese is default.
#[derive(Clone, Copy, Debug, Default)]
pub enum Locale {
    #[default]
    ZhCn,
    EnUs,
}

impl Locale {
    pub fn login_title(self) -> &'static str {
        self.text("登录时启动", "Launch at login")
    }
    pub fn login_hint(self) -> &'static str {
        self.text("登录后在后台运行。状态以 macOS 为准；不会在下次启动时自动恢复被关闭的登录项。", "Run in the background after login. macOS owns this status; starting Neo will not re-enable a disabled login item.")
    }
    pub fn login_disabled(self) -> &'static str {
        self.text("未注册", "Not registered")
    }
    pub fn login_enabled(self) -> &'static str {
        self.text("已启用", "Enabled")
    }
    pub fn login_approval(self) -> &'static str {
        self.text(
            "需要在系统设置中批准",
            "Approval required in System Settings",
        )
    }
    pub fn login_not_found(self) -> &'static str {
        self.text("系统未找到登录服务", "Login service not found")
    }
    pub fn login_unsupported(self) -> &'static str {
        self.text(
            "登录启动需要 macOS 13 或更新版本",
            "Launch at login requires macOS 13 or later",
        )
    }
    pub fn login_packaged_app(self) -> &'static str {
        self.text(
            "请从已签名的 EcoPasteNeo.app 设置登录启动",
            "Configure login startup from the signed EcoPasteNeo.app",
        )
    }
    pub fn login_unknown(self) -> &'static str {
        self.text("无法识别系统登录项状态", "Unknown system login item status")
    }
    pub fn login_failed(self) -> &'static str {
        self.text("无法更改登录启动", "Could not change login startup")
    }
    pub fn login_loading(self) -> &'static str {
        self.text("正在查询 macOS 登录项…", "Querying macOS login item…")
    }
    pub fn login_refresh(self) -> &'static str {
        self.text("刷新登录项状态", "Refresh login status")
    }
    pub fn login_system_settings(self) -> &'static str {
        self.text("打开系统登录项设置", "Open Login Items in System Settings")
    }
    pub fn login_remove(self) -> &'static str {
        self.text("移除登录项", "Remove login item")
    }

    fn text(self, zh: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::ZhCn => zh,
            Self::EnUs => en,
        }
    }
    pub fn shortcut_title(self) -> &'static str {
        self.text("历史面板全局快捷键", "Global history panel shortcut")
    }
    pub fn shortcut_combination(self) -> &'static str {
        self.text("组合键", "Key combination")
    }
    pub fn shortcut_double(self) -> &'static str {
        self.text("双击修饰键", "Double-tap modifier")
    }
    pub fn shortcut_combination_hint(self) -> &'static str {
        self.text(
            "也可输入组合名称（如 Control+Alt+Super+KeyV）。点击保存后生效。",
            "Or enter a combination (e.g. Control+Alt+Super+KeyV). Save to apply.",
        )
    }
    pub fn shortcut_record(self) -> &'static str {
        self.text("录入组合键", "Record combination")
    }
    pub fn shortcut_recording(self) -> &'static str {
        self.text(
            "请按组合键；Esc 取消。录入期间暂停全局唤出。",
            "Press a combination; Esc cancels. Global activation is paused while recording.",
        )
    }
    pub fn shortcut_permission(self) -> &'static str {
        self.text("双击需要输入监控权限，只观察按键状态，不记录输入。请在系统设置中允许当前 EcoPasteNeo 后重试；原快捷键仍保留。", "Double-tap requires Input Monitoring to observe key state without recording input. Allow this EcoPasteNeo in System Settings and retry; the previous shortcut is retained.")
    }
    pub fn shortcut_double_hint(self) -> &'static str {
        self.text("选择修饰键后点保存。仅双击模式需要输入监控权限；只读监听，不拦截或记录输入。", "Choose a modifier and save. Only double-tap needs Input Monitoring; observation is read-only and does not intercept or record input.")
    }
    pub fn shortcut_listener_failed(self) -> &'static str {
        self.text("双击监听不可用或权限已撤销。可从菜单进入设置，改用组合键。", "Double-tap monitoring is unavailable or permission was revoked. Open Settings from the menu to choose a combination.")
    }
    pub fn shortcut_settings_unavailable(self) -> &'static str {
        self.text(
            "设置不可用，请修复后重试。",
            "Settings unavailable; repair and retry.",
        )
    }
    pub fn shortcut_conflict(self) -> &'static str {
        self.text("无法注册该组合键，可能已被占用。原设置和有效的旧快捷键保留。", "Could not register this combination; it may be in use. Previous settings and any active shortcut are retained.")
    }
    pub fn shortcut_registration_failed(self, reason: &str) -> String {
        format!(
            "{}: {reason}",
            self.text(
                "无法启用组合键，原设置保留",
                "Could not activate the combination; previous settings are retained"
            )
        )
    }
    pub fn shortcut_invalid(self) -> &'static str {
        self.text(
            "请选择包含修饰键和普通按键的有效组合。",
            "Choose a valid combination with a modifier and a standard key.",
        )
    }
    pub fn shortcut_inactive(self) -> &'static str {
        self.text("全局快捷键未生效", "Global shortcut inactive")
    }
    pub fn shortcut_default(self) -> &'static str {
        self.text("恢复默认组合", "Default combination")
    }
    pub fn shortcut_effective(self, description: &str) -> String {
        format!("{}: {description}", self.text("当前生效", "Active"))
    }
    pub fn combination_label(self, key: &str) -> String {
        key.replace("control+", "⌃")
            .replace("Control+", "⌃")
            .replace("alt+", "⌥")
            .replace("Alt+", "⌥")
            .replace("shift+", "⇧")
            .replace("Shift+", "⇧")
            .replace("super+", "⌘")
            .replace("Super+", "⌘")
            .replace("meta+", "⌘")
            .replace("Meta+", "⌘")
            .replace("Key", "")
            .replace("Digit", "")
    }
    pub fn modifier_label(
        self,
        modifier: ecopaste_neo_core::panel_shortcut::TapModifier,
    ) -> &'static str {
        use ecopaste_neo_core::panel_shortcut::TapModifier::*;
        match modifier {
            Control => "Control",
            Option => "Option",
            Shift => "Shift",
            Command => "Command",
        }
    }
    pub fn shortcut_label(
        self,
        choice: &ecopaste_neo_core::panel_shortcut::PanelShortcut,
    ) -> String {
        use ecopaste_neo_core::panel_shortcut::PanelShortcut;
        match choice {
            PanelShortcut::Combination { key } => self.combination_label(key),
            PanelShortcut::DoubleTap { modifier } => format!(
                "{} {}",
                self.text("双击", "Double-tap"),
                self.modifier_label(*modifier)
            ),
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
        self.text("采集规则自动保存，下一次复制即生效；快捷键需点击保存。暂停采集不影响历史查询和粘贴。", "Capture rules save automatically and apply to the next copy; shortcuts require Save. Pausing capture keeps history search and paste available.")
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
    pub fn collect_secrets(self) -> &'static str {
        self.text("收录识别出的敏感内容", "Collect detected sensitive content")
    }
    pub fn redact_secrets(self) -> &'static str {
        self.text(
            "在列表和预览中遮盖敏感内容",
            "Mask sensitive content in history and preview",
        )
    }
    pub fn sensitive_hint(self) -> &'static str {
        self.text("仅使用现有识别规则，不保证识别所有密码。关闭收录只影响之后的复制。遮盖不是加密，不改变主动粘贴的内容。", "Existing detection rules cannot identify every password. Disabling collection affects future copies. Masking is not encryption and does not change content you paste.")
    }
    pub fn saving(self) -> &'static str {
        self.text("正在保存…", "Saving…")
    }
    pub fn excluded_apps(self) -> &'static str {
        self.text("排除应用", "Excluded applications")
    }
    pub fn excluded_apps_hint(self) -> &'static str {
        self.text("选择 .app 应用，按应用标识排除之后的复制。已有历史记录保留。", "Choose a .app to exclude future copies by application identifier. Existing history is kept.")
    }
    pub fn add_application(self) -> &'static str {
        self.text("添加应用…", "Add application…")
    }
    pub fn remove_application(self) -> &'static str {
        self.text("移除", "Remove")
    }
    pub fn clear_excluded_apps(self) -> &'static str {
        self.text("清空排除列表", "Clear exclusions")
    }
    pub fn no_excluded_apps(self) -> &'static str {
        self.text("未排除任何应用", "No applications excluded")
    }
    pub fn invalid_application(self) -> &'static str {
        self.text(
            "无法添加应用。请选择带有有效应用标识的 .app。",
            "Could not add application. Choose a .app with a valid application identifier.",
        )
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
            "↑↓ 选择 · Enter 粘贴 · Esc 隐藏",
            "↑↓ Select · Enter Paste · Esc Hide",
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
