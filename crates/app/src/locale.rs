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
    pub fn hint(self) -> &'static str {
        self.text(
            "⌃⌥⌘V 唤出 · ↑↓ 选择 · Esc 隐藏",
            "⌃⌥⌘V Show · ↑↓ Select · Esc Hide",
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
