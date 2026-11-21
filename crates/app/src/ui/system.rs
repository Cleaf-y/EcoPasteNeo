use crate::platform::{self, ModifierListener};
use anyhow::{Context, Result};
use ecopaste_neo_app::locale::Locale;
use ecopaste_neo_core::panel_shortcut::PanelShortcut;
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey},
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuId, MenuItem},
};

#[derive(Clone, Debug)]
pub enum SystemAction {
    Toggle,
    Show,
    Settings,
    Quit,
    Menu(MenuId),
}

enum Activation {
    Combination(HotKey),
    DoubleTap(ModifierListener),
}

/// Main-thread-owned integration using GPUI's existing platform event loop.
/// Poll next_action from GPUI; no event thread or second run loop is created.
pub struct SystemIntegration {
    manager: GlobalHotKeyManager,
    active: Option<Activation>,
    candidate: Option<Activation>,
    editing: bool,
    pressed: bool,
    recorded: Option<String>,
    error: Option<String>,
    locale: Locale,
    tray: TrayIcon,
    _menu: Menu,
    _application_menu: Option<Menu>,
    show: MenuItem,
    settings: MenuItem,
    quit: MenuItem,
}

impl SystemIntegration {
    /// Create main-thread menu resources, then register the independent shortcut.
    pub fn new(locale: Locale, saved: Option<&PanelShortcut>) -> Result<Self> {
        platform::ensure_main_thread()
            .context("Tray and global hotkey setup requires the main thread")?;
        let manager =
            GlobalHotKeyManager::new().context("Could not initialize the global hotkey manager")?;
        let show = MenuItem::new(locale.show(), true, None);
        let quit = MenuItem::new(locale.quit(), true, None);
        let settings = MenuItem::new(locale.settings_title(), true, None);
        let menu = Menu::with_items(&[&show, &settings, &quit])
            .context("Could not build the tray menu")?;
        let application_menu = platform::install_application_menu(&[&show, &settings, &quit])?;
        let tray = platform::tray_icon_builder(&menu)?
            .with_tooltip("EcoPasteNeo")
            .build()
            .context("Could not create the tray icon")?;
        let mut integration = Self {
            manager,
            active: None,
            candidate: None,
            editing: false,
            pressed: false,
            recorded: None,
            error: None,
            locale,
            tray,
            _menu: menu,
            _application_menu: application_menu,
            show,
            settings,
            quit,
        };
        if let Some(saved) = saved {
            match integration.prepare(saved, false) {
                Ok(()) => integration.finish(true),
                Err(error) => integration.error = Some(error.to_string()),
            }
        } else {
            integration.error = Some(locale.shortcut_settings_unavailable().into());
        }
        integration.refresh_hint();
        Ok(integration)
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The UI displays the active registration, independently of the editable draft.
    pub fn description(&self) -> String {
        match &self.active {
            Some(Activation::Combination(key)) => self.locale.combination_label(&key.to_string()),
            Some(Activation::DoubleTap(listener)) => {
                self.locale.shortcut_label(&PanelShortcut::DoubleTap {
                    modifier: listener.modifier(),
                })
            }
            None => self.locale.shortcut_inactive().into(),
        }
    }

    fn refresh_hint(&mut self) {
        let description = self.error.as_deref().unwrap_or("");
        let label = if self.active.is_some() {
            self.description()
        } else {
            format!("{}: {}", self.locale.shortcut_inactive(), description)
        };
        self.show
            .set_text(format!("{} · {label}", self.locale.show()));
        let _ = self
            .tray
            .set_tooltip(Some(format!("EcoPasteNeo — {label}")));
    }

    /// Suppress delivery without giving up the old registration; drain queued events.
    pub fn edit(&mut self, editing: bool) {
        self.editing = editing;
        self.recorded = None;
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if matches!(&self.active,Some(Activation::Combination(key)) if event.id()==key.id()) {
                self.pressed = event.state() == HotKeyState::Pressed;
            }
        }
        if let Some(Activation::DoubleTap(listener)) = &self.active {
            listener.arm(!editing);
        }
    }

    /// Carbon consumes the currently registered chord before GPUI's recorder sees it.
    pub fn take_recorded(&mut self) -> Option<String> {
        self.recorded.take()
    }

    /// Reserve the candidate before any settings write; the old activation remains owned.
    pub fn prepare(&mut self, choice: &PanelShortcut, request: bool) -> Result<()> {
        if self.candidate.is_some() {
            anyhow::bail!("{}", self.locale.saving());
        }
        self.candidate = match choice {
            PanelShortcut::Combination { key } => {
                let key = parse_combination(key, self.locale)?;
                if matches!(&self.active,Some(Activation::Combination(old)) if old.id()==key.id()) {
                    None
                } else {
                    self.manager
                        .register(key)
                        .map_err(|error| registration_error(error, self.locale))?;
                    Some(Activation::Combination(key))
                }
            }
            PanelShortcut::DoubleTap { modifier } => Some(Activation::DoubleTap(
                ModifierListener::new(*modifier, request, self.locale)?,
            )),
        };
        self.edit(true);
        Ok(())
    }

