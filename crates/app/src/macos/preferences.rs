use ecopaste_neo_app::{history::HistorySource, locale::Locale};
use ecopaste_neo_core::{
    clipboard::settings::CaptureKind,
    settings::{Settings, SettingsChange},
};
use gpui::{
    App, AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Task, Window, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, div, px, size,
};
use gpui_component::{ActiveTheme, Disableable, Root, StyledExt, button::Button, switch::Switch};

/// Retains the view and pending write across normal window close/recreation.
pub struct SettingsWindow {
    handle: Option<WindowHandle<Root>>,
    view: Entity<Preferences>,
    locale: Locale,
}

impl SettingsWindow {
    pub fn new(source: HistorySource, locale: Locale, cx: &mut App) -> Self {
        let view = cx.new(|_| Preferences::new(source, locale));
        Self {
            handle: None,
            view,
            locale,
        }
    }

    /// Reuse a live normal window; a closed window is recreated with the same view.
    pub fn show(&mut self, cx: &mut App) -> anyhow::Result<()> {
        self.view.update(cx, |view, cx| {
            if view.pending.is_none() {
                view.sync();
                cx.notify();
            }
        });
        cx.activate(true);
        if let Some(handle) = self.handle
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return Ok(());
        }
        let view = self.view.clone();
        let bounds = Bounds::centered(None, size(px(520.0), px(540.0)), cx);
        self.handle = Some(cx.open_window(
            WindowOptions {
                kind: WindowKind::Normal,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                is_minimizable: true,
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title(self.locale.settings_title());
                cx.new(|cx| Root::new(view, window, cx))
            },
        )?);
        Ok(())
    }
}

struct Preferences {
    source: HistorySource,
    locale: Locale,
    saved: Option<Settings>,
    error: Option<String>,
    pending: Option<Task<()>>,
}

impl Preferences {
    fn new(source: HistorySource, locale: Locale) -> Self {
        let mut view = Self {
            source,
            locale,
            saved: None,
            error: None,
            pending: None,
        };
        view.sync();
        view
    }

    /// Reopening reads committed memory, without re-reading or repairing the file.
    fn sync(&mut self) {
        match self.source.settings().snapshot() {
            Ok(settings) => {
                self.saved = Some(settings);
                self.error = None;
            }
            Err(error) => {
                self.saved = None;
                self.error = Some(error.to_string());
            }
        }
    }

    /// Controls remain at their saved values until this worker finishes successfully.
    fn change(&mut self, change: SettingsChange, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let pending = self.source.update_settings(change);
        self.await_update(pending, cx);
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let pending = self.source.reload_settings();
        self.await_update(pending, cx);
    }

    /// Keep the operation alive across close and update this retained view on completion.
    fn await_update(
        &mut self,
        pending: tokio::task::JoinHandle<anyhow::Result<Settings>>,
        cx: &mut Context<Self>,
    ) {
        self.pending = Some(cx.spawn(async move |this, cx| {
            let result = match pending.await {
                Ok(result) => result,
                Err(error) => Err(error.into()),
            };
            let _ = this.update(cx, |this, cx| {
                this.pending = None;
                match result {
                    Ok(settings) => {
                        this.saved = Some(settings);
                        this.error = None;
                    }
                    Err(error) => {
                        this.saved = this.source.settings().snapshot().ok();
                        this.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for Preferences {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.pending.is_some() || self.saved.is_none();
        let settings = self.saved.clone().unwrap_or_default();
        let mut content = div()
            .id("settings")
            .tab_group()
            .size_full()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .p_6()
            .v_flex()
            .gap_4()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child(self.locale.settings_title()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.locale.settings_hint()),
            )
            .child(
                Switch::new("capture-enabled")
                    .label(self.locale.capture_enabled())
                    .checked(self.saved.is_some() && settings.capture_enabled)
                    .disabled(disabled)
                    .on_change(cx.listener(|this, checked, _, cx| {
                        this.change(SettingsChange::CaptureEnabled(*checked), cx)
                    })),
            );
        for (index, kind) in [
            CaptureKind::Text,
            CaptureKind::Html,
            CaptureKind::Rtf,
            CaptureKind::Image,
            CaptureKind::Files,
        ]
        .into_iter()
        .enumerate()
        {
            content = content.child(
                Switch::new(("capture-type", index))
                    .label(self.locale.capture_type(kind))
                    .checked(self.saved.is_some() && settings.capture.is_enabled(kind))
                    .disabled(disabled)
                    .on_change(cx.listener(move |this, checked, _, cx| {
                        this.change(SettingsChange::CaptureType(kind, *checked), cx)
                    })),
            );
        }
        if let Some(error) = &self.error {
            content = content
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(self.locale.settings_error(self.saved.is_none(), error)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.source.paths().settings_path().display().to_string()),
                )
                .child(
                    Button::new("retry-settings")
                        .label(self.locale.retry())
                        .disabled(self.pending.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                );
        }
        if self.pending.is_some() {
            content = content.child(div().text_sm().child(self.locale.saving()));
        }
        content
    }
}
