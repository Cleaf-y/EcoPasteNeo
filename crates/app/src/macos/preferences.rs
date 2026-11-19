use super::system::{SystemIntegration, parse_combination};
use ecopaste_neo_app::{history::HistorySource, locale::Locale};
use ecopaste_neo_core::panel_shortcut::{PanelShortcut, TapModifier};
use ecopaste_neo_core::{
    clipboard::settings::CaptureKind,
    settings::{Settings, SettingsChange},
};
use gpui::{
    App, AppContext, Bounds, Context, Entity, EventEmitter, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, PathPromptOptions, Render,
    StatefulInteractiveElement, Styled, Task, Window, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, Disableable, Root, StyledExt,
    button::Button,
    input::{Input, InputState},
    switch::Switch,
};
use objc2_foundation::{NSBundle, NSString};
use std::{cell::RefCell, rc::Rc};

/// Retains the view and pending write across normal window close/recreation.
pub struct SettingsWindow {
    handle: Option<WindowHandle<Root>>,
    view: Entity<Preferences>,
    locale: Locale,
    runtime_status: String,
}

struct SettingsSaved;

impl EventEmitter<SettingsSaved> for Preferences {}

impl SettingsWindow {
    /// Notify consumers only after a save or explicit reload succeeds.
    pub fn on_saved(&self, mut callback: impl FnMut(&mut App) + 'static, cx: &mut App) {
        cx.subscribe(&self.view, move |_, _: &SettingsSaved, cx| callback(cx))
            .detach();
    }

    pub fn new(
        source: HistorySource,
        locale: Locale,
        integration: Rc<RefCell<SystemIntegration>>,
        cx: &mut App,
    ) -> Self {
        let view = cx.new(|cx| Preferences::new(source, locale, integration, cx));
        Self {
            runtime_status: String::new(),
            handle: None,
            view,
            locale,
        }
    }

    /// Refresh a visible Settings status after permission loss without a busy repaint loop.
    pub fn poll_shortcut(&mut self, cx: &mut App) {
        let integration = self.view.read(cx).integration.clone();
        let recorded = integration.borrow_mut().take_recorded();
        if let Some(key) = recorded {
            self.view.update(cx, |view, cx| {
                if view.recording {
                    view.draft = PanelShortcut::Combination { key };
                    view.recording = false;
                    view.reset_input = true;
                    view.integration.borrow_mut().edit(false);
                    cx.notify();
                }
            });
        }
        let status = format!(
            "{}{:?}",
            integration.borrow().description(),
            integration.borrow().error()
        );
        if status != self.runtime_status {
            self.runtime_status = status;
            self.view.update(cx, |_, cx| cx.notify());
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
                let weak = view.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.cancel_shortcut();
                        cx.notify();
                    });
                    true
                });
                cx.new(|cx| Root::new(view, window, cx))
            },
        )?);
        Ok(())
    }
}

struct Preferences {
    integration: Rc<RefCell<SystemIntegration>>,
    draft: PanelShortcut,
    recording: bool,
    shortcut_pending: bool,
    shortcut_reload: bool,
    shortcut_error: Option<String>,
    combo_input: Option<Entity<InputState>>,
    reset_input: bool,
    focus: FocusHandle,
    source: HistorySource,
    locale: Locale,
    saved: Option<Settings>,
    error: Option<String>,
    pending: Option<Task<()>>,
    picker: Option<Task<()>>,
    selection_error: bool,
}

