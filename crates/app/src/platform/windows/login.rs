//! Launch at login has no Windows registration yet; Settings shows it as unavailable.
use anyhow::{Result, bail};
use ecopaste_neo_app::locale::Locale;
use windows::{
    Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    core::w,
};

use crate::platform::LoginStatus;

/// Windows startup entries pass `--auto-launch`, which `Options` already reads.
pub fn launched_at_login() -> bool {
    false
}

pub fn status(_locale: Locale) -> Result<LoginStatus> {
    Ok(LoginStatus::Unsupported)
}

pub fn set_enabled(_enabled: bool, locale: Locale) -> Result<()> {
    bail!("{}", locale.login_unsupported())
}

/// Open Settings → Apps → Startup in response to an explicit button.
pub fn open_system_settings() {
    // SAFETY: Static wide strings; ShellExecuteW reports failure through its return value.
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("ms-settings:startupapps"),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}
