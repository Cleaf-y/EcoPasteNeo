//! Explicit macOS shortcut-conflict fixture. No keyboard injection or clipboard access.
#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
    use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};
    use std::{
        io,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    let key: HotKey = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Pass a shortcut, e.g. Control+Alt+Super+KeyN"))?
        .parse()?;
    let marker = objc2::MainThreadMarker::new().expect("main thread");
    let _application = objc2_app_kit::NSApplication::sharedApplication(marker);
    let manager = GlobalHotKeyManager::new()?;
    manager.register(key)?;
    if std::env::args().any(|arg| arg == "--probe") {
        manager.unregister(key)?;
        println!("Combination is available: {key}");
        return Ok(());
    }
    println!("Reserved {key}; press Enter to release and exit.");
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
        signal.store(true, Ordering::Release);
    });
    while !stop.load(Ordering::Acquire) {
        unsafe {
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_millis(100), false);
        }
    }
    manager.unregister(key)?;
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This fixture requires macOS.");
}