impl Preferences {
    fn new(
        source: HistorySource,
        locale: Locale,
        integration: Rc<RefCell<SystemIntegration>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            integration,
            draft: PanelShortcut::default(),
            recording: false,
            shortcut_pending: false,
            shortcut_reload: false,
            shortcut_error: None,
            combo_input: None,
            reset_input: true,
            focus: cx.focus_handle(),
            source,
            locale,
            saved: None,
            error: None,
            pending: None,
            picker: None,
            selection_error: false,
        };
        view.sync();
        view
    }

    /// Reopening reads committed memory, without re-reading or repairing the file.
    fn sync(&mut self) {
        match self.source.settings().snapshot() {
            Ok(settings) => {
                if !self.recording {
                    self.draft = settings.panel_shortcut.clone();
                    self.reset_input = true;
                }
                self.saved = Some(settings);
                self.error = None;
            }
            Err(error) => {
                self.saved = None;
                self.error = Some(error.to_string());
            }
        }
    }

    /// Closing or cancelling discards the draft without touching the active registration.
    fn cancel_shortcut(&mut self) {
        if self.shortcut_pending {
            return;
        }
        self.recording = false;
        self.integration.borrow_mut().edit(false);
        self.draft = self
            .saved
            .as_ref()
            .map(|s| s.panel_shortcut.clone())
            .unwrap_or_default();
        self.reset_input = true;
        self.shortcut_error = None;
    }

    fn record_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        cx.stop_propagation();
        if event.keystroke.key == "escape" {
            self.cancel_shortcut();
            cx.notify();
            return;
        }
        if event.is_held {
            return;
        }
        let stroke = &event.keystroke;
        let mut parts = Vec::new();
        if stroke.modifiers.control {
            parts.push("Control".to_string());
        }
        if stroke.modifiers.alt {
            parts.push("Alt".to_string());
        }
        if stroke.modifiers.shift {
            parts.push("Shift".to_string());
        }
        if stroke.modifiers.platform {
            parts.push("Super".to_string());
        }
        parts.push(stroke.key.clone());
        match parse_combination(&parts.join("+"), self.locale) {
            Ok(key) => {
                let key = key.to_string();
                self.draft = PanelShortcut::Combination { key: key.clone() };
                if let Some(input) = &self.combo_input {
                    input.update(cx, |input, cx| input.set_value(key, window, cx));
                }
                self.recording = false;
                self.integration.borrow_mut().edit(false);
                self.shortcut_error = None;
            }
            Err(error) => self.shortcut_error = Some(error.to_string()),
        }
        cx.notify();
    }

    /// Validate/reserve natively first, persist on the worker, then transfer ownership.
    fn save_shortcut(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.saved.is_none() {
            return;
        }
        let mut choice = self.draft.clone();
        if let PanelShortcut::Combination { key } = &mut choice {
            let value = self
                .combo_input
                .as_ref()
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or(key.clone());
            match parse_combination(&value, self.locale) {
                Ok(parsed) => *key = parsed.to_string(),
                Err(error) => {
                    self.shortcut_error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        }
        let result = self.integration.borrow_mut().prepare(&choice, true);
        if let Err(error) = result {
            self.shortcut_error = Some(error.to_string());
            cx.notify();
            return;
        }
        self.recording = false;
        self.shortcut_pending = true;
        self.shortcut_error = None;
        self.change(SettingsChange::PanelShortcut(choice), cx);
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
        self.shortcut_reload = true;
        let pending = self.source.reload_settings();
        self.await_update(pending, cx);
    }

    /// Resolve the chosen bundle itself, never the app currently in front of Settings.
    fn choose_excluded_app(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.picker.is_some() || self.saved.is_none() {
            return;
        }
        self.selection_error = false;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.locale.add_application().into()),
        });
        let background = cx.background_executor().clone();
        self.picker = Some(cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => background
                    .spawn(async move {
                        let path = paths.first()?;
                        if path.extension()? != "app" {
                            return None;
                        }
                        let bundle = NSBundle::bundleWithPath(&NSString::from_str(path.to_str()?))?;
                        let id = bundle.bundleIdentifier()?.to_string();
                        (!id.is_empty() && !id.chars().any(char::is_whitespace)).then_some(id)
                    })
                    .await
                    .ok_or(())
                    .map(Some),
                Ok(Ok(None)) => Ok(None),
                _ => Err(()),
            };
            let _ = this.update(cx, |this, cx| {
                this.picker = None;
                match result {
                    Ok(Some(id)) => this.change(SettingsChange::AddExcludedApp(id), cx),
                    Ok(None) => {}
                    Err(()) => this.selection_error = true,
                }
                cx.notify();
            });
        }));
        cx.notify();
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
                let shortcut_write = this.shortcut_pending;
                if this.shortcut_pending {
                    this.integration.borrow_mut().finish(result.is_ok());
                    this.shortcut_pending = false;
                    this.recording = false;
                }
                let reload = std::mem::take(&mut this.shortcut_reload);
                match result {
                    Ok(settings) => {
                        if reload {
                            let result = this
                                .integration
                                .borrow_mut()
                                .prepare(&settings.panel_shortcut, false);
                            match result {
                                Ok(()) => this.integration.borrow_mut().finish(true),
                                Err(error) => this.shortcut_error = Some(error.to_string()),
                            }
                        }
                        this.draft = settings.panel_shortcut.clone();
                        this.reset_input = true;
                        this.saved = Some(settings);
                        this.error = None;
                        cx.emit(SettingsSaved);
                    }
                    Err(error) => {
                        if shortcut_write {
                            this.shortcut_error =
                                Some(this.locale.settings_error(false, &error.to_string()));
                        }
                        this.saved = this.source.settings().snapshot().ok();
                        this.draft = this
                            .saved
                            .as_ref()
                            .map(|s| s.panel_shortcut.clone())
                            .unwrap_or_default();
                        this.reset_input = true;
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.pending.is_some() || self.picker.is_some() || self.saved.is_none();
        let settings = self.saved.clone().unwrap_or_default();
        if self.combo_input.is_none() {
            self.combo_input = Some(cx.new(|cx| InputState::new(window, cx)));
        }
        if self.reset_input {
            let key = match &self.draft {
                PanelShortcut::Combination { key } => key.clone(),
                _ => match PanelShortcut::default() {
                    PanelShortcut::Combination { key } => key,
                    _ => unreachable!(),
                },
            };
            self.combo_input
                .as_ref()
                .unwrap()
                .update(cx, |input, cx| input.set_value(key, window, cx));
            self.reset_input = false;
        }
        let mut shortcuts = div()
            .v_flex()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child(self.locale.shortcut_title()),
            )
            .child(
                div().text_sm().child(
                    self.locale
                        .shortcut_effective(&self.integration.borrow().description()),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        Button::new("shortcut-combination")
                            .label(self.locale.shortcut_combination())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.recording = false;
                                this.integration.borrow_mut().edit(false);
                                this.draft = this
                                    .saved
                                    .as_ref()
                                    .map(|s| s.panel_shortcut.clone())
                                    .filter(|s| matches!(s, PanelShortcut::Combination { .. }))
                                    .unwrap_or_default();
                                this.reset_input = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("shortcut-double")
                            .label(self.locale.shortcut_double())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.recording = false;
                                this.integration.borrow_mut().edit(false);
                                this.draft = PanelShortcut::DoubleTap {
                                    modifier: TapModifier::Control,
                                };
                                cx.notify();
                            })),
                    ),
            );
        match &self.draft {
            PanelShortcut::Combination { .. } => {
                shortcuts = shortcuts
                    .child(
                        Input::new(self.combo_input.as_ref().unwrap())
                            .disabled(disabled || self.recording),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(self.locale.shortcut_combination_hint()),
                    )
                    .child(
                        Button::new("record-shortcut")
                            .label(self.locale.shortcut_record())
                            .disabled(disabled || self.recording)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.recording = true;
                                this.integration.borrow_mut().edit(true);
                                window.focus(&this.focus, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("default-shortcut")
                            .label(self.locale.shortcut_default())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.recording = false;
                                this.integration.borrow_mut().edit(false);
                                this.draft = PanelShortcut::default();
                                this.reset_input = true;
                                cx.notify();
                            })),
                    );
            }
            PanelShortcut::DoubleTap { modifier } => {
                shortcuts =
                    shortcuts.child(div().text_sm().child(self.locale.shortcut_double_hint()));
                let mut buttons = div().flex().gap_2();
                for (index, value) in [
                    TapModifier::Control,
                    TapModifier::Option,
                    TapModifier::Shift,
                    TapModifier::Command,
                ]
                .into_iter()
                .enumerate()
                {
                    buttons = buttons.child(
                        Button::new(("tap-modifier", index))
                            .label(format!(
                                "{}{}",
                                if *modifier == value { "✓ " } else { "" },
                                self.locale.modifier_label(value)
                            ))
                            .disabled(disabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft = PanelShortcut::DoubleTap { modifier: value };
                                cx.notify();
                            })),
                    );
                }
                shortcuts = shortcuts.child(buttons);
            }
        }
        if self.recording {
            shortcuts = shortcuts.child(div().text_sm().child(self.locale.shortcut_recording()));
        }
        if let Some(error) = self
            .shortcut_error
            .as_deref()
            .or(self.integration.borrow().error())
        {
            shortcuts = shortcuts.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.to_string()),
            );
        }
        shortcuts = shortcuts.child(
            div()
                .flex()
                .gap_3()
                .child(
                    Button::new("save-shortcut")
                        .label(self.locale.save())
                        .disabled(disabled || self.recording)
                        .on_click(cx.listener(|this, _, _, cx| this.save_shortcut(cx))),
                )
                .child(
                    Button::new("cancel-shortcut")
                        .label(self.locale.cancel())
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_shortcut();
                            cx.notify();
                        })),
                ),
        );
        let mut content = div()
            .id("settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::record_key))
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
            .child(shortcuts)
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
        content = content
            .child(
                Switch::new("collect-secrets")
                    .label(self.locale.collect_secrets())
                    .checked(self.saved.is_some() && settings.sensitive.collect_secrets)
                    .disabled(disabled)
                    .on_change(cx.listener(|this, checked, _, cx| {
                        this.change(SettingsChange::CollectSecrets(*checked), cx)
                    })),
            )
            .child(
                Switch::new("redact-secrets")
                    .label(self.locale.redact_secrets())
                    .checked(self.saved.is_some() && settings.sensitive.redact_secrets)
                    .disabled(disabled)
                    .on_change(cx.listener(|this, checked, _, cx| {
                        this.change(SettingsChange::RedactSecrets(*checked), cx)
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.locale.sensitive_hint()),
            );
        content = content
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child(self.locale.excluded_apps()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.locale.excluded_apps_hint()),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        Button::new("add-excluded-app")
                            .label(self.locale.add_application())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.choose_excluded_app(cx))),
                    )
                    .child(
                        Button::new("clear-excluded-apps")
                            .label(self.locale.clear_excluded_apps())
                            .disabled(disabled || settings.filters.excluded_app_ids.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.change(SettingsChange::ClearExcludedApps, cx)
                            })),
                    ),
            );
        if let Some(settings) = &self.saved {
            if settings.filters.excluded_app_ids.is_empty() {
                content = content.child(div().text_sm().child(self.locale.no_excluded_apps()));
            }
            for (index, id) in settings.filters.excluded_app_ids.iter().enumerate() {
                let id = id.clone();
                content = content.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .truncate()
                                .child(id.clone()),
                        )
                        .child(
                            Button::new(("remove-excluded-app", index))
                                .label(self.locale.remove_application())
                                .disabled(disabled)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.change(SettingsChange::RemoveExcludedApp(id.clone()), cx)
                                })),
                        ),
                );
            }
        }
        if self.selection_error {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(self.locale.invalid_application()),
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
