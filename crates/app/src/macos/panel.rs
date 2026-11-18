use std::time::Duration;

use ecopaste_neo_app::{
    history::{HistorySource, HistoryState},
    locale::Locale,
};
use ecopaste_neo_core::{db::models::ClipboardKind, identity::PRODUCT_NAME};
use gpui::{
    AppContext, Context, Entity, EntityInputHandler, InteractiveElement, IntoElement, KeyDownEvent,
    ListAlignment, ListState, ObjectFit, ParentElement, Render, Styled, Task, Window, div, img,
    list, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, StyledExt,
    input::{Input, InputEvent, InputState},
};

use super::native::Panel;

pub struct HistoryPanel {
    input: Entity<InputState>,
    panel: Panel,
    source: HistorySource,
    state: HistoryState,
    list: ListState,
    locale: Locale,
    visible: bool,
    request: Option<Task<()>>,
}

impl HistoryPanel {
    /// Keep the search input focused while a nonactivating native panel owns keys.
    pub fn new(
        source: HistorySource,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let panel = Panel::from_window(window).expect("GPUI must expose an AppKit window");
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(locale.search()));
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.state.reset(input.read(cx).value().to_string());
                this.list.reset(0);
                this.load_more(true, cx);
            }
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.visible {
                this.hide(cx);
            }
        })
        .detach();
        let list = ListState::new(0, ListAlignment::Top, px(300.0));
        let weak = cx.entity().downgrade();
        list.set_scroll_handler(move |event, _, cx| {
            let end = event.visible_range.end;
            let _ = weak.update(cx, |this, cx| {
                if end.saturating_add(8) >= this.state.rows.len() && this.state.error.is_none() {
                    this.load_more(false, cx);
                }
            });
        });
        let mut state = HistoryState::default();
        state.reset(String::new());
        let mut view = Self {
            input,
            panel,
            source,
            state,
            list,
            locale,
            visible: false,
            request: None,
        };
        view.load_more(false, cx);
        view
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = true;
        self.panel.show();
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.visible = false;
        self.panel.hide();
        cx.notify();
    }

    /// Debounce keyword changes, then accept only the matching search generation.
    fn load_more(&mut self, debounce: bool, cx: &mut Context<Self>) {
        let Some(request) = self.state.begin() else {
            return;
        };
        let source = self.source.clone();
        let keyword = self.state.keyword.clone();
        let executor = cx.background_executor().clone();
        self.request = Some(cx.spawn(async move |this, cx| {
            if debounce {
                executor.timer(Duration::from_millis(120)).await;
            }
            let result = match source.query(keyword, HistoryState::offset(request)).await {
                Ok(result) => result,
                Err(error) => Err(error.into()),
            };
            let _ = this.update(cx, |this, cx| {
                let previous = this.state.rows.len();
                if this.state.complete(request, result) {
                    let added = this.state.rows.len() - previous;
                    this.list.splice(previous..previous, added);
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    /// Leave navigation and Escape to the IME while marked text is present.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        match event.keystroke.key.as_str() {
            "up" | "down" | "pageup" | "pagedown" | "home" | "end"
                if !self.state.rows.is_empty() =>
            {
                let last = self.state.rows.len() - 1;
                self.state.selected = match event.keystroke.key.as_str() {
                    "up" => self.state.selected.saturating_sub(1),
                    "down" => (self.state.selected + 1).min(last),
                    "pageup" => self.state.selected.saturating_sub(5),
                    "pagedown" => (self.state.selected + 5).min(last),
                    "home" => 0,
                    _ => last,
                };
                self.list.scroll_to_reveal_item(self.state.selected);
                if self.state.selected.saturating_add(8) >= self.state.rows.len()
                    && self.state.error.is_none()
                {
                    self.load_more(false, cx);
                }
                cx.stop_propagation();
                cx.notify();
            }
            "escape" => {
                self.hide(cx);
                cx.stop_propagation();
            }
            "q" if event.keystroke.modifiers.platform => {
                cx.quit();
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    /// Render only visible cards; every card keeps a stable height after loading.
    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(row) = self.state.rows.get(index) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let locale = self.locale;
        let kind = match row.item.kind {
            ClipboardKind::Text => locale.text_kind(),
            ClipboardKind::Image => locale.image(),
            ClipboardKind::Files => locale.files(),
        };
        let header = div()
            .flex()
            .justify_between()
            .gap_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(format!(
                "{}{}",
                kind,
                if row.item.is_pinned {
                    format!(" · {}", locale.pinned())
                } else {
                    String::new()
                }
            ))
            .child(row.item.created_at.format("%m-%d %H:%M").to_string());
        let mut card = div()
            .id(("history-row", index))
            .w_full()
            .mb_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(if index == self.state.selected {
                theme.list_active_border
            } else {
                theme.border
            })
            .bg(if index == self.state.selected {
                theme.list_active
            } else {
                theme.background
            })
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .child(header);
        match row.item.kind {
            ClipboardKind::Text => {
                card = card.child(
                    div()
                        .whitespace_normal()
                        .child(row.item.summary.clone().unwrap_or_default()),
                );
            }
            ClipboardKind::Image => {
                let width = row.item.width.unwrap_or(1).max(1) as f32;
                let height = row.item.height.unwrap_or(1).max(1) as f32;
                let preview_height = (height / width * 280.0).clamp(72.0, 180.0);
                card = card
                    .child(match &row.thumbnail {
                        Some(path) => div().h(px(preview_height)).child(
                            img(path.clone())
                                .w_full()
                                .h_full()
                                .object_fit(ObjectFit::Contain),
                        ),
                        None => div()
                            .h(px(preview_height))
                            .flex()
                            .items_center()
                            .text_color(theme.muted_foreground)
                            .child(locale.image_missing()),
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "{} × {} · {}",
                                row.item.width.unwrap_or(0),
                                row.item.height.unwrap_or(0),
                                bytes(row.item.size.unwrap_or(0).max(0) as u64)
                            )),
                    );
            }
            ClipboardKind::Files => {
                for file in &row.files {
                    let detail = if !file.exists {
                        locale.missing().to_owned()
                    } else if file.is_directory {
                        locale.folder().to_owned()
                    } else {
                        file.size.map(bytes).unwrap_or_default()
                    };
                    card = card.child(
                        div().flex().flex_col().child(file.name.clone()).child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(format!("{} · {}", file.path.display(), detail)),
                        ),
                    );
                }
                card = card.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(locale.files_count(row.file_count)),
                );
            }
        }
        if let Some(note) = row.item.note.as_ref().filter(|note| !note.is_empty()) {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(note.clone()),
            );
        }
        div()
            .w_full()
            .px_3()
            .child(card.on_click(cx.listener(move |this, _, window, cx| {
                this.state.selected = index;
                this.input.update(cx, |input, cx| input.focus(window, cx));
                cx.notify();
            })))
            .into_any_element()
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.locale;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let weak = cx.entity().downgrade();
        let mut body = div().flex_1().min_h_0().w_full();
        if self.state.rows.is_empty() {
            let label = if self.state.loading {
                locale.loading()
            } else if self.state.error.is_some() {
                locale.failed()
            } else if self.state.keyword.trim().is_empty() {
                locale.empty()
            } else {
                locale.no_results()
            };
            body = body
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child(label);
        } else {
            body = body.child(
                list(self.list.clone(), move |index, _, cx| {
                    weak.update(cx, |this, cx| this.render_row(index, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .size_full(),
            );
        }
        let mut footer = div()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(theme.border)
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .text_xs()
            .text_color(muted)
            .child(locale.count(self.state.rows.len(), self.state.total));
        if self.state.loading {
            footer = footer.child(locale.loading());
        } else if self.state.error.is_some() || self.state.has_more {
            footer = footer.child(
                div()
                    .id("load-more")
                    .cursor_pointer()
                    .text_color(theme.primary)
                    .child(if self.state.error.is_some() {
                        locale.retry()
                    } else {
                        locale.more()
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.load_more(false, cx))),
            );
        }
        div()
            .id("history-panel")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .capture_key_down(cx.listener(Self::key_down))
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .pb_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(div().text_lg().font_semibold().child(PRODUCT_NAME))
                            .child(div().text_xs().text_color(muted).child(locale.hint())),
                    )
                    .child(Input::new(&self.input).cleanable(true)),
            )
            .child(body)
            .when(self.state.error.is_some(), |view| {
                view.child(div().px_4().py_2().text_color(theme.danger).child(format!(
                    "{}: {}",
                    locale.failed(),
                    self.state.error.as_deref().unwrap_or_default()
                )))
            })
            .child(footer)
    }
}

fn bytes(size: u64) -> String {
    if size < 1024 {
        format!("{size} B")
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    }
}
