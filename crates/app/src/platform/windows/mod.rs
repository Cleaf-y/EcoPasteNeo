mod drag;
pub mod login;
mod panel;
mod paste;
mod shortcut;

use std::{
    path::Path,
    sync::OnceLock,
    thread::{self, ThreadId},
};

use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{IsMenuItem, Menu},
};
use windows::Win32::UI::WindowsAndMessaging::EndMenu;

pub use drag::{NativeDrag, PressEvent, left_button_down};
pub use panel::{NativePanel, PasteTarget};
pub use paste::{check_paste_permission, send_paste_keys};
pub use shortcut::ModifierListener;

pub const DOUBLE_TAP_SUPPORTED: bool = false;

static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();

/// The thread that builds the application runs GPUI's Win32 message loop.
pub fn application() -> gpui::Application {
    MAIN_THREAD.get_or_init(|| thread::current().id());
    gpui_platform::application()
}

pub fn ensure_main_thread() -> Result<()> {
    (MAIN_THREAD.get() == Some(&thread::current().id()))
        .then_some(())
        .context("Windows UI integration requires GPUI's main thread")
}

/// Windows has no application-wide menu bar; the tray menu carries the same items.
pub fn install_application_menu(_items: &[&dyn IsMenuItem]) -> Result<Option<Menu>> {
    Ok(None)
}

/// Left click shows the panel; the menu opens on right click as usual for notification icons.
pub fn tray_icon_builder(menu: &Menu) -> Result<TrayIconBuilder> {
    Ok(TrayIconBuilder::new()
        .with_icon(tray_icon()?)
        .with_menu(Box::new(menu.clone()))
        .with_menu_on_left_click(false))
}

/// Draw a colored clipboard that stays visible on both light and dark taskbars.
fn tray_icon() -> Result<Icon> {
    const SIZE: u32 = 32;
    const BODY: [u8; 4] = [0x25, 0x63, 0xeb, 0xff];
    const CLIP: [u8; 4] = [0x1e, 0x3a, 0x8a, 0xff];
    const LINE: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
    let mut rgba = vec![0; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let clip = (11..=20).contains(&x) && (2..=7).contains(&y);
            let line = matches!(y, 13 | 14 | 19 | 20 | 25) && (10..=21).contains(&x);
            let body = (5..=26).contains(&x) && (5..=29).contains(&y);
            let color = if clip {
                CLIP
            } else if line && body {
                LINE
            } else if body {
                BODY
            } else {
                continue;
            };
            let offset = ((y * SIZE + x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&color);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).context("Could not generate the tray icon")
}

/// Clipboard sources are identified by their executable's absolute path.
pub fn application_id_from_path(path: &Path) -> Option<String> {
    let executable = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
    if !executable || !path.is_absolute() {
        return None;
    }
    path.to_str().map(str::to_owned)
}

/// End the modal popup-menu loop running on this thread.
pub fn cancel_menu(_menu: &Menu) {
    // SAFETY: EndMenu only affects menus owned by the calling thread.
    let _ = unsafe { EndMenu() };
}
