//! Native integration shared by the GPUI views in `ui/`.
//!
//! Each platform module exports the same names; `ui/` reaches the operating system only
//! through these re-exports and never uses `cfg(target_os)` itself. Everything here runs on
//! GPUI's main thread unless stated otherwise.
//!
//! - `application()` builds the GPUI application with the platform's process role.
//! - `NativePanel` borrows a live GPUI window: show at the pointer's screen, hide, place the
//!   preview beside its parent, pop up a context menu, and hand keyboard focus to a
//!   `PasteTarget` and back.
//! - `PasteTarget` is the external window or process captured when the panel is shown.
//!   `is_available` allows preparing a paste; `is_foreground` must hold right before
//!   `send_paste_keys` injects the shortcut.
//! - `ModifierListener` observes double taps of one modifier without consuming input.
//! - `NativeDrag` starts a copy-only file drag from a captured mouse press.
//! - `login` reads and changes the system-owned launch-at-login state.

use ecopaste_neo_app::locale::Locale;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("EcoPasteNeo supports macOS and Windows only");

pub use imp::{
    DOUBLE_TAP_SUPPORTED, ModifierListener, NativeDrag, NativePanel, PasteTarget, PressEvent,
    application, application_id_from_path, cancel_menu, check_paste_permission, ensure_main_thread,
    install_application_menu, left_button_down, login, send_paste_keys, tray_icon_builder,
};

/// System-owned login state. Never mirror this state into the settings file.
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only SMAppService reports registration states")
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
    Unsupported,
    Unpackaged,
}

impl LoginStatus {
    pub fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::NotRegistered => locale.login_disabled(),
            Self::Enabled => locale.login_enabled(),
            Self::RequiresApproval => locale.login_approval(),
            Self::NotFound => locale.login_not_found(),
            Self::Unsupported => locale.login_unsupported(),
            Self::Unpackaged => locale.login_packaged_app(),
        }
    }

    pub fn can_change(self) -> bool {
        !matches!(self, Self::Unsupported | Self::Unpackaged)
    }
}
