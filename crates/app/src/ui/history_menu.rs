use ecopaste_neo_app::locale::Locale;
use ecopaste_neo_core::db::models::ClipboardItem;
use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};

#[derive(Clone, Copy)]
pub enum MenuAction {
    Favorite,
    Pin,
    EditNote,
    ClearNote,
    Delete,
}

pub struct MenuSelection {
    pub id: String,
    pub label: String,
    pub note: Option<String>,
    pub action: MenuAction,
}

/// Retain the native menu through event delivery and bind it to one immutable record ID.
pub struct HistoryMenu {
    pub menu: Menu,
    id: String,
    label: String,
    note: Option<String>,
    entries: Vec<(MenuItem, MenuAction)>,
    handled: bool,
}

impl HistoryMenu {
    pub fn new(item: &ClipboardItem, locale: Locale) -> anyhow::Result<Self> {
        let entries = vec![
            (
                MenuItem::new(locale.favorite_action(item.is_favorite), true, None),
                MenuAction::Favorite,
            ),
            (
                MenuItem::new(locale.pin_action(item.is_pinned), true, None),
                MenuAction::Pin,
            ),
            (
                MenuItem::new(locale.edit_note(), true, None),
                MenuAction::EditNote,
            ),
            (
                MenuItem::new(
                    locale.clear_note(),
                    item.note.as_ref().is_some_and(|note| !note.is_empty()),
                    None,
                ),
                MenuAction::ClearNote,
            ),
            (
                MenuItem::new(locale.delete_record(), true, None),
                MenuAction::Delete,
            ),
        ];
        let menu = Menu::new();
        for (index, (item, _)) in entries.iter().enumerate() {
            if index == 4 {
                menu.append(&PredefinedMenuItem::separator())?;
            }
            menu.append(item)?;
        }
        Ok(Self {
            menu,
            id: item.id.clone(),
            label: if item.is_sensitive {
                locale.sensitive_hidden().into()
            } else {
                item.summary
                    .as_deref()
                    .unwrap_or_default()
                    .chars()
                    .take(120)
                    .collect()
            },
            note: item.note.clone(),
            entries,
            handled: false,
        })
    }

    /// A delayed or duplicate native event can never resolve against the keyboard selection.
    pub fn take_action(&mut self, id: &MenuId) -> Option<MenuSelection> {
        if self.handled {
            return None;
        }
        let action = self.entries.iter().find(|(item, _)| item.id() == id)?.1;
        self.handled = true;
        Some(MenuSelection {
            id: self.id.clone(),
            label: self.label.clone(),
            note: self.note.clone(),
            action,
        })
    }

    /// Revoke queued events before cancelling tracking, retaining the native menu until replacement.
    pub fn cancel(&mut self) {
        self.handled = true;
        crate::platform::cancel_menu(&self.menu);
    }
}
