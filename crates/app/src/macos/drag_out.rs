use std::{path::PathBuf, sync::mpsc};

use anyhow::{Context, Result, ensure};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSEventType, NSImage, NSImageNameMultipleDocuments,
};
use objc2_foundation::{NSMutableArray, NSPoint, NSRect, NSSize, NSString, NSURL};

use super::native::Panel;

struct SourceIvars {
    ended: mpsc::Sender<()>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "EcoPasteNeoFileDragSource"]
    #[ivars = SourceIvars]
    struct Source;

    unsafe impl NSObjectProtocol for Source {}
    unsafe impl NSDraggingSource for Source {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        unsafe fn operations(
            &self,
            _: &NSDraggingSession,
            _: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }
        #[unsafe(method(ignoreModifierKeysForDraggingSession:))]
        unsafe fn ignore_modifiers(&self, _: &NSDraggingSession) -> bool {
            true
        }
        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        unsafe fn ended(&self, _: &NSDraggingSession, _: NSPoint, _: NSDragOperation) {
            let _ = self.ivars().ended.send(());
        }
    }
);

/// The real mouse-down event survives asynchronous path preparation on the main thread.
#[derive(Clone)]
pub struct PressEvent(Retained<NSEvent>);

impl PressEvent {
    pub fn capture() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let event = NSApplication::sharedApplication(mtm).currentEvent()?;
        (event.r#type() == NSEventType::LeftMouseDown).then_some(Self(event))
    }
}

pub fn left_button_down() -> bool {
    NSEvent::pressedMouseButtons() & 1 != 0
}

/// Retain both source and session until AppKit's completion callback has returned.
pub struct NativeDrag {
    _source: Retained<Source>,
    _session: Retained<NSDraggingSession>,
    ended: mpsc::Receiver<()>,
}

impl NativeDrag {
    /// Start outside a GPUI update; AppKit may dispatch nested mouse/key events here.
    /// Only file URL writers are used. The general system pasteboard is untouched.
    pub fn start(panel: &Panel, paths: &[PathBuf], event: &PressEvent) -> Result<Self> {
        let mtm =
            MainThreadMarker::new().context("Native file dragging requires the main thread")?;
        ensure!(
            !paths.is_empty() && left_button_down(),
            "File drag has no active mouse gesture"
        );
        let view = panel.content_view();
        let origin = view.convertPoint_fromView(event.0.locationInWindow(), None);
        // SAFETY: AppKit exports this immutable built-in image name.
        let icon = NSImage::imageNamed(unsafe { NSImageNameMultipleDocuments })
            .unwrap_or_else(|| NSImage::initWithSize(NSImage::alloc(), NSSize::new(64.0, 64.0)));
        let frame = NSRect::new(
            NSPoint::new(origin.x - 32.0, origin.y - 32.0),
            NSSize::new(64.0, 64.0),
        );
        let items = NSMutableArray::new();
        for path in paths {
            let url = NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(&path.to_string_lossy()),
                false,
            );
            let item = NSDraggingItem::initWithPasteboardWriter(
                NSDraggingItem::alloc(),
                &ProtocolObject::from_retained(url),
            );
            unsafe {
                item.setDraggingFrame_contents(frame, Some(&icon));
            }
            items.addObject(&*item);
        }
        let (send, ended) = mpsc::channel();
        let source = Source::alloc(mtm).set_ivars(SourceIvars { ended: send });
        // SAFETY: NSObject initializes the allocated main-thread subclass and its installed ivars.
        let source: Retained<Source> = unsafe { msg_send![super(source), init] };
        let session = view.beginDraggingSessionWithItems_event_source(
            &items,
            &event.0,
            ProtocolObject::from_ref(&*source),
        );
        session.setAnimatesToStartingPositionsOnCancelOrFail(true);
        Ok(Self {
            _source: source,
            _session: session,
            ended,
        })
    }

    pub fn finished(&self) -> bool {
        self.ended.try_recv().is_ok()
    }
}
