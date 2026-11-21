use anyhow::{Result, bail};
use ecopaste_neo_app::locale::Locale;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};

/// Windows grants key injection without a user-facing permission. Input into windows of a
/// higher integrity level (an elevated target) is dropped silently by the system instead.
pub fn check_paste_permission(_locale: Locale) -> Result<()> {
    Ok(())
}

/// Send one complete Ctrl+V. Ctrl+V works in Chromium, Electron and Monaco editors, where
/// Shift+Insert toggles overwrite mode instead of pasting.
pub fn send_paste_keys() -> Result<()> {
    let inputs = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    // SAFETY: `inputs` is a fully initialized INPUT array of the declared size.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        bail!("SendInput injected {sent}/{} key events", inputs.len());
    }
    Ok(())
}

fn key(code: VIRTUAL_KEY, release: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: code,
                dwFlags: if release {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    }
}
