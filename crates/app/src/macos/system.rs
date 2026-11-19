use anyhow::{Context, Result};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
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

/// Main-thread-owned integration using GPUI's existing AppKit event loop.
/// Poll next_action from GPUI; no event thread or second run loop is created.
pub struct SystemIntegration {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
    tray: TrayIcon,
    _menu: Menu,
    _application_menu: Menu,
    show: MenuItem,
    settings: MenuItem,
    quit: MenuItem,
}

impl SystemIntegration {
    /// Create main-thread menu resources, then register the independent shortcut.
    pub fn new(locale: ecopaste_neo_app::locale::Locale) -> Result<Self> {
        MainThreadMarker::new().context("Tray and global hotkey setup requires the main thread")?;
        let manager =
            GlobalHotKeyManager::new().context("Could not initialize the global hotkey manager")?;
        let hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER),
            Code::KeyV,
        );
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
            .with_tooltip("EcoPasteNeo — Ctrl+Alt+Cmd+V")
            .with_title("Neo")
            .with_icon(Self::icon()?)
            .with_icon_as_template(true)
            .with_menu(Box::new(menu.clone()))
            .with_menu_on_left_click(true)
            .build()
            .context("Could not create the macOS menu bar icon")?;
        // Register last: any earlier setup failure leaves no registered shortcut.
        manager
            .register(hotkey)
            .context("Could not register Ctrl+Alt+Cmd+V; the shortcut may already be in use")?;
        Ok(Self {
            manager,
            hotkey,
            tray,
            _menu: menu,
            _application_menu: application_menu,
            show,
            settings,
            quit,
        })
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
    pub fn next_action(&self) -> Option<SystemAction> {
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
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id() == self.hotkey.id() && event.state() == HotKeyState::Pressed {
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
        if let Err(error) = self.manager.unregister(self.hotkey) {
            eprintln!("Could not unregister Ctrl+Alt+Cmd+V: {error}");
        }
        // Tray/menu handles are then dropped, removing the status item.
    }
}
