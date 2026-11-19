//! Explicit diagnostic: only report Finder/TextEdit application metadata, never clipboard data.
use ecopaste_neo_core::clipboard::source::detect_frontmost;
use std::time::{Duration, Instant};

fn main() {
    #[cfg(target_os = "macos")]
    let _application = objc2_app_kit::NSApplication::sharedApplication(
        objc2::MainThreadMarker::new().expect("main thread"),
    );
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut previous = None;
    while Instant::now() < deadline {
        if let Some(app) = detect_frontmost()
            && matches!(app.id.as_str(), "com.apple.finder" | "com.apple.TextEdit")
        {
            let entry = (app.id, app.path);
            if previous.as_ref() != Some(&entry) {
                println!(
                    "{}\t{}",
                    entry.0,
                    entry
                        .1
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "<no bundle path>".into())
                );
                previous = Some(entry);
            }
        } else {
            previous = None;
        }
        #[cfg(target_os = "macos")]
        unsafe {
            core_foundation::runloop::CFRunLoop::run_in_mode(
                core_foundation::runloop::kCFRunLoopDefaultMode,
                Duration::from_millis(100),
                false,
            );
        }
        #[cfg(not(target_os = "macos"))]
        std::thread::sleep(Duration::from_millis(100));
    }
}
