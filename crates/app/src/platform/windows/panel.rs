use std::{ffi::c_void, marker::PhantomData, num::NonZeroIsize, rc::Rc};

use anyhow::{Result, anyhow, bail};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use tray_icon::menu::{ContextMenu, Menu, dpi::LogicalPosition};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{
        GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
        MonitorFromWindow,
    },
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::{
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{
            BringWindowToTop, GetCursorPos, GetForegroundWindow, GetWindowRect,
            GetWindowThreadProcessId, HWND_TOPMOST, IsWindow, SW_HIDE, SWP_NOACTIVATE, SWP_NOSIZE,
            SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
        },
    },
};

/// Borrowed HWND of GPUI's existing window; GPUI keeps creating and destroying it.
///
/// Use only on GPUI's main thread while the original window is alive. The panel is an
/// ordinary activating window: it takes the foreground when shown and gives it back to the
/// captured target right before a paste.
#[derive(Clone, Copy)]
pub struct NativePanel {
    hwnd: HWND,
    _main_thread_only: PhantomData<Rc<()>>,
}

impl NativePanel {
    pub fn from_window(window: &gpui::Window) -> Result<Self> {
        // Use the trait explicitly: GPUI also has an unrelated window_handle method.
        let handle = HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow!("GPUI did not provide a native window handle: {error}"))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            bail!("Expected a Win32 window handle");
        };
        Ok(Self {
            hwnd: HWND(handle.hwnd.get() as *mut c_void),
            _main_thread_only: PhantomData,
        })
    }

    /// Center on the pointer's monitor, then take the foreground so typing reaches the search box.
    pub fn show(&self) {
        let mut pointer = POINT::default();
        // SAFETY: Plain Win32 queries on a live window owned by this thread.
        unsafe {
            let monitor = match GetCursorPos(&mut pointer) {
                Ok(()) => MonitorFromPoint(pointer, MONITOR_DEFAULTTONEAREST),
                Err(_) => MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST),
            };
            match (work_area(monitor), window_rect(self.hwnd)) {
                (Some(area), Some(frame)) => {
                    let width = frame.right - frame.left;
                    let height = frame.bottom - frame.top;
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_TOPMOST),
                        area.left + (area.right - area.left - width) / 2,
                        area.top + (area.bottom - area.top - height) / 2,
                        0,
                        0,
                        SWP_NOSIZE | SWP_SHOWWINDOW,
                    );
                }
                _ => {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOSIZE | SWP_SHOWWINDOW,
                    );
                }
            }
        }
        bring_to_foreground(self.hwnd);
    }

    /// GPUI identifies a Windows display by its HMONITOR.
    pub fn display_id(&self) -> Option<gpui::DisplayId> {
        // SAFETY: MonitorFromWindow accepts any window handle and always returns a monitor here.
        let monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) };
        (!monitor.is_invalid()).then(|| gpui::DisplayId::new(monitor.0 as u64))
    }

    /// Show the preview beside its parent without taking the foreground from the panel.
    pub fn show_preview(&self, parent: &NativePanel) {
        // SAFETY: Both windows are live GPUI windows owned by this thread.
        unsafe {
            let scale = GetDpiForWindow(parent.hwnd).max(96) as f32 / 96.0;
            let pixels = |value: f32| (value * scale).round() as i32;
            let monitor = MonitorFromWindow(parent.hwnd, MONITOR_DEFAULTTONEAREST);
            let (Some(area), Some(anchor)) = (work_area(monitor), window_rect(parent.hwnd)) else {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                return;
            };
            let margin = pixels(12.0);
            let width = pixels(480.0)
                .min(area.right - area.left - 2 * margin)
                .max(1);
            let height = pixels(600.0)
                .min(area.bottom - area.top - 2 * margin)
                .max(1);
            let right = anchor.right + margin;
            let left = anchor.left - width - margin;
            let preferred = if right + width <= area.right - margin {
                right
            } else {
                left
            };
            let x = preferred.clamp(
                area.left + margin,
                (area.right - width - margin).max(area.left + margin),
            );
            let y = anchor.top.clamp(
                area.top + margin,
                (area.bottom - height - margin).max(area.top + margin),
            );
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    pub fn hide_preview(&self) {
        self.hide();
    }

    pub fn is_key(&self) -> bool {
        // SAFETY: Reading the foreground window has no preconditions.
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    /// muda runs the modal popup loop and posts the chosen item through `MenuEvent`.
    pub fn popup_menu(&self, menu: &Menu, position: gpui::Point<gpui::Pixels>) {
        let position = LogicalPosition::new(
            f64::from(f32::from(position.x)),
            f64::from(f32::from(position.y)),
        );
        // SAFETY: GPUI keeps this HWND alive; the caller runs on the window's thread.
        unsafe {
            menu.show_context_menu_for_hwnd(self.hwnd.0 as isize, Some(position.into()));
        }
    }

    /// Before pasting, hand the foreground back while Neo still owns it: once the panel is
    /// hidden, Windows no longer lets this process choose the next foreground window.
    pub fn yield_to(&self, target: &PasteTarget, _staying_visible: bool) {
        // SAFETY: SetForegroundWindow validates the handle and reports failure by return value.
        let _ = unsafe { SetForegroundWindow(target.0) };
    }

    /// Take the foreground back after a pinned paste.
    pub fn reclaim(&self) {
        bring_to_foreground(self.hwnd);
    }

    pub fn hide(&self) {
        // SAFETY: Hiding a live window owned by this thread.
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }
}

impl HasWindowHandle for NativePanel {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = NonZeroIsize::new(self.hwnd.0 as isize).ok_or(HandleError::Unavailable)?;
        // SAFETY: The caller keeps the GPUI window alive while the borrowed handle is used.
        Ok(unsafe { WindowHandle::borrow_raw(Win32WindowHandle::new(hwnd).into()) })
    }
}

/// The external top-level window that was in the foreground when the panel was shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasteTarget(HWND);

impl PasteTarget {
    pub fn capture() -> Option<Self> {
        // SAFETY: Reading the foreground window has no preconditions.
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.is_invalid() && !owned_by_self(hwnd)).then_some(Self(hwnd))
    }

    /// The activating panel holds the foreground while shown, so the target only has to
    /// survive and nothing outside Neo may have taken the foreground meanwhile.
    pub fn is_available(&self) -> bool {
        // SAFETY: IsWindow and GetForegroundWindow accept stale handles.
        unsafe {
            if !IsWindow(Some(self.0)).as_bool() {
                return false;
            }
            let foreground = GetForegroundWindow();
            foreground == self.0 || owned_by_self(foreground)
        }
    }

    pub fn is_foreground(&self) -> bool {
        // SAFETY: Reading the foreground window has no preconditions.
        unsafe { GetForegroundWindow() == self.0 }
    }
}

fn owned_by_self(hwnd: HWND) -> bool {
    let mut process = 0;
    // SAFETY: A stale handle yields thread 0 and leaves the process ID untouched.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
    process == std::process::id()
}

/// A hotkey or tray click does not always grant foreground rights; attaching to the current
/// foreground thread's input queue is the documented way to make the switch stick.
fn bring_to_foreground(hwnd: HWND) {
    // SAFETY: Plain Win32 calls; the input queues are detached again before returning.
    unsafe {
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            return;
        }
        let current = GetCurrentThreadId();
        let foreground = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let attached = foreground != 0
            && foreground != current
            && AttachThreadInput(current, foreground, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        if attached {
            let _ = AttachThreadInput(current, foreground, false);
        }
    }
}

fn work_area(monitor: HMONITOR) -> Option<RECT> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a properly sized MONITORINFO.
    unsafe { GetMonitorInfoW(monitor, &mut info) }
        .as_bool()
        .then_some(info.rcWork)
}

fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a valid output buffer.
    unsafe { GetWindowRect(hwnd, &mut rect) }
        .ok()
        .map(|()| rect)
}
