use std::{sync::Arc, time::Duration};

use ecopaste_neo_app::{
    history::{CaptureSession, HistorySource, HistoryState},
    locale::Locale,
    record_preview::fitted_image_size,
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

use super::record_preview::PreviewWindow;
use crate::platform::{self, NativePanel, PasteTarget};
use ecopaste_neo_core::writeback::WritebackGuard;

#[path = "record_management.rs"]
mod record_management;
use record_management::Management;
#[path = "file_drag.rs"]
mod file_drag;
use file_drag::FileDrag;

pub struct HistoryPanel {
    input: Entity<InputState>,
    panel: NativePanel,
    source: HistorySource,
    state: HistoryState,
    list: ListState,
    locale: Locale,
    visible: bool,
    request: Option<Task<()>>,
    capture: Option<CaptureSession>,
    guard: Arc<WritebackGuard>,
    pinned: bool,
    pasting: bool,
    paste_task: Option<Task<()>>,
    action_error: Option<String>,
    target: Option<PasteTarget>,
    interaction: u64,
    refresh_pending: bool,
    preview: Option<PreviewWindow>,
    management: Management,
    file_drag: FileDrag,
}

impl HistoryPanel {
    /// Keep the search input focused while a nonactivating native panel owns keys.
    pub fn new(
        source: HistorySource,
        locale: Locale,
        capture_enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let panel = NativePanel::from_window(window).expect("GPUI must expose a native window");
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(locale.search()));
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.close_preview(false, cx);
                this.state.reset(input.read(cx).value().to_string());
                this.list.reset(0);
                this.load_more(true, cx);
            }
        })
        .detach();
        cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                cx.on_next_frame(window, |this, _, cx| this.check_panel_focus(cx));
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
        let guard = Arc::new(WritebackGuard::new());
        let (capture, action_error) = if capture_enabled {
            match source.start_capture(guard.clone()) {
                Ok(capture) => (Some(capture), None),
                Err(_) => (None, Some(locale.capture_failed().to_owned())),
            }
        } else {
            (None, None)
        };
        let mut view = Self {
            input,
            panel,
            source,
            state,
            list,
            locale,
            visible: false,
            request: None,
            capture,
            guard,
            pinned: false,
            pasting: false,
            paste_task: None,
            action_error,
            target: None,
            interaction: 0,
            refresh_pending: false,
            preview: None,
            management: Management::default(),
            file_drag: FileDrag::default(),
        };
        view.load_more(false, cx);
        view
    }

    /// Re-measure cards and revoke old preview results after committed display changes.
    pub fn settings_saved(&mut self, cx: &mut Context<Self>) {
        self.list.reset(self.state.rows.len());
        if let Some(preview) = &self.preview {
            preview.settings_saved(cx);
        }
        cx.notify();
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_drag.gesture.busy() {
            return;
        }
        self.interaction += 1;
        self.target = PasteTarget::capture();
        self.visible = true;
        if self.refresh_pending {
            self.refresh(cx);
        }
        self.panel.show();
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.file_drag.release();
        self.management.close();
        self.close_preview(false, cx);
        self.interaction += 1;
        self.visible = false;
        self.panel.hide();
        cx.notify();
    }

    /// Coalesce committed updates; hidden panels refresh on their next show.
    pub fn poll_capture(&mut self, cx: &mut Context<Self>) {
        if let Some(capture) = &self.capture {
            while let Ok(success) = capture.events.try_recv() {
                if success {
                    self.refresh_pending = true;
                } else {
                    self.action_error = Some(self.locale.capture_failed().to_owned());
                    cx.notify();
                }
            }
        }
        if self.refresh_pending
            && self.visible
            && !self.pasting
            && !self.state.loading
            && !self.file_drag.gesture.busy()
        {
            self.refresh(cx);
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.close_preview(false, cx);
        self.refresh_pending = false;
        self.state.reset(self.state.keyword.clone());
        self.list.reset(0);
        self.load_more(false, cx);
    }

    /// Resolve the selected ID on the worker, then perform native steps on the UI thread.
    fn paste_selected(&mut self, plain: bool, cx: &mut Context<Self>) {
        if self.pasting || self.management.blocks_list() || self.file_drag.gesture.busy() {
            return;
        }
        let Some(row) = self.state.rows.get(self.state.selected) else {
            return;
        };
        if let Err(error) = platform::check_paste_permission(self.locale) {
            self.action_error = Some(error.to_string());
            cx.notify();
            return;
        }
        let Some(target) = self.target.filter(PasteTarget::is_available) else {
            self.action_error = Some(self.locale.target_missing().into());
            cx.notify();
            return;
        };
        let id = row.item.id.clone();
        self.close_preview(false, cx);
        let prepared = self.source.prepare_write(id, plain);
        let session = self.interaction;
        self.pasting = true;
        self.action_error = None;
        let executor = cx.background_executor().clone();
        self.paste_task = Some(cx.spawn(async move |this, cx| {
            let prepared = prepared.await;
            let ready = this.update(cx, |this, cx| -> anyhow::Result<bool> {
                if !this.visible || session != this.interaction {
                    return Ok(false);
                }
                platform::check_paste_permission(this.locale)?;
                anyhow::ensure!(target.is_available(), "{}", this.locale.target_missing());
                let write = prepared??;
                write.write(&this.guard)?;
                this.panel.yield_to(&target, this.pinned);
                if !this.pinned {
                    this.hide(cx);
                }
                Ok(true)
            });
            let result = match ready {
                Ok(Ok(true)) => {
                    executor.timer(Duration::from_millis(70)).await;
                    this.update(cx, |this, _| -> anyhow::Result<()> {
                        // A newly reopened panel or a changed target cancels the pending key event.
                        anyhow::ensure!(
                            this.pinned || !this.visible,
                            "{}",
                            this.locale.target_missing()
                        );
                        anyhow::ensure!(target.is_foreground(), "{}", this.locale.target_missing());
                        platform::check_paste_permission(this.locale)?;
                        platform::send_paste_keys()
                    })
                    .unwrap_or_else(Err)
                }
                Ok(Ok(false)) => Ok(()),
                Ok(Err(error)) => Err(error),
                Err(error) => Err(error),
            };
            executor.timer(Duration::from_millis(70)).await;
            let _ = this.update(cx, |this, cx| {
                if this.pinned && this.visible && target.is_foreground() {
                    this.panel.reclaim();
                }
                this.pasting = false;
                if let Err(error) = result {
                    this.action_error = Some(format!("{}: {error}", this.locale.paste_failed()));
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Preview focus belongs to the same panel session, not to a new paste target.
    pub(super) fn check_panel_focus(&mut self, cx: &mut Context<Self>) {
        if !self.visible
            || self.pinned
            || self.pasting
            || self.management.tracks_menu()
            || self.file_drag.gesture.busy()
            || self.panel.is_key()
            || self
                .preview
                .as_ref()
                .is_some_and(|preview| preview.is_key(cx))
        {
            return;
        }
        self.hide(cx);
    }

    /// Return focus to the existing main panel without changing the saved target.
    pub(super) fn close_preview(&mut self, restore_focus: bool, cx: &mut Context<Self>) {
        if let Some(preview) = &self.preview {
            preview.close(cx);
        }
        if restore_focus && self.visible {
            self.panel.reclaim();
        }
        cx.notify();
    }

    /// Reuse one nonactivating child, leaving the saved external process untouched.
    fn open_preview(&mut self, cx: &mut Context<Self>) {
        if self.pasting || self.management.blocks_list() || self.file_drag.gesture.busy() {
            return;
        }
        let Some(id) = self
            .state
            .rows
            .get(self.state.selected)
            .map(|row| row.item.id.clone())
        else {
            return;
        };
        if self.preview.is_none() {
            match PreviewWindow::new(
                self.source.clone(),
                self.locale,
                cx.entity().downgrade(),
                &self.panel,
                cx,
            ) {
                Ok(preview) => self.preview = Some(preview),
                Err(_) => {
                    self.action_error = Some(self.locale.preview_failed().to_owned());
                    cx.notify();
                    return;
                }
            }
        }
        if let Some(preview) = &self.preview {
            preview.show(id, &self.panel, cx);
        }
        cx.notify();
    }

    fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.is_open(cx))
        {
            self.close_preview(true, cx);
        } else {
            self.open_preview(cx);
        }
    }

    fn update_open_preview(&mut self, cx: &mut Context<Self>) {
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.is_open(cx))
        {
            self.open_preview(cx);
        }
    }

    /// Navigation from the child window follows the same selected record as the list.
    pub(super) fn select_preview_neighbor(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.state.rows.is_empty() {
            return;
        }
        self.state.selected = if down {
            (self.state.selected + 1).min(self.state.rows.len() - 1)
        } else {
            self.state.selected.saturating_sub(1)
        };
        self.list.scroll_to_reveal_item(self.state.selected);
        self.update_open_preview(cx);
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
        if self.file_drag.gesture.busy() {
            if event.keystroke.key == "escape" {
                self.file_drag.release();
            }
            cx.stop_propagation();
            return;
        }
        if self.management_key_down(event, window, cx) {
            return;
        }
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
                self.update_open_preview(cx);
                if self.state.selected.saturating_add(8) >= self.state.rows.len()
                    && self.state.error.is_none()
                {
                    self.load_more(false, cx);
                }
                cx.stop_propagation();
                cx.notify();
            }
            "enter" => {
                self.paste_selected(event.keystroke.modifiers.shift, cx);
                cx.stop_propagation();
            }
            "escape" => {
                if self
                    .preview
                    .as_ref()
                    .is_some_and(|preview| preview.is_open(cx))
                {
                    self.close_preview(true, cx);
                } else {
                    self.hide(cx);
                }
                cx.stop_propagation();
            }
            "p" if event.keystroke.modifiers.secondary(cx) => {
                self.toggle_preview(cx);
                cx.stop_propagation();
            }
            "q" if event.keystroke.modifiers.secondary(cx) => {
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
        let mut labels = vec![kind];
        if row.item.is_pinned {
            labels.push(locale.pinned());
        }
        if row.item.is_favorite {
            labels.push(locale.favorite());
        }
        let header = div()
            .flex()
            .justify_between()
            .gap_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(labels.join(" · "))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(row.item.created_at.format("%m-%d %H:%M").to_string())
                    .child(
                        div()
                            .id(("preview-row", index))
                            .cursor_pointer()
                            .text_color(theme.primary)
                            .child(locale.preview())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.state.selected = index;
                                this.open_preview(cx);
                            })),
                    ),
            );
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
                    div().whitespace_normal().child(
                        if row.item.is_sensitive
                            && self
                                .source
                                .settings()
                                .snapshot()
                                .map(|s| s.sensitive.redact_secrets)
                                .unwrap_or(true)
                        {
                            "••••••••".to_owned()
                        } else {
                            row.item.summary.clone().unwrap_or_default()
                        },
                    ),
                );
            }
            ClipboardKind::Image => {
                let width = row.item.width.unwrap_or(1).max(1) as f32;
                let height = row.item.height.unwrap_or(1).max(1) as f32;
                let preview_height = (height / width * 280.0).clamp(72.0, 180.0);
                let (image_width, image_height) =
                    fitted_image_size(width, height, 280.0, preview_height);
                card = card
                    .child(match &row.thumbnail {
                        Some(path) => div()
                            .w_full()
                            .h(px(preview_height))
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                img(path.clone())
                                    .w(px(image_width))
                                    .max_w_full()
                                    .h(px(image_height))
                                    .max_h(px(preview_height))
                                    .flex_none()
                                    .min_w_0()
                                    .min_h_0()
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
        let menu_id = row.item.id.clone();
        let drag_id = menu_id.clone();
        div()
            .w_full()
            .px_3()
            .child(
                card.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, event, _, cx| {
                        this.press_file_row(&drag_id, event, cx);
                    }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |this, event, _, cx| {
                        cx.stop_propagation();
                        this.popup_history_menu(&menu_id, event, cx);
                    }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.management.blocks_list() || !this.file_drag.gesture.allows_click() {
                        return;
                    }
                    this.state.selected = index;
                    this.input.update(cx, |input, cx| input.focus(window, cx));
                    this.paste_selected(false, cx);
                    cx.notify();
                })),
            )
            .into_any_element()
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let motion = cx.entity().downgrade();
        let release = motion.clone();
        let drag_events = gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, _, cx| {
                    if phase.bubble() {
                        let _ = motion.update(cx, |this, cx| this.move_file_drag(event, cx));
                    }
                });
                window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, _, cx| {
                    if phase.capture() && event.button == gpui::MouseButton::Left {
                        let _ = release.update(cx, |this, _| this.file_drag.release());
                    }
                });
            },
        )
        .absolute()
        .size_full();
        let locale = self.locale;
        let dialog = self.render_record_dialog(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let root = div()
            .id("history-panel")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .capture_key_down(cx.listener(Self::key_down))
            .child(drag_events);
        if let Some(dialog) = dialog {
            return root
                .child(dialog)
                .when(self.action_error.is_some(), |view| {
                    view.child(
                        div()
                            .px_4()
                            .text_color(cx.theme().danger)
                            .child(self.action_error.clone().unwrap_or_default()),
                    )
                });
        }
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
        let controls = div()
            .flex()
            .gap_3()
            .child(
                div()
                    .id("preview-selected")
                    .cursor_pointer()
                    .child(locale.preview())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_preview(cx))),
            )
            .child(
                div()
                    .id("pin-panel")
                    .cursor_pointer()
                    .child(if self.pinned {
                        locale.unpin_panel()
                    } else {
                        locale.pin_panel()
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pinned = !this.pinned;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("paste-plain")
                    .cursor_pointer()
                    .child(locale.paste_plain())
                    .on_click(cx.listener(|this, _, _, cx| this.paste_selected(true, cx))),
            )
            .child(
                div()
                    .id("paste-selected")
                    .cursor_pointer()
                    .child(locale.paste())
                    .on_click(cx.listener(|this, _, _, cx| this.paste_selected(false, cx))),
            );
        root.child(
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
                .child(Input::new(&self.input).cleanable(true))
                .child(controls),
        )
        .child(body)
        .when(self.state.error.is_some(), |view| {
            view.child(div().px_4().py_2().text_color(theme.danger).child(format!(
                "{}: {}",
                locale.failed(),
                self.state.error.as_deref().unwrap_or_default()
            )))
        })
        .when(self.action_error.is_some(), |view| {
            view.child(
                div()
                    .px_4()
                    .py_2()
                    .text_color(theme.danger)
                    .child(self.action_error.clone().unwrap_or_default()),
            )
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
