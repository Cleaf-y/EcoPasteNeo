mod native;
mod panel;
mod paste;
mod system;

use std::time::Duration;

use anyhow::Result;
use ecopaste_neo_app::{history::HistorySource, options::Options};
use gpui::{App, AppContext, Bounds, WindowBounds, WindowKind, WindowOptions, px, size};
use gpui_component::Root;
use panel::HistoryPanel;
use system::{SystemAction, SystemIntegration};

/// Start the native UI only after storage has been validated and migrated.
pub fn run() -> Result<()> {
    let options = Options::parse(std::env::args().skip(1))?;
    let source = HistorySource::open(&options)?;
    eprintln!(
        "EcoPasteNeo database: {}",
        source.paths().db_path().display()
    );
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        let integration = match SystemIntegration::new(options.locale) {
            Ok(integration) => integration,
            Err(error) => {
                eprintln!("EcoPasteNeo system integration: {error:#}");
                cx.quit();
                return;
            }
        };
        let bounds = Bounds::centered(None, size(px(620.0), px(700.0)), cx);
        let mut panel = None;
        let handle = cx
            .open_window(
                WindowOptions {
                    kind: WindowKind::PopUp,
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title(ecopaste_neo_core::identity::PRODUCT_NAME);
                    let view = cx.new(|cx| {
                        HistoryPanel::new(source, options.locale, options.capture, window, cx)
                    });
                    let weak = view.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        let _ = weak.update(cx, |view, cx| view.hide(cx));
                        false
                    });
                    if options.show {
                        view.update(cx, |view, cx| view.show(window, cx));
                    }
                    panel = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("Cannot create EcoPasteNeo panel");
        let panel = panel.expect("Panel view must exist");
        let executor = cx.background_executor().clone();
        cx.spawn(async move |cx| {
            loop {
                executor.timer(Duration::from_millis(25)).await;
                let _ = handle.update(cx, |_, _, cx| {
                    panel.update(cx, |view, cx| view.poll_capture(cx));
                });
                while let Some(action) = integration.next_action() {
                    if matches!(action, SystemAction::Quit) {
                        cx.update(|cx| cx.quit());
                        return;
                    }
                    if handle
                        .update(cx, |_, window, cx| {
                            panel.update(cx, |view, cx| match action {
                                SystemAction::Toggle if view.visible() => view.hide(cx),
                                SystemAction::Toggle | SystemAction::Show => view.show(window, cx),
                                SystemAction::Quit => unreachable!(),
                            });
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        })
        .detach();
    });
    Ok(())
}
