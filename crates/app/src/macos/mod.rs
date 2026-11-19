mod drag_out;
mod history_menu;
mod login;
mod native;
mod panel;
mod paste;
mod preferences;
mod record_preview;
mod shortcut;
mod system;

use std::{cell::RefCell, rc::Rc, time::Duration};

use anyhow::Result;
use ecopaste_neo_app::{history::HistorySource, options::Options};
use gpui::{
    App, AppContext, Bounds, WindowBounds, WindowKind, WindowOptions, WindowingRequest, px, size,
};
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
    // Set accessory policy before AppKit launches; the graphical backend stays available.
    let application = gpui_platform::application().with_windowing(WindowingRequest::Headless);
    application.run(move |cx: &mut App| {
        let login_startup = login::launched_at_login();
        gpui_component::init(cx);
        let integration = match SystemIntegration::new(
            options.locale,
            source
                .settings()
                .snapshot()
                .ok()
                .as_ref()
                .map(|settings| &settings.panel_shortcut),
        ) {
            Ok(integration) => integration,
            Err(error) => {
                eprintln!("EcoPasteNeo system integration: {error:#}");
                cx.quit();
                return;
            }
        };
        let integration = Rc::new(RefCell::new(integration));
        let bounds = Bounds::centered(None, size(px(620.0), px(700.0)), cx);
        let mut settings = preferences::SettingsWindow::new(
            source.clone(),
            options.locale,
            integration.clone(),
            cx,
        );
        let settings_unavailable =
            source.settings().snapshot().is_err() || integration.borrow().error().is_some();
        let startup = options.startup_windows(login_startup, settings_unavailable);
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
                    if startup.panel {
                        view.update(cx, |view, cx| view.show(window, cx));
                    }
                    panel = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("Cannot create EcoPasteNeo panel");
        let panel = panel.expect("Panel view must exist");
        let settings_panel = panel.downgrade();
        settings.on_saved(
            move |cx| {
                let _ = settings_panel.update(cx, |view, cx| view.settings_saved(cx));
            },
            cx,
        );
        if startup.settings
            && let Err(error) = settings.show(cx)
        {
            eprintln!("Could not show settings: {error:#}");
        }
        let executor = cx.background_executor().clone();
        cx.spawn(async move |cx| {
            loop {
                executor.timer(Duration::from_millis(25)).await;
                cx.update(|cx| settings.poll_shortcut(cx));
                let _ = handle.update(cx, |_, window, cx| {
                    panel.update(cx, |view, cx| {
                        view.poll_capture(cx);
                        view.poll_management(window, cx);
                        view.poll_drag(cx);
                    });
                });
                loop {
                    let action = integration.borrow_mut().next_action();
                    let Some(action) = action else {
                        break;
                    };
                    if matches!(action, SystemAction::Quit) {
                        cx.update(|cx| cx.quit());
                        return;
                    }
                    if matches!(action, SystemAction::Settings) {
                        cx.update(|cx| {
                            if let Err(error) = settings.show(cx) {
                                eprintln!("Could not show settings: {error:#}");
                            }
                        });
                        continue;
                    }
                    if handle
                        .update(cx, |_, window, cx| {
                            panel.update(cx, |view, cx| match action {
                                SystemAction::Toggle if view.visible() => view.hide(cx),
                                SystemAction::Toggle | SystemAction::Show => view.show(window, cx),
                                SystemAction::Quit | SystemAction::Settings => unreachable!(),
                                SystemAction::Menu(id) => view.handle_menu_event(&id, window, cx),
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
