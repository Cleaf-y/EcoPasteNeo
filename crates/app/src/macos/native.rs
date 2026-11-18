use anyhow::{Context, Result, anyhow, bail};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{NSEvent, NSScreen, NSView, NSWindow, NSWindowOrderingMode, NSWorkspace};
use objc2_foundation::{NSMouseInRect, NSPoint, NSRect, NSSize, ns_string};
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

    /// Create child windows on the current native screen so their initial scale matches.
    pub fn display_id(&self) -> Option<gpui::DisplayId> {
        let screen = self.native().screen()?;
        let number = screen
            .deviceDescription()
            .objectForKey(ns_string!("NSScreenNumber"))?;
        // SAFETY: AppKit's documented NSScreenNumber entry is an NSNumber display ID.
        let id: u32 = unsafe { msg_send![&*number, unsignedIntValue] };
        Some(gpui::DisplayId::new(u64::from(id)))
    }

    /// Show a child PopUp beside the history panel without activating or recapturing a target.
    pub fn show_preview(&self, parent: &Panel) {
        let window = self.native();
        // SAFETY: This helper is used only for GPUI WindowKind::PopUp, an NSPanel subclass.
        unsafe {
            let _: () = msg_send![window, setBecomesKeyOnlyIfNeeded: true];
        }
        if let Some(screen) = parent.native().screen() {
            let area = screen.visibleFrame();
            let anchor = parent.native().frame();
            let width = 480.0_f64.min(area.size.width - 24.0).max(1.0);
            let height = 600.0_f64.min(area.size.height - 24.0).max(1.0);
            let right = anchor.origin.x + anchor.size.width + 12.0;
            let left = anchor.origin.x - width - 12.0;
            let preferred = if right + width <= area.origin.x + area.size.width - 12.0 {
                right
            } else {
                left
            };
            let x = preferred.clamp(
                area.origin.x + 12.0,
                (area.origin.x + area.size.width - width - 12.0).max(area.origin.x + 12.0),
            );
            let y = (anchor.origin.y + anchor.size.height - height).clamp(
                area.origin.y + 12.0,
                (area.origin.y + area.size.height - height - 12.0).max(area.origin.y + 12.0),
            );
            window.setFrame_display(
                NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)),
                true,
            );
        }
        if window.parentWindow().is_none() {
            // SAFETY: Both live GPUI windows are accessed on their main thread.
            unsafe {
                parent
                    .native()
                    .addChildWindow_ordered(window, NSWindowOrderingMode::Above);
            }
        }
        window.orderFront(None);
    }

    /// Detach a closed preview so showing its parent cannot implicitly show it again.
    pub fn hide_preview(&self) {
        let window = self.native();
        window.orderOut(None);
        if let Some(parent) = window.parentWindow() {
            parent.removeChildWindow(window);
        }
    }

    pub fn is_key(&self) -> bool {
        self.native().isKeyWindow()
    }

    pub(super) fn content_view(&self) -> objc2::rc::Retained<NSView> {
        self.native()
            .contentView()
            .expect("GPUI window has a content view")
    }

    /// Run native tracking outside GPUI entity updates: AppKit dispatches nested events.
    pub fn popup_menu(&self, menu: &tray_icon::menu::Menu, position: gpui::Point<gpui::Pixels>) {
        use tray_icon::menu::{ContextMenu, dpi::LogicalPosition};
        let view = self
            .native()
            .contentView()
            .expect("GPUI window has a content view");
        // SAFETY: GPUI keeps the NSView and window alive; this task runs on the main thread.
        unsafe {
            menu.show_context_menu_for_nsview(
                (&*view as *const NSView).cast(),
                Some(
                    LogicalPosition::new(
                        f64::from(f32::from(position.x)),
                        f64::from(f32::from(position.y)),
                    )
                    .into(),
                ),
            );
        }
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
