//! Native handles live on one OS thread; consumers receive owned prepared data.

use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use chrono::Utc;
use clipboard_rs::{ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext, WatcherShutdown};

pub use super::read::ClipboardReader;
use super::{prepare_capture, CaptureError, CaptureSettings, ImageStore, PreparedCapture, Result};
use crate::db::models::ClipboardApp;
use crate::writeback::WritebackGuard;

pub const POLL_INTERVAL: Duration = Duration::from_millis(120);
pub const READ_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(15),
    Duration::from_millis(35),
    Duration::from_millis(75),
];

/// Retry only read errors, never a successful empty or non-empty read.
pub fn read_with_retry<T, E>(
    retry_delays: &[Duration],
    mut read: impl FnMut() -> std::result::Result<Option<T>, E>,
) -> std::result::Result<Option<T>, E> {
    let mut result = read();
    for delay in retry_delays {
        if result.is_ok() {
            return result;
        }
        thread::sleep(*delay);
        result = read();
    }
    result
}

/// Resolve the source before reading the pasteboard; nothing native crosses an await.
fn read_prepared(
    reader: &ClipboardReader,
    store: &ImageStore,
    settings: &CaptureSettings,
    guard: Option<&WritebackGuard>,
) -> Result<Option<PreparedCapture>> {
    let source = super::source::detect_frontmost().map(|source| {
        let now = Utc::now();
        ClipboardApp {
            id: source.id,
            name: source.name,
            platform: source.platform,
            icon_file: None,
            created_at: now,
            updated_at: now,
        }
    });
    if !settings.accepts_source(source.as_ref()) {
        return Ok(None);
    }
    let Some(payload) = read_with_retry(&READ_RETRY_DELAYS, || {
        reader.read_with_capture(&settings.capture)
    })?
    else {
        return Ok(None);
    };
    prepare_capture(store, &payload, settings, source, guard)
}

/// Manual reads and listener callbacks use the same preparation path.
/// This only reads after an explicit application action; creating core starts no listener.
pub fn read_capture(
    store: &ImageStore,
    settings: &CaptureSettings,
) -> Result<Option<PreparedCapture>> {
    let reader = ClipboardReader::new()?;
    read_prepared(&reader, store, settings, None)
}

/// Dropping the handle stops and joins the listener, releasing its OS resources.
pub struct WatcherHandle {
    shutdown: Option<WatcherShutdown>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        self.shutdown.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Handler<S, F> {
    reader: ClipboardReader,
    store: ImageStore,
    guard: Arc<WritebackGuard>,
    settings: S,
    deliver: F,
}

impl<S, F> ClipboardHandler for Handler<S, F>
where
    S: FnMut() -> Option<CaptureSettings>,
    F: FnMut(Result<PreparedCapture>),
{
    fn on_clipboard_change(&mut self) {
        // None pauses capture before probing the source or reading native data.
        let Some(settings) = (self.settings)() else {
            return;
        };
        match read_prepared(&self.reader, &self.store, &settings, Some(&self.guard)) {
            Ok(Some(prepared)) => (self.deliver)(Ok(prepared)),
            Ok(None) => {}
            Err(error) => (self.deliver)(Err(error)),
        }
    }
}

/// Start native capture without depending on a UI runtime or caching a database pool.
/// The receiver queues owned captures, fetches the current pool, calls `persist_capture`,
/// then refreshes the panel from SQLite. A None settings snapshot pauses capture.
/// The sink must enqueue quickly rather than block the native listener on database work.
pub fn start_watcher<S, F>(
    store: ImageStore,
    guard: Arc<WritebackGuard>,
    settings: S,
    deliver: F,
) -> Result<WatcherHandle>
where
    S: FnMut() -> Option<CaptureSettings> + Send + 'static,
    F: FnMut(Result<PreparedCapture>) + Send + 'static,
{
    let (ready, started) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("neo-clipboard-watcher".to_owned())
        .spawn(move || {
            let setup = (|| {
                let reader = ClipboardReader::new()?;
                let watcher = ClipboardWatcherContext::new_with_interval(POLL_INTERVAL)
                    .map_err(|error| CaptureError::Clipboard(error.to_string()))?;
                Ok::<_, CaptureError>((reader, watcher))
            })();
            let (reader, mut watcher) = match setup {
                Ok(value) => value,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            watcher.add_handler(Handler {
                reader,
                store,
                guard,
                settings,
                deliver,
            });
            if ready.send(Ok(watcher.get_shutdown_channel())).is_err() {
                return;
            }
            watcher.start_watch();
        })
        .map_err(anyhow::Error::from)?;
    match started.recv() {
        Ok(Ok(shutdown)) => Ok(WatcherHandle {
            shutdown: Some(shutdown),
            thread: Some(thread),
        }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(error) => {
            let _ = thread.join();
            Err(anyhow::Error::from(error).into())
        }
    }
}
