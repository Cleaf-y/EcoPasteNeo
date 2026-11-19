//! Carbon's default nonexclusive registration cannot detect another registrant.
//! Probe exclusively before handing the free combination to GlobalHotKeyManager.

use anyhow::{Result, bail};
use ecopaste_neo_app::locale::Locale;
use global_hotkey::hotkey::{HotKey, Modifiers};
use std::ffi::c_void;

#[repr(C)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: HotKeyId,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(key: *mut c_void) -> i32;
}

/// Physical ANSI/function/navigation key codes from the macOS SDK's Events.h.
/// Media controls are deliberately excluded: they use a separate event listener.
pub fn scan_code(key: &str) -> Option<u32> {
    let names = [
        ("KeyA", 0x00),
        ("KeyS", 0x01),
        ("KeyD", 0x02),
        ("KeyF", 0x03),
        ("KeyH", 0x04),
        ("KeyG", 0x05),
        ("KeyZ", 0x06),
        ("KeyX", 0x07),
        ("KeyC", 0x08),
        ("KeyV", 0x09),
        ("KeyB", 0x0b),
        ("KeyQ", 0x0c),
        ("KeyW", 0x0d),
        ("KeyE", 0x0e),
        ("KeyR", 0x0f),
        ("KeyY", 0x10),
        ("KeyT", 0x11),
        ("Digit1", 0x12),
        ("Digit2", 0x13),
        ("Digit3", 0x14),
        ("Digit4", 0x15),
        ("Digit6", 0x16),
        ("Digit5", 0x17),
        ("Equal", 0x18),
        ("Digit9", 0x19),
        ("Digit7", 0x1a),
        ("Minus", 0x1b),
        ("Digit8", 0x1c),
        ("Digit0", 0x1d),
        ("BracketRight", 0x1e),
        ("KeyO", 0x1f),
        ("KeyU", 0x20),
        ("BracketLeft", 0x21),
        ("KeyI", 0x22),
        ("KeyP", 0x23),
        ("Enter", 0x24),
        ("KeyL", 0x25),
        ("KeyJ", 0x26),
        ("Quote", 0x27),
        ("KeyK", 0x28),
        ("Semicolon", 0x29),
        ("Backslash", 0x2a),
        ("Comma", 0x2b),
        ("Slash", 0x2c),
        ("KeyN", 0x2d),
        ("KeyM", 0x2e),
        ("Period", 0x2f),
        ("Tab", 0x30),
        ("Space", 0x31),
        ("Backquote", 0x32),
        ("Backspace", 0x33),
        ("Escape", 0x35),
        ("F17", 0x40),
        ("F18", 0x4f),
        ("F19", 0x50),
        ("F20", 0x5a),
        ("F5", 0x60),
        ("F6", 0x61),
        ("F7", 0x62),
        ("F3", 0x63),
        ("F8", 0x64),
        ("F9", 0x65),
        ("F11", 0x67),
        ("F13", 0x69),
        ("F16", 0x6a),
        ("F14", 0x6b),
        ("F10", 0x6d),
        ("F12", 0x6f),
        ("F15", 0x71),
        ("Insert", 0x72),
        ("Home", 0x73),
        ("PageUp", 0x74),
        ("Delete", 0x75),
        ("F4", 0x76),
        ("End", 0x77),
        ("F2", 0x78),
        ("PageDown", 0x79),
        ("F1", 0x7a),
        ("ArrowLeft", 0x7b),
        ("ArrowRight", 0x7c),
        ("ArrowDown", 0x7d),
        ("ArrowUp", 0x7e),
        ("NumpadDecimal", 0x41),
        ("NumpadMultiply", 0x43),
        ("NumpadAdd", 0x45),
        ("NumpadDivide", 0x4b),
        ("NumpadEnter", 0x4c),
        ("NumpadSubtract", 0x4e),
        ("NumpadEqual", 0x51),
        ("Numpad0", 0x52),
        ("Numpad1", 0x53),
        ("Numpad2", 0x54),
        ("Numpad3", 0x55),
        ("Numpad4", 0x56),
        ("Numpad5", 0x57),
        ("Numpad6", 0x58),
        ("Numpad7", 0x59),
        ("Numpad8", 0x5b),
        ("Numpad9", 0x5c),
    ];
    names
        .into_iter()
        .find_map(|(name, code)| (name == key).then_some(code))
}

/// A temporary exclusive registration detects an existing exclusive owner.
/// Both operations run on the main thread; no event pump occurs during the probe.
pub fn reserve_exclusive(key: HotKey, locale: Locale) -> Result<ExclusiveRegistration> {
    let code = scan_code(&key.key.to_string())
        .ok_or_else(|| anyhow::anyhow!("{}", locale.shortcut_invalid()))?;
    let mut flags = 0;
    for (modifier, flag) in [
        (Modifiers::SHIFT, 512),
        (Modifiers::SUPER, 256),
        (Modifiers::ALT, 2048),
        (Modifiers::CONTROL, 4096),
    ] {
        if key.mods.contains(modifier) {
            flags |= flag;
        }
    }
    let mut reference = std::ptr::null_mut();
    let status = unsafe {
        RegisterEventHotKey(
            code,
            flags,
            HotKeyId {
                signature: u32::from_be_bytes(*b"NeoP"),
                id: key.id(),
            },
            GetApplicationEventTarget(),
            1,
            &mut reference,
        )
    };
    if status != 0 {
        bail!("{}", locale.shortcut_conflict());
    }
    Ok(ExclusiveRegistration(reference))
}

/// The conflict fixture retains this exclusive native registration until explicit exit.
pub struct ExclusiveRegistration(*mut c_void);

impl Drop for ExclusiveRegistration {
    fn drop(&mut self) {
        unsafe {
            UnregisterEventHotKey(self.0);
        }
    }
}

/// Probe the OS reservation without replacing the manager used for event delivery.
pub fn check_available(key: HotKey, locale: Locale) -> Result<()> {
    drop(reserve_exclusive(key, locale)?);
    Ok(())
}
