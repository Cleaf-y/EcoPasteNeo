use super::shortcut::ModifierListener;
use anyhow::{Context, Result};
use ecopaste_neo_app::locale::Locale;
use ecopaste_neo_core::panel_shortcut::PanelShortcut;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use objc2::MainThreadMarker;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuId, MenuItem, Submenu},
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

/// Main-thread-owned integration using GPUI's existing AppKit event loop.
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
    _application_menu: Menu,
    show: MenuItem,
    settings: MenuItem,
    quit: MenuItem,
}

impl SystemIntegration {
    /// Create main-thread menu resources, then register the independent shortcut.
    pub fn new(locale: Locale, saved: Option<&PanelShortcut>) -> Result<Self> {
        MainThreadMarker::new().context("Tray and global hotkey setup requires the main thread")?;
        let manager =
            GlobalHotKeyManager::new().context("Could not initialize the global hotkey manager")?;
        let show = MenuItem::new(locale.show(), true, None);
        let quit = MenuItem::new(locale.quit(), true, None);
        let settings = MenuItem::new(locale.settings_title(), true, None);
        let menu = Menu::with_items(&[&show, &settings, &quit])
            .context("Could not build the tray menu")?;
        let application = Submenu::with_items(
            ecopaste_neo_core::identity::PRODUCT_NAME,
            true,
            &[&show, &settings, &quit],
        )
        .context("Could not build the application menu")?;
        let application_menu =
            Menu::with_items(&[&application]).context("Could not build the menu bar")?;
        application_menu.init_for_nsapp();
        let tray = TrayIconBuilder::new()
            .with_tooltip("EcoPasteNeo")
            .with_title("Neo")
            .with_icon(Self::icon()?)
            .with_icon_as_template(true)
            .with_menu(Box::new(menu.clone()))
            .with_menu_on_left_click(true)
            .build()
            .context("Could not create the macOS menu bar icon")?;
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
                    modifier: listener.modifier,
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
                    super::hotkey_probe::check_available(key, self.locale)?;
                    self.manager
                        .register(key)
                        .map_err(|_| anyhow::anyhow!("{}", self.locale.shortcut_conflict()))?;
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

    /// Draw a monochrome clipboard template that follows the menu bar appearance.
    fn icon() -> Result<Icon> {
        const SIZE: u32 = 18;
        let mut rgba = vec![0; (SIZE * SIZE * 4) as usize];
        // A monochrome clipboard outline and two content strokes. AppKit's
        // template rendering adapts the black/transparent mark to light/dark mode.
        for y in 0..SIZE {
            for x in 0..SIZE {
                let outline = ((x == 4 || x == 13) && (4..=15).contains(&y))
                    || ((y == 4 || y == 15) && (4..=13).contains(&x));
                let clip = (7..=10).contains(&x) && (2..=5).contains(&y);
                let strokes = (y == 8 || y == 11) && (7..=10).contains(&x);
                if outline || clip || strokes {
                    rgba[((y * SIZE + x) * 4 + 3) as usize] = 255;
                }
            }
        }
        Icon::from_rgba(rgba, SIZE, SIZE).context("Could not generate the tray icon")
    }

    /// Drain native receivers from the existing GPUI event loop.
    pub fn next_action(&mut self) -> Option<SystemAction> {
        assert!(
            MainThreadMarker::new().is_some(),
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
    if key.mods.is_empty() || super::hotkey_probe::scan_code(&key.key.to_string()).is_none() {
        anyhow::bail!("{}", locale.shortcut_invalid());
    }
    Ok(key)
}
