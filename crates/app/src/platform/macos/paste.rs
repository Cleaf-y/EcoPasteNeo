use anyhow::{Result, anyhow, bail};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use ecopaste_neo_app::locale::Locale;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// Check without prompting; denial must precede clipboard mutation and key injection.
pub fn check_paste_permission(locale: Locale) -> Result<()> {
    if unsafe { AXIsProcessTrusted() } {
        return Ok(());
    }
    let executable = std::env::current_exe()?;
    let bundle = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"));
    let path = bundle.unwrap_or(&executable).display();
    bail!("{}: {}", locale.permission_required(), path)
}

/// Construct both events before posting either, then send one complete Command-V.
pub fn send_paste_keys() -> Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow!("Could not create keyboard event source"))?;
    let down = CGEvent::new_keyboard_event(source.clone(), 0x09, true)
        .map_err(|_| anyhow!("Could not create key-down event"))?;
    let up = CGEvent::new_keyboard_event(source, 0x09, false)
        .map_err(|_| anyhow!("Could not create key-up event"))?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);
    up.post(CGEventTapLocation::HID);
    Ok(())
}
