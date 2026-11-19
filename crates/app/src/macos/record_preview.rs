use ecopaste_neo_app::{
    history::HistorySource,
    locale::Locale,
    record_preview::{PreviewState, fitted_image_size, text_rows},
};
use ecopaste_neo_core::preview::PreviewContent;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ListAlignment, ListState, ObjectFit, ParentElement, Render, Styled, Task,
    WeakEntity, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, img, list,
    prelude::*, px, size,
};
use gpui_component::{ActiveTheme, Root};

use super::{native::Panel, panel::HistoryPanel};

/// One reusable child window. Closing clears its data and invalidates every request.
pub struct PreviewWindow {
    _window: WindowHandle<Root>,
    view: Entity<RecordPreview>,
    panel: Panel,
}

impl PreviewWindow {
    /// Create a hidden child once; opening records later must not activate the app.
    pub fn new(
        source: HistorySource,
        locale: Locale,
        owner: WeakEntity<HistoryPanel>,
        parent: &Panel,
        cx: &mut App,
    ) -> anyhow::Result<Self> {
        let mut content = None;
        let display_id = parent.display_id();
        let bounds = Bounds::centered(display_id, size(px(480.0), px(600.0)), cx);
        let window = cx.open_window(
            WindowOptions {
                kind: WindowKind::PopUp,
                titlebar: None,
                display_id,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                show: false,
                focus: false,
                is_resizable: false,
                is_minimizable: false,
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title(locale.preview_title());
                let close_owner = owner.clone();
                window.on_window_should_close(cx, move |_, cx| {
                    let _ = close_owner.update(cx, |owner, cx| owner.close_preview(true, cx));
                    false
                });
                let view = cx.new(|cx| RecordPreview::new(source, locale, owner, window, cx));
                content = Some(view.clone());
                cx.new(|cx| Root::new(view, window, cx))
            },
        )?;
        let view = content.expect("Preview view must exist after window creation");
        let panel = view.read(cx).panel;
        Ok(Self {
            _window: window,
            view,
            panel,
        })
    }

    /// Reload the open record without moving focus, showing a window, or recapturing a target.
    pub fn settings_saved(&self, cx: &mut App) {
        self.view.update(cx, |view, cx| {
            if view.state.open
                && let Some(id) = view.state.id.clone()
            {
                view.load(id, cx);
            }
        });
    }

    pub fn is_open(&self, cx: &App) -> bool {
        self.view.read(cx).state.open
    }
    pub fn is_key(&self, cx: &App) -> bool {
        self.is_open(cx) && self.panel.is_key()
    }

    /// Updating a record never updates the history panel's captured paste target.
    pub fn show(&self, id: String, parent: &Panel, cx: &mut App) {
        self.view.update(cx, |view, cx| view.load(id, cx));
        self.panel.show_preview(parent);
    }

    /// Revoke the request, clear display data, and detach the native child.
    pub fn close(&self, cx: &mut App) {
        self.view.update(cx, |view, cx| {
            view.state.close();
            view.request = None;
            view.rows.clear();
            view.list.reset(0);
            cx.notify();
        });
        self.panel.hide_preview();
    }
}

struct RecordPreview {
    owner: WeakEntity<HistoryPanel>,
    source: HistorySource,
    locale: Locale,
    panel: Panel,
    focus: FocusHandle,
    state: PreviewState,
    rows: Vec<String>,
    list: ListState,
    request: Option<Task<()>>,
}

