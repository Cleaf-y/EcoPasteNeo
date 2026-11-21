use super::*;
use crate::platform::{NativeDrag, PressEvent};
use ecopaste_neo_app::drag::{DragGesture, DragRequest};
use gpui::{MouseDownEvent, MouseMoveEvent};

#[derive(Default)]
pub(super) struct FileDrag {
    pub gesture: DragGesture,
    event: Option<PressEvent>,
    prepared: Option<(DragRequest, anyhow::Result<Vec<std::path::PathBuf>>)>,
    preparation: Option<Task<()>>,
    launch: Option<Task<()>>,
    native: Option<NativeDrag>,
}

impl FileDrag {
    pub fn release(&mut self) {
        self.gesture.release();
        self.prepared = None;
        self.event = None;
        self.preparation = None;
    }
}

impl HistoryPanel {
    /// Begin preparation by ID on mouse-down, without consuming ordinary clicks.
    pub(super) fn press_file_row(
        &mut self,
        id: &str,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if self.pasting || self.management.blocks_list() || self.file_drag.gesture.active() {
            return;
        }
        self.file_drag.release();
        let request = self
            .file_drag
            .gesture
            .begin(f32::from(event.position.x), f32::from(event.position.y));
        let supported = self
            .state
            .rows
            .iter()
            .find(|row| row.item.id == id)
            .is_some_and(|row| {
                matches!(row.item.kind, ClipboardKind::Files | ClipboardKind::Image)
            });
        if !supported {
            self.file_drag.gesture.release();
            return;
        }
        self.file_drag.event = PressEvent::capture();
        let pending = self.source.prepare_drag(id.to_owned());
        self.file_drag.preparation = Some(cx.spawn(async move |this, cx| {
            let result = pending.await.unwrap_or_else(|error| Err(error.into()));
            let _ = this.update(cx, |this, cx| {
                if !this.file_drag.gesture.accepts(request) {
                    return;
                }
                this.file_drag.prepared = Some((request, result));
                this.try_start_file_drag(cx);
            });
        }));
    }

    /// Window-level motion also follows the original press outside the card's hitbox.
    pub(super) fn move_file_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !event.dragging() {
            self.file_drag.release();
            return;
        }
        self.file_drag
            .gesture
            .move_pointer(f32::from(event.position.x), f32::from(event.position.y));
        self.try_start_file_drag(cx);
    }

    fn try_start_file_drag(&mut self, cx: &mut Context<Self>) {
        let Some((request, _)) = &self.file_drag.prepared else {
            return;
        };
        if !self.file_drag.gesture.ready(*request) {
            return;
        }
        if !self.visible
            || self.pasting
            || self.management.blocks_list()
            || !platform::left_button_down()
        {
            self.file_drag.release();
            return;
        }
        let (request, prepared) = self.file_drag.prepared.take().unwrap();
        let paths = match prepared {
            Ok(paths) => paths,
            Err(error) => {
                self.action_error = Some(self.locale.drag_error(error.downcast_ref()));
                self.file_drag.release();
                cx.notify();
                return;
            }
        };
        let Some(event) = self.file_drag.event.take() else {
            self.action_error = Some(self.locale.drag_error(None));
            self.file_drag.release();
            cx.notify();
            return;
        };
        if !self.file_drag.gesture.start(request) {
            return;
        }
        self.close_preview(false, cx);
        self.action_error = None;
        let panel = self.panel;
        let session = self.interaction;
        self.file_drag.launch = Some(cx.spawn(async move |this, cx| {
            let current = this
                .update(cx, |this, _| this.visible && this.interaction == session)
                .unwrap_or(false);
            if !current {
                let _ = this.update(cx, |this, _| this.file_drag.gesture.finish());
                return;
            }
            let native = NativeDrag::start(&panel, &paths, &event);
            let _ = this.update(cx, |this, cx| {
                match native {
                    Ok(native) => this.file_drag.native = Some(native),
                    Err(error) => {
                        eprintln!("Could not start native file drag: {error:#}");
                        this.file_drag.gesture.finish();
                        this.action_error = Some(this.locale.drag_error(None));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Completion only clears tracking. Finder receipt is verified independently.
    pub(in crate::ui) fn poll_drag(&mut self, cx: &mut Context<Self>) {
        if self
            .file_drag
            .native
            .as_ref()
            .is_some_and(|drag| drag.finished())
        {
            self.file_drag.native = None;
            self.file_drag.gesture.finish();
            self.file_drag.release();
            self.check_panel_focus(cx);
            cx.notify();
        } else if !self.file_drag.gesture.active() && !platform::left_button_down() {
            self.file_drag.release();
        }
    }
}
