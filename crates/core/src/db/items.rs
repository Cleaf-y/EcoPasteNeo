//! History reads shared by the Neo application and the existing Tauri adapter.

use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use super::models::{
    ClipboardGroupFilter, ClipboardItem, ClipboardItemPage, ClipboardItemQuery, ClipboardItemSort,
    ClipboardKind,
};
use super::Result;

const SELECT_ITEM: &str = "SELECT id, kind, sub_kind, group_id, source_app_id, content, \
     content_hash, search_text, summary, file_types, size, width, height, use_count, is_favorite, is_pinned, \
     is_sensitive, platform, note, created_at, updated_at FROM clipboard_items";

/// 列表/单条刷新场景的精简 SELECT：text 类型条目的 `content` 与 `search_text` 一律置空，
/// 由前端用 `summary` 渲染。HTML/RTF/长纯文本可能很大（用户复制整段文档），
/// 整段过 IPC + 进 DOM 是这条链路最昂贵的一环；image/files 的 content 是
/// 文件名 / 路径列表，保留原值。预览/写回走 [`find_item_by_id`] 拿完整 content。
///
/// LEFT JOIN `clipboard_apps` 顺带把来源应用名 / 图标文件名带回，前端直接渲染，
/// 不再额外发 list_clipboard_apps + get_clipboard_app_icon_path 请求。
const LIST_SELECT_ITEM: &str = "SELECT clipboard_items.id, clipboard_items.kind, \
     clipboard_items.sub_kind, clipboard_items.group_id, clipboard_items.source_app_id, \
     CASE WHEN clipboard_items.kind = 'text' THEN '' ELSE clipboard_items.content END AS content, \
     clipboard_items.content_hash, \
     CASE WHEN clipboard_items.kind = 'text' THEN NULL ELSE clipboard_items.search_text END AS search_text, \
     clipboard_items.summary, clipboard_items.file_types, clipboard_items.size, \
     clipboard_items.width, clipboard_items.height, clipboard_items.use_count, \
     clipboard_items.is_favorite, clipboard_items.is_pinned, \
     clipboard_items.is_sensitive, \
     clipboard_items.platform, clipboard_items.note, \
     clipboard_items.created_at, clipboard_items.updated_at, \
     clipboard_apps.name AS source_app_name, \
     clipboard_apps.icon_file AS source_app_icon_file \
     FROM clipboard_items \
     LEFT JOIN clipboard_apps ON clipboard_apps.id = clipboard_items.source_app_id";

/// 按 `content_hash` 查最近一条同内容记录（命中 `idx_clipboard_items_content_hash` 索引）。
pub async fn find_item_by_content_hash(
    pool: &SqlitePool,
    hash: &str,
) -> Result<Option<ClipboardItem>> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(SELECT_ITEM);
    qb.push(" WHERE content_hash = ")
        .push_bind(hash.to_owned())
        .push(" ORDER BY created_at DESC LIMIT 1");

    let item = qb
        .build_query_as::<ClipboardItem>()
        .fetch_optional(pool)
        .await?;
    Ok(item)
}

/// Returns one summary page and the total matching the same filters.
/// Short tokens use LIKE because the trigram index cannot match fewer than three
/// characters. The caller supplies its current pool after any directory switch.
pub async fn query_items_page(
    pool: &SqlitePool,
    q: &ClipboardItemQuery,
) -> Result<ClipboardItemPage> {
    let keyword = KeywordFilter::from_keyword(q.keyword.as_deref());
    let list = fetch_items(pool, q, keyword.clone()).await?;
    let total = fetch_items_count(pool, q, keyword).await?;
    let has_more = q.offset.saturating_add(list.len() as i64) < total;

    Ok(ClipboardItemPage {
        list,
        total,
        has_more,
    })
}

/// 按 `id` 查找单条记录，不存在时返回 `None`。
pub async fn find_item_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ClipboardItem>> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(SELECT_ITEM);
    qb.push(" WHERE id = ").push_bind(id.to_owned());

    let item = qb
        .build_query_as::<ClipboardItem>()
        .fetch_optional(pool)
        .await?;
    Ok(item)
}

/// 按 `id` 查找单条记录的「列表视图」副本——与 [`fetch_items`] 走同款 [`LIST_SELECT_ITEM`] 裁剪：
/// text 类型条目的 `content` / `search_text` 一律置空，由前端用 `summary` 渲染。
/// 供前端响应 `clipboard://updated` 事件时按 id 拉取使用，避免事件驱动刷新整页 refetch
/// 时回传整段 HTML/RTF。需要完整 `content` 的写回 / 预览路径请走 [`find_item_by_id`]。
pub async fn find_item_for_list_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<ClipboardItem>> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(LIST_SELECT_ITEM);
    qb.push(" WHERE clipboard_items.id = ")
        .push_bind(id.to_owned());

    let item = qb
        .build_query_as::<ClipboardItem>()
        .fetch_optional(pool)
        .await?;
    Ok(item)
}