    /// Publish only a successfully persisted candidate; failed writes release only the candidate.
    pub fn finish(&mut self, saved: bool) {
        if let Some(candidate) = self.candidate.take() {
            if saved {
                self.pressed = false;
                if let Some(old) = self.active.replace(candidate) {
                    self.release(old);
                }
            } else {
                self.release(candidate);
            }
        }
        if saved {
            self.error = None;
        }
        self.edit(false);
        self.refresh_hint();
    }

    fn release(&self, activation: Activation) {
        if let Activation::Combination(key) = activation
            && let Err(error) = self.manager.unregister(key)
        {
            eprintln!("Could not unregister panel shortcut: {error}");
        }
    }

    /// Drain native receivers from the existing GPUI event loop.
    pub fn next_action(&mut self) -> Option<SystemAction> {
        assert!(
            platform::ensure_main_thread().is_ok(),
            "System events must be polled on the main thread"
        );
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id() == self.show.id() {
                return Some(SystemAction::Show);
            }
            if event.id() == self.quit.id() {
                return Some(SystemAction::Quit);
            }
            if event.id() == self.settings.id() {
                return Some(SystemAction::Settings);
            }
            return Some(SystemAction::Menu(event.id));
        }
        if let Some(Activation::DoubleTap(listener)) = &self.active {
            if !listener.available() {
                let old = self.active.take().expect("active listener");
                self.release(old);
                self.error = Some(self.locale.shortcut_listener_failed().into());
                self.refresh_hint();
            } else if listener.take() && !self.editing {
                return Some(SystemAction::Toggle);
            }
        }
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if !matches!(&self.active,Some(Activation::Combination(key)) if event.id()==key.id()) {
                continue;
            }
            let pressed = event.state() == HotKeyState::Pressed;
            let repeated = self.pressed;
            self.pressed = pressed;
            if pressed
                && !repeated
                && self.editing
                && let Some(Activation::Combination(key)) = &self.active
            {
                self.recorded = Some(key.to_string());
            }
            if pressed && !repeated && !self.editing {
                return Some(SystemAction::Toggle);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                id,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
                && id == self.tray.id()
            {
                return Some(SystemAction::Show);
            }
        }
        None
    }
}

impl Drop for SystemIntegration {
    fn drop(&mut self) {
        if let Some(candidate) = self.candidate.take() {
            self.release(candidate);
        }
        if let Some(active) = self.active.take() {
            self.release(active);
        }
    }
}

/// Validate the same representation used by the recorder and persisted configuration.
pub fn parse_combination(key: &str, locale: Locale) -> Result<HotKey> {
    let key: HotKey = key
        .parse()
        .map_err(|_| anyhow::anyhow!("{}", locale.shortcut_invalid()))?;
    if key.mods.is_empty()
        || matches!(
            key.key,
            Code::MediaPlayPause
                | Code::MediaTrackNext
                | Code::MediaTrackPrevious
                | Code::MediaFastForward
                | Code::MediaRewind
        )
    {
        anyhow::bail!("{}", locale.shortcut_invalid());
    }
    Ok(key)
}

/// Occupancy is actionable separately from an unsupported key or native API failure.
fn registration_error(error: global_hotkey::Error, locale: Locale) -> anyhow::Error {
    match error {
        global_hotkey::Error::AlreadyRegistered(_) => {
            anyhow::anyhow!("{}", locale.shortcut_conflict())
        }
        error => anyhow::anyhow!(
            "{}",
            locale.shortcut_registration_failed(&error.to_string())
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use global_hotkey::hotkey::Modifiers;

    #[test]
    fn legacy_super_keeps_command_semantics_and_display() {
        for value in ["Control+Alt+Super+KeyV", "control+alt+super+KeyN"] {
            let key = parse_combination(value, Locale::ZhCn).unwrap();
            assert!(key.mods.contains(Modifiers::META));
            assert!(key.mods.contains(Modifiers::CONTROL | Modifiers::ALT));
            assert_eq!(key.to_string().parse::<HotKey>().unwrap(), key);
            let label = Locale::ZhCn.combination_label(&key.to_string());
            let expected = if cfg!(target_os = "windows") {
                "Ctrl+Alt+Win+"
            } else {
                "⌃⌥⌘"
            };
            assert!(label.starts_with(expected), "{label}");
            assert!(!label.contains("meta"));
        }
    }

    #[test]
    fn occupied_and_other_registration_errors_remain_distinct() {
        let key = parse_combination("Control+Super+KeyV", Locale::EnUs).unwrap();
        let conflict =
            registration_error(global_hotkey::Error::AlreadyRegistered(key), Locale::EnUs)
                .to_string();
        assert_eq!(conflict, Locale::EnUs.shortcut_conflict());
        let failure = registration_error(
            global_hotkey::Error::FailedToRegister("native fixture failure".into()),
            Locale::EnUs,
        )
        .to_string();
        assert!(failure.contains("native fixture failure"));
        assert_ne!(failure, conflict);
    }

    #[test]
    fn combinations_cannot_enable_media_key_monitoring() {
        for key in [
            "MediaPlayPause",
            "MediaTrackNext",
            "MediaTrackPrevious",
            "MediaFastForward",
            "MediaRewind",
        ] {
            assert!(parse_combination(&format!("Control+{key}"), Locale::EnUs).is_err());
        }
    }
}
