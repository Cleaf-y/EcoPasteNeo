use anyhow::{Context, Result, anyhow, bail};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSScreen, NSView, NSWindow, NSWorkspace};
use objc2_foundation::{NSMouseInRect, NSPoint};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{marker::PhantomData, ptr::NonNull, rc::Rc};

/// Borrowed identity of GPUI's existing native window, not a replacement NSPanel.
///
/// Create and use only on the main thread, while the original GPUI window is alive.
/// Copies do not retain the NSWindow and must not outlive it. The Rc marker prevents
/// sending this identity to another thread. No style, behavior, or level is changed.
#[derive(Clone, Copy)]
pub struct Panel {
    window: NonNull<NSWindow>,
    _main_thread_only: PhantomData<Rc<()>>,
}

impl Panel {
    /// Borrow the live NSWindow owned by this GPUI window on the main thread.
    pub fn from_window(window: &gpui::Window) -> Result<Self> {
        MainThreadMarker::new().context("Native panel creation requires the main thread")?;
        // Use the trait explicitly: GPUI also has an unrelated window_handle method.
        let handle = HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow!("GPUI did not provide a native window handle: {error}"))?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            bail!(
                "Expected an AppKit window handle; EcoPasteNeo GPUI currently supports only macOS"
            );
        };
        // The raw-window-handle contract supplies a live NSView owned by GPUI.
        let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        let native = view.window().context("GPUI's NSView has no NSWindow")?;
        Ok(Self {
            window: NonNull::from(&*native),
            _main_thread_only: PhantomData,
        })
    }

    fn native(&self) -> &NSWindow {
        assert!(
            MainThreadMarker::new().is_some(),
            "AppKit operation requires the main thread"
        );
        // The caller keeps the original GPUI window alive, as required above.
        unsafe { self.window.as_ref() }
    }

    /// Center the panel on the pointer's screen at show time, without activating the app.
    pub fn show(&self) {
        let window = self.native();
        let pointer = NSEvent::mouseLocation();
        let screens = NSScreen::screens(window.mtm());
        for screen in &screens {
            if !NSMouseInRect(pointer, screen.frame(), false) {
                continue;
            }
            let area = screen.visibleFrame();
            let size = window.frame().size;
            window.setFrameOrigin(NSPoint::new(
                area.origin.x + (area.size.width - size.width) * 0.5,
                area.origin.y + (area.size.height - size.height) * 0.5,
            ));
            break;
        }
        window.makeKeyAndOrderFront(None);
    }

    /// Give keys back to the still-active target without activating any application.
    pub fn resign_key(&self) {
        self.native().resignKeyWindow();
    }

    pub fn restore_key(&self) {
        self.native().makeKeyWindow();
    }

    pub fn hide(&self) {
        self.native().orderOut(None);
    }
}

/// Nonactivating panels keep the external application's process frontmost.
pub fn frontmost_pid() -> Option<i32> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = app.processIdentifier();
    (pid != std::process::id() as i32).then_some(pid)
}
