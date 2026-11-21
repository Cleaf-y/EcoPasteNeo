mod drag;
pub mod login;
mod panel;
mod paste;
mod shortcut;

use std::path::Path;

use anyhow::{Context, Result};
use gpui::WindowingRequest;
use objc2::MainThreadMarker;
use objc2_foundation::{NSBundle, NSString};
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{ContextMenu, IsMenuItem, Menu, Submenu},
};

pub use drag::{NativeDrag, PressEvent, left_button_down};
pub use panel::{NativePanel, PasteTarget};
pub use paste::{check_paste_permission, send_paste_keys};
pub use shortcut::ModifierListener;

pub const DOUBLE_TAP_SUPPORTED: bool = true;

/// Request the accessory role before AppKit launches; the graphical backend stays available.
pub fn application() -> gpui::Application {
    gpui_platform::application().with_windowing(WindowingRequest::Headless)
}

pub fn ensure_main_thread() -> Result<()> {
    MainThreadMarker::new()
        .map(|_| ())
        .context("AppKit integration requires the main thread")
}

/// Accessory apps still own the menu bar while one of their windows is key.
pub fn install_application_menu(items: &[&dyn IsMenuItem]) -> Result<Option<Menu>> {
    let application = Submenu::with_items(ecopaste_neo_core::identity::PRODUCT_NAME, true, items)
        .context("Could not build the application menu")?;
    let menu = Menu::with_items(&[&application]).context("Could not build the menu bar")?;
    menu.init_for_nsapp();
    Ok(Some(menu))
}

/// The menu bar item opens its menu on click and follows the light/dark appearance.
pub fn tray_icon_builder(menu: &Menu) -> Result<TrayIconBuilder> {
    Ok(TrayIconBuilder::new()
        .with_title("Neo")
        .with_icon(tray_icon()?)
        .with_icon_as_template(true)
        .with_menu(Box::new(menu.clone()))
        .with_menu_on_left_click(true))
}

/// Draw a monochrome clipboard template that follows the menu bar appearance.
fn tray_icon() -> Result<Icon> {
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

/// Exclusions match the bundle identifier reported for clipboard sources.
pub fn application_id_from_path(path: &Path) -> Option<String> {
    if path.extension()? != "app" {
        return None;
    }
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(path.to_str()?))?;
    let id = bundle.bundleIdentifier()?.to_string();
    (!id.is_empty() && !id.chars().any(char::is_whitespace)).then_some(id)
}

/// End native tracking of a context menu that is still open.
pub fn cancel_menu(menu: &Menu) {
    // SAFETY: muda owns this live NSMenu; every caller runs on the main thread.
    let menu = unsafe { &*menu.ns_menu().cast::<objc2_app_kit::NSMenu>() };
    menu.cancelTracking();
}