/// 关键词过滤分流：≥3 字符走 FTS5（trigram），1–2 字符走 LIKE 兜底，空 / 仅空白不过滤。
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeywordFilter {
    None,
    /// `clipboard_items_fts MATCH ?` 的表达式，已含前缀通配。
    Fts(String),
    /// 已转义 `% _ \` 的关键词；下游统一拼成 `%<kw>%` 多列模糊匹配。
    Like(String),
}

impl KeywordFilter {
    /// 按字符数（非字节）判定走 FTS 还是 LIKE。CJK 一个字符也算 1，
    /// 与 trigram 的 3 字符门槛保持一致。
    ///
    /// 注意：门槛按**每个空白分词**判定，而非整串字符数。trigram 索引对 <3 字符的
    /// token 永远 0 命中，而 FTS5 表达式里多 token 之间默认是 AND —— 只要有一个短词，
    /// 整条表达式就被拖成 0 结果（例如 `a b` 会生成 `"a"* "b"*`，二者皆 <3 字符）。
    /// 因此仅当所有分词均 ≥3 字符时才走 FTS，否则降级到 LIKE 兜底。
    fn from_keyword(keyword: Option<&str>) -> Self {
        let Some(trimmed) = keyword.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::None;
        };

        let fts_viable = trimmed
            .split_whitespace()
            .all(|token| token.chars().count() >= 3);

        if fts_viable {
            if let Some(expr) = build_fts_expr(trimmed) {
                return Self::Fts(expr);
            }
        }

        Self::Like(escape_like(trimmed))
    }
}

/// 把用户关键词拆成 FTS5 前缀匹配表达式（如 `foo bar` -> `"foo"* "bar"*`）。
/// 双引号包裹 + 转义，避免关键词中的 FTS5 语法字符被当作运算符。空白关键词返回 `None`。
fn build_fts_expr(keyword: &str) -> Option<String> {
    let expr = keyword
        .split_whitespace()
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ");

    (!expr.is_empty()).then_some(expr)
}

/// 转义 LIKE 的特殊字符（`\ % _`），配合 SQL 端 `ESCAPE '\\'`。
fn escape_like(keyword: &str) -> String {
    let mut out = String::with_capacity(keyword.len());
    for ch in keyword.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 拼装查询：过滤（含可选关键词匹配） + 排序（置顶恒前置） + 分页。
/// 所有 bind 均传入拥有所有权/Copy 的值，避免 `QueryBuilder` 借用 `q` 引发的生命周期问题。
async fn fetch_items(
    pool: &SqlitePool,
    q: &ClipboardItemQuery,
    keyword: KeywordFilter,
) -> Result<Vec<ClipboardItem>> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(LIST_SELECT_ITEM);
    qb.push(" WHERE 1 = 1");
    push_filter_clauses(&mut qb, q, &keyword);

    qb.push(" ORDER BY clipboard_items.is_pinned DESC, ");
    match q.sort {
        ClipboardItemSort::CreatedAt => {
            qb.push("clipboard_items.created_at DESC");
        }
        ClipboardItemSort::UpdatedAt => {
            qb.push("clipboard_items.updated_at DESC, clipboard_items.created_at DESC");
        }
        ClipboardItemSort::UseCount => {
            qb.push("clipboard_items.use_count DESC, clipboard_items.created_at DESC");
        }
    }

    // A unique final key keeps equal timestamps/counts stable across pages.
    qb.push(", clipboard_items.id DESC");
    qb.push(" LIMIT ").push_bind(q.limit);
    qb.push(" OFFSET ").push_bind(q.offset);

    let items = qb.build_query_as::<ClipboardItem>().fetch_all(pool).await?;
    Ok(items)
}

/// 统计满足同样过滤条件的总条数（不参与排序 / 分页），与 [`fetch_items`] 共用 [`push_filter_clauses`]。
async fn fetch_items_count(
    pool: &SqlitePool,
    q: &ClipboardItemQuery,
    keyword: KeywordFilter,
) -> Result<i64> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT COUNT(*) FROM clipboard_items");
    qb.push(" WHERE 1 = 1");
    push_filter_clauses(&mut qb, q, &keyword);

    let row: (i64,) = qb.build_query_as().fetch_one(pool).await?;
    Ok(row.0)
}