impl RecordPreview {
    /// Keep internal keyboard focus while observing the shared panel session.
    fn new(
        source: HistorySource,
        locale: Locale,
        owner: WeakEntity<HistoryPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_window_activation(window, |this, window, _| {
            if !window.is_window_active() {
                let owner = this.owner.clone();
                window.on_next_frame(move |_, cx| {
                    let _ = owner.update(cx, |owner, cx| owner.check_panel_focus(cx));
                });
            }
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            owner,
            source,
            locale,
            panel: Panel::from_window(window).expect("Preview must expose AppKit window"),
            focus,
            state: PreviewState::default(),
            rows: vec![],
            list: ListState::new(0, ListAlignment::Top, px(200.0)),
            request: None,
        }
    }

    /// Clear the previous record immediately; late completions may never show a window.
    fn load(&mut self, id: String, cx: &mut Context<Self>) {
        let request = self.state.begin(id.clone());
        self.rows.clear();
        self.list.reset(0);
        let redact = self
            .source
            .settings()
            .snapshot()
            .map(|s| s.sensitive.redact_secrets)
            .unwrap_or(true);
        let pending = self.source.preview(id);
        self.request = Some(cx.spawn(async move |this, cx| {
            let result = match pending.await {
                Ok(result) => result,
                Err(error) => Err(error.into()),
            };
            let _ = this.update(cx, |this, cx| {
                let current = this
                    .source
                    .settings()
                    .snapshot()
                    .map(|s| s.sensitive.redact_secrets)
                    .unwrap_or(true);
                if current != redact {
                    return;
                }
                if !this.state.complete(request, result) {
                    return;
                }
                let count = match &this.state.content {
                    Some(PreviewContent::Text(text)) => {
                        this.rows = text_rows(text);
                        this.rows.len()
                    }
                    Some(PreviewContent::Files(files)) => files.len(),
                    _ => 0,
                };
                this.list.reset(count);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Defer parent callbacks until this child entity is no longer borrowed.
    fn close(&self, cx: &mut Context<Self>) {
        let owner = self.owner.clone();
        cx.defer(move |cx| {
            let _ = owner.update(cx, |owner, cx| owner.close_preview(true, cx));
        });
    }

    /// Keyboard events are forwarded without showing the main panel or recapturing its target.
    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => {
                self.close(cx);
                cx.stop_propagation();
            }
            "q" if event.keystroke.modifiers.platform => {
                cx.quit();
                cx.stop_propagation();
            }
            "p" | "w" if event.keystroke.modifiers.platform => {
                self.close(cx);
                cx.stop_propagation();
            }
            "up" | "down" => {
                let down = event.keystroke.key == "down";
                let owner = self.owner.clone();
                cx.defer(move |cx| {
                    let _ = owner.update(cx, |owner, cx| owner.select_preview_neighbor(down, cx));
                });
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn render_item(&self, index: usize, cx: &App) -> gpui::AnyElement {
        match &self.state.content {
            Some(PreviewContent::Text(_)) => div()
                .w_full()
                .px_4()
                .min_h(px(22.0))
                .text_xs()
                .font_family("monospace")
                .whitespace_normal()
                .child(self.rows.get(index).cloned().unwrap_or_default())
                .into_any_element(),
            Some(PreviewContent::Files(files)) => {
                let Some(file) = files.get(index) else {
                    return div().into_any_element();
                };
                let label = if !file.exists {
                    self.locale.missing()
                } else if file.is_directory {
                    self.locale.folder()
                } else {
                    self.locale.files()
                };
                div()
                    .w_full()
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(file.name.clone())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .whitespace_normal()
                            .child(file.path.to_string_lossy().into_owned()),
                    )
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }
}

impl Render for RecordPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let weak = cx.entity().downgrade();
        let available_height = (window.viewport_size().height - px(110.0)).max(px(1.0));
        let available_width = (window.viewport_size().width - px(24.0)).max(px(1.0));
        let mut body = div().flex_1().min_h_0().w_full().overflow_hidden();
        match &self.state.content {
            Some(PreviewContent::Text(text)) if text.is_empty() => {
                body = body.child(self.locale.empty_content())
            }
            Some(PreviewContent::Text(_) | PreviewContent::Files(_)) => {
                body = body.child(
                    list(self.list.clone(), move |index, _, cx| {
                        weak.update(cx, |this, cx| this.render_item(index, cx))
                            .unwrap_or_else(|_| div().into_any_element())
                    })
                    .size_full(),
                );
            }
            Some(PreviewContent::Image(Some(image))) => {
                let (image_width, image_height) = fitted_image_size(
                    image.width as f32,
                    image.height as f32,
                    f32::from(available_width),
                    f32::from(available_height),
                );
                body = body
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .child(
                        div()
                            .w_full()
                            .h(available_height)
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                img(image.path.clone())
                                    .w(px(image_width))
                                    .h(px(image_height))
                                    .flex_none()
                                    .min_w_0()
                                    .min_h_0()
                                    .object_fit(ObjectFit::Contain),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "{} × {} · {}",
                                image.width,
                                image.height,
                                self.locale.original_image()
                            )),
                    );
            }
            _ => {
                let label = match &self.state.content {
                    Some(PreviewContent::Hidden) => self.locale.sensitive_hidden(),
                    Some(PreviewContent::Image(None)) => self.locale.image_missing(),
                    _ if self.state.loading => self.locale.loading(),
                    _ => self.locale.preview_failed(),
                };
                body = body
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.muted_foreground)
                    .child(label);
            }
        }
        div()
            .id("record-preview")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .text_sm()
            .bg(theme.background)
            .text_color(theme.foreground)
            .capture_key_down(cx.listener(Self::key_down))
            .child(
                div()
                    .p_3()
                    .flex()
                    .justify_between()
                    .gap_2()
                    .child(self.locale.preview_title())
                    .child(
                        div()
                            .id("close-preview")
                            .cursor_pointer()
                            .text_color(theme.primary)
                            .child(self.locale.close_preview())
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            )
            .child(body)
    }
}
