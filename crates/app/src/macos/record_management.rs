use super::*;
use crate::macos::history_menu::{HistoryMenu, MenuAction, MenuSelection};
use ecopaste_neo_app::history::RecordAction;
use gpui::{MouseDownEvent, Point};
use tray_icon::menu::MenuId;

#[derive(Default)]
pub(super) struct Management {
    menu: Option<HistoryMenu>,
    menu_open: bool,
    restore_focus: bool,
    pending: Option<MenuSelection>,
    menu_task: Option<Task<()>>,
    dialog: Option<RecordDialog>,
    mutating: bool,
    mutation: Option<Task<()>>,
}

struct RecordDialog {
    id: String,
    label: String,
    note: Option<Entity<InputState>>,
}

impl Management {
    pub fn blocks_list(&self) -> bool {
        self.menu_open || self.dialog.is_some() || self.mutating
    }

    pub fn tracks_menu(&self) -> bool {
        self.menu_open || self.restore_focus
    }

    /// A hidden panel revokes pending dialogs without cancelling an already-started write.
    pub fn close(&mut self) {
        if let Some(menu) = &mut self.menu {
            menu.cancel();
        }
        self.pending = None;
        self.restore_focus = false;
        self.dialog = None;
    }
}

impl HistoryPanel {
    /// Snapshot the right-clicked record before native tracking or any list refresh.
    pub(super) fn popup_history_menu(
        &mut self,
        id: &str,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.visible
            || self.pasting
            || self.management.blocks_list()
            || self.file_drag.gesture.busy()
        {
            return;
        }
        self.file_drag.release();
        let Some(row) = self.state.rows.iter().find(|row| row.item.id == id) else {
            return;
        };
        let menu = match HistoryMenu::new(&row.item, self.locale) {
            Ok(menu) => menu,
            Err(error) => {
                eprintln!("Could not create history context menu: {error:#}");
                self.action_error = Some(self.locale.management_failed().into());
                cx.notify();
                return;
            }
        };
        let native_menu = menu.menu.clone();
        self.management.menu = Some(menu);
        self.management.menu_open = true;
        self.management.pending = None;
        let panel = self.panel;
        let session = self.interaction;
        let position: Point<gpui::Pixels> = event.position;
        self.management.menu_task = Some(cx.spawn(async move |this, cx| {
            // Native tracking runs with no GPUI App/entity borrow held across its nested loop.
            let current = this
                .update(cx, |this, _| this.visible && this.interaction == session)
                .unwrap_or(false);
            if current {
                panel.popup_menu(&native_menu, position);
            }
            let _ = this.update(cx, |this, cx| {
                this.management.menu_open = false;
                this.management.restore_focus = this.visible && this.interaction == session;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// One receiver routes tray and context events; menu item handles identify their saved ID.
    pub(in crate::macos) fn handle_menu_event(
        &mut self,
        id: &MenuId,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.visible {
            return;
        }
        if let Some(selection) = self
            .management
            .menu
            .as_mut()
            .and_then(|menu| menu.take_action(id))
        {
            self.management.pending = Some(selection);
            cx.notify();
        }
    }

    /// Wait until tracking has ended before opening an editor or starting a mutation.
    pub(in crate::macos) fn poll_management(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.management.menu_open || !self.visible {
            return;
        }
        if std::mem::take(&mut self.management.restore_focus) {
            if native::frontmost_pid() == self.target {
                self.panel.restore_key();
                self.input.update(cx, |input, cx| input.focus(window, cx));
            } else {
                self.check_panel_focus(cx);
            }
        }
        let Some(selection) = self.management.pending.take() else {
            return;
        };
        match selection.action {
            MenuAction::Favorite => {
                self.manage_record(selection.id, RecordAction::ToggleFavorite, cx)
            }
            MenuAction::Pin => self.manage_record(selection.id, RecordAction::TogglePinned, cx),
            MenuAction::ClearNote => self.manage_record(selection.id, RecordAction::Note(None), cx),
            MenuAction::EditNote => {
                self.close_preview(false, cx);
                let note = cx.new(|cx| InputState::new(window, cx));
                note.update(cx, |input, cx| {
                    input.set_value(selection.note.unwrap_or_default(), window, cx);
                    input.focus(window, cx);
                });
                self.management.dialog = Some(RecordDialog {
                    id: selection.id,
                    label: selection.label,
                    note: Some(note),
                });
                self.panel.restore_key();
            }
            MenuAction::Delete => {
                self.close_preview(false, cx);
                self.management.dialog = Some(RecordDialog {
                    id: selection.id,
                    label: selection.label,
                    note: None,
                });
                self.panel.restore_key();
            }
        }
        cx.notify();
    }

    fn close_record_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.management.mutating {
            return;
        }
        self.management.dialog = None;
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Dialog submission uses its stored ID, even if capture has reordered the list.
    fn submit_record_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &self.management.dialog else {
            return;
        };
        let action = if let Some(input) = &dialog.note {
            let value = input.read(cx).value().to_string();
            RecordAction::Note((!value.is_empty()).then_some(value))
        } else {
            RecordAction::Delete
        };
        self.manage_record(dialog.id.clone(), action, cx);
    }

    /// Refresh from SQL after commit so filters, ordering, counts and preview agree.
    fn manage_record(&mut self, id: String, action: RecordAction, cx: &mut Context<Self>) {
        if self.management.mutating {
            return;
        }
        self.close_preview(false, cx);
        self.management.mutating = true;
        self.action_error = None;
        let pending = self.source.manage(id, action);
        self.management.mutation = Some(cx.spawn(async move |this, cx| {
            let result = pending.await;
            let _ = this.update(cx, |this, cx| {
                this.management.mutating = false;
                match result {
                    Ok(Ok(cleaned)) => {
                        this.management.dialog = None;
                        this.management.restore_focus = this.visible;
                        if !cleaned {
                            this.action_error = Some(this.locale.image_cleanup_failed().into());
                        }
                        this.refresh(cx);
                    }
                    error => {
                        eprintln!("History management failed: {error:?}");
                        this.action_error = Some(this.locale.management_failed().into());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The editor owns Enter/Escape, including marked-text handling; neither reaches paste.
    pub(super) fn management_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.management.blocks_list() {
            return false;
        }
        if let Some(note) = self
            .management
            .dialog
            .as_ref()
            .and_then(|dialog| dialog.note.clone())
            && note.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            return true;
        }
        match event.keystroke.key.as_str() {
            "enter" => {
                self.submit_record_dialog(cx);
                cx.stop_propagation();
            }
            "escape" => {
                self.close_record_dialog(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
        true
    }

    pub(super) fn render_record_dialog(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let dialog = self.management.dialog.as_ref()?;
        let locale = self.locale;
        let theme = cx.theme();
        let mut body =
            div()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .child(
                    div()
                        .text_lg()
                        .font_semibold()
                        .child(if dialog.note.is_some() {
                            locale.edit_note()
                        } else {
                            locale.delete_record()
                        }),
                );
        if let Some(input) = &dialog.note {
            body = body.child(Input::new(input).disabled(self.management.mutating));
        } else {
            body = body.child(locale.delete_confirmation());
        }
        body = body.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(dialog.label.clone()),
        );
        if self.management.mutating {
            return Some(body.child(locale.loading()).into_any_element());
        }
        Some(
            body.child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .id("cancel-record-dialog")
                            .cursor_pointer()
                            .child(locale.cancel())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_record_dialog(window, cx)
                            })),
                    )
                    .child(
                        div()
                            .id("submit-record-dialog")
                            .cursor_pointer()
                            .text_color(if dialog.note.is_some() {
                                theme.primary
                            } else {
                                theme.danger
                            })
                            .child(if dialog.note.is_some() {
                                locale.save()
                            } else {
                                locale.delete_record()
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.submit_record_dialog(cx))),
                    ),
            )
            .into_any_element(),
        )
    }
}