/// 把当前查询的过滤条件追加到 `qb`（不含 ORDER BY / LIMIT / OFFSET），供列表查询与计数共用。
fn push_filter_clauses(
    qb: &mut QueryBuilder<Sqlite>,
    q: &ClipboardItemQuery,
    keyword: &KeywordFilter,
) {
    match keyword {
        KeywordFilter::None => {}
        KeywordFilter::Fts(expr) => {
            qb.push(
                " AND clipboard_items.rowid IN (SELECT rowid FROM clipboard_items_fts WHERE clipboard_items_fts MATCH ",
            )
            .push_bind(expr.clone())
            .push(")");
        }
        KeywordFilter::Like(kw) => {
            // FTS 索引覆盖 search_text / note 两列，LIKE 兜底也跟齐，
            // 让 1–2 字符短词的命中范围与长词一致。
            let pattern = format!("%{kw}%");
            qb.push(" AND (clipboard_items.search_text LIKE ")
                .push_bind(pattern.clone())
                .push(" ESCAPE '\\' OR clipboard_items.note LIKE ")
                .push_bind(pattern)
                .push(" ESCAPE '\\')");
        }
    }
    // group（UI Tab）覆盖显式 kind / favorite；为 None 时回退到显式字段（单测使用）。
    let (effective_kind, effective_favorite) = match q.group {
        Some(ClipboardGroupFilter::All) => (None, None),
        Some(ClipboardGroupFilter::Text) => (Some(ClipboardKind::Text), None),
        Some(ClipboardGroupFilter::Image) => (Some(ClipboardKind::Image), None),
        Some(ClipboardGroupFilter::Files) => (Some(ClipboardKind::Files), None),
        Some(ClipboardGroupFilter::Favorite) => (None, Some(true)),
        None => (q.kind, q.favorite),
    };
    if let Some(kind) = effective_kind {
        qb.push(" AND clipboard_items.kind = ").push_bind(kind);
    }
    if let Some(group_id) = &q.group_id {
        qb.push(" AND clipboard_items.group_id = ")
            .push_bind(group_id.clone());
    }
    if let Some(favorite) = effective_favorite {
        qb.push(" AND clipboard_items.is_favorite = ")
            .push_bind(favorite);
    }
    if let Some(pinned) = q.pinned {
        qb.push(" AND clipboard_items.is_pinned = ")
            .push_bind(pinned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_filter_classifies_by_char_length() {
        assert_eq!(KeywordFilter::from_keyword(None), KeywordFilter::None);
        assert_eq!(
            KeywordFilter::from_keyword(Some("   ")),
            KeywordFilter::None
        );

        // 1–2 字符走 LIKE（含 CJK）。
        assert_eq!(
            KeywordFilter::from_keyword(Some("a")),
            KeywordFilter::Like("a".to_owned())
        );
        assert_eq!(
            KeywordFilter::from_keyword(Some("中文")),
            KeywordFilter::Like("中文".to_owned())
        );

        // ≥3 字符走 FTS。
        assert_eq!(
            KeywordFilter::from_keyword(Some("foo")),
            KeywordFilter::Fts("\"foo\"*".to_owned())
        );
        assert_eq!(
            KeywordFilter::from_keyword(Some("foo bar")),
            KeywordFilter::Fts("\"foo\"* \"bar\"*".to_owned())
        );
        assert_eq!(
            KeywordFilter::from_keyword(Some("a\"b")),
            KeywordFilter::Fts("\"a\"\"b\"*".to_owned())
        );
    }

    #[test]
    fn keyword_filter_drops_multi_token_short_words_to_like() {
        // 整串 ≥3 字符，但每个空白分词都 <3 字符：trigram 对 <3 字符 token 永远 0 命中，
        // 且 FTS5 多 token 默认 AND，整条表达式会被任一短词拖成 0 结果 → 必须降级 LIKE。
        assert_eq!(
            KeywordFilter::from_keyword(Some("a b")),
            KeywordFilter::Like("a b".to_owned())
        );
        assert_eq!(
            KeywordFilter::from_keyword(Some("ab cd")),
            KeywordFilter::Like("ab cd".to_owned())
        );
        // 混合长度：只要有一个分词 <3 字符，整条 FTS 表达式就会被拖成 0 命中 → LIKE。
        assert_eq!(
            KeywordFilter::from_keyword(Some("foo b")),
            KeywordFilter::Like("foo b".to_owned())
        );
        // 所有分词均 ≥3 字符仍走 FTS（回归保障，语义不变）。
        assert_eq!(
            KeywordFilter::from_keyword(Some("foo bar baz")),
            KeywordFilter::Fts("\"foo\"* \"bar\"* \"baz\"*".to_owned())
        );
    }

    #[test]
    fn escape_like_escapes_special_chars() {
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("c:\\path"), "c:\\\\path");
    }
}
