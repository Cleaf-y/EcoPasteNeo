use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use drag::{DragItem, DragMode, Image, Options};
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
    WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON},
};

use super::panel::NativePanel;

/// OLE drag starts from the live button state, so no native event has to be retained.
#[derive(Clone)]
pub struct PressEvent;

impl PressEvent {
    pub fn capture() -> Option<Self> {
        left_button_down().then_some(Self)
    }
}

/// GetAsyncKeyState reports physical buttons; follow the user's primary-button setting.
pub fn left_button_down() -> bool {
    // SAFETY: Both calls only read input state.
    unsafe {
        let primary = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        GetAsyncKeyState(i32::from(primary.0)) < 0
    }
}

/// `DoDragDrop` runs a modal loop and returns only after a drop or cancellation.
pub struct NativeDrag;

impl NativeDrag {
    /// Start outside a GPUI update; the modal OLE loop dispatches nested window messages.
    /// Only CF_HDROP file paths are offered, as a copy. The system clipboard is untouched.
    pub fn start(panel: &NativePanel, paths: &[PathBuf], _event: &PressEvent) -> Result<Self> {
        ensure!(
            !paths.is_empty() && left_button_down(),
            "File drag has no active mouse gesture"
        );
        drag::start_drag(
            panel,
            DragItem::Files(paths.to_vec()),
            // Decoding a large original for the drag image would stall the gesture.
            Image::Raw(Vec::new()),
            |_, _| {},
            Options {
                mode: DragMode::Copy,
                ..Options::default()
            },
        )
        .context("Could not start the OLE file drag")?;
        Ok(Self)
    }

    /// The drag already ended when `start` returned.
    pub fn finished(&self) -> bool {
        true
    }
}
