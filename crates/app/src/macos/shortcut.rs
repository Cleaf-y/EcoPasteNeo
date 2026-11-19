//! Passive global modifier observation on GPUI's existing main run loop.

use anyhow::{Result, bail};
use core_foundation::{
    base::TCFType,
    mach_port::CFMachPortInvalidate,
    runloop::{CFRunLoop, CFRunLoopSource, kCFRunLoopCommonModes},
};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType, EventField,
};
use ecopaste_neo_app::{
    double_tap::{DoubleTap, TapEvent},
    locale::Locale,
};
use ecopaste_neo_core::panel_shortcut::TapModifier;
use foreign_types::ForeignType;
use objc2_app_kit::NSEvent;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn CGEventGetTimestamp(event: core_graphics::sys::CGEventRef) -> u64;
    fn CGEventTapIsEnabled(tap: core_foundation::mach_port::CFMachPortRef) -> bool;
}

/// Owns the callback until the run-loop source and Mach port are invalidated.
pub struct ModifierListener {
    pub modifier: TapModifier,
    tap: CGEventTap<'static>,
    source: CFRunLoopSource,
    enabled: Rc<Cell<bool>>,
    fired: Rc<Cell<bool>>,
    gesture: Rc<RefCell<DoubleTap>>,
}

impl ModifierListener {
    /// Permission prompts are allowed only after an explicit double-tap save.
    pub fn new(modifier: TapModifier, request: bool, locale: Locale) -> Result<Self> {
        if !unsafe { CGPreflightListenEventAccess() }
            && !(request && unsafe { CGRequestListenEventAccess() })
        {
            bail!("{}", locale.shortcut_permission());
        }
        let interval = Duration::from_secs_f64(NSEvent::doubleClickInterval());
        let gesture = Rc::new(RefCell::new(DoubleTap::new(interval)));
        let enabled = Rc::new(Cell::new(false));
        let fired = Rc::new(Cell::new(false));
        let (state, armed, output) = (gesture.clone(), enabled.clone(), fired.clone());
        let (codes, flag) = match modifier {
            TapModifier::Control => ([59, 62], 1 << 18),
            TapModifier::Option => ([58, 61], 1 << 19),
            TapModifier::Shift => ([56, 60], 1 << 17),
            TapModifier::Command => ([55, 54], 1 << 20),
        };
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::TailAppendEventTap,
            CGEventTapOptions::ListenOnly,
            vec![
                CGEventType::FlagsChanged,
                CGEventType::KeyDown,
                CGEventType::KeyUp,
            ],
            move |_, kind, event| {
                if !armed.get() {
                    return None;
                }
                let key = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                let input = match kind {
                    CGEventType::FlagsChanged => {
                        // Device-independent keyboard modifiers, excluding Caps Lock's toggle state.
                        let flags = event.get_flags().bits() & 0xfe0000;
                        TapEvent::Modifier {
                            key,
                            selected: codes.contains(&key),
                            alone: flags == flag,
                            released: flags == 0,
                        }
                    }
                    CGEventType::KeyDown => TapEvent::KeyDown(key),
                    CGEventType::KeyUp => TapEvent::KeyUp(key),
                    _ => TapEvent::Cancel,
                };
                let now = Duration::from_nanos(unsafe { CGEventGetTimestamp(event.as_ptr()) });
                if state.borrow_mut().observe(input, now) {
                    output.set(true);
                }
                // ListenOnly cannot alter, swallow or synthesize any keyboard event.
                None
            },
        )
        .map_err(|_| anyhow::anyhow!("{}", locale.shortcut_listener_failed()))?;
        let source = match tap.mach_port.create_runloop_source(0) {
            Ok(source) => source,
            Err(()) => {
                unsafe {
                    CFMachPortInvalidate(tap.mach_port.as_concrete_TypeRef());
                }
                bail!("{}", locale.shortcut_listener_failed());
            }
        };
        unsafe {
            CFRunLoop::get_main().add_source(&source, kCFRunLoopCommonModes);
        }
        tap.enable();
        Ok(Self {
            modifier,
            tap,
            source,
            enabled,
            fired,
            gesture,
        })
    }

    pub fn available(&self) -> bool {
        unsafe {
            CGPreflightListenEventAccess()
                && CGEventTapIsEnabled(self.tap.mach_port.as_concrete_TypeRef())
        }
    }

    /// Arming always discards pre-edit taps and pending delivery.
    pub fn arm(&self, enabled: bool) {
        self.enabled.set(enabled);
        self.fired.set(false);
        *self.gesture.borrow_mut() =
            DoubleTap::new(Duration::from_secs_f64(NSEvent::doubleClickInterval()));
    }

    pub fn take(&self) -> bool {
        self.fired.replace(false)
    }
}

impl Drop for ModifierListener {
    fn drop(&mut self) {
        self.enabled.set(false);
        unsafe {
            CFRunLoop::get_main().remove_source(&self.source, kCFRunLoopCommonModes);
            CFMachPortInvalidate(self.tap.mach_port.as_concrete_TypeRef());
        }
    }
}
