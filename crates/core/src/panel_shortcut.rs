//! Persisted panel activation choice. No platform permission or registration here.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TapModifier {
    Control,
    Option,
    Shift,
    Command,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum PanelShortcut {
    Combination { key: String },
    DoubleTap { modifier: TapModifier },
}

/// macOS keeps ⌃⌥⌘V; Windows uses Alt+C, the shortcut EcoPaste users there already know.
impl Default for PanelShortcut {
    fn default() -> Self {
        let key = if cfg!(target_os = "windows") {
            "Alt+KeyC"
        } else {
            "Control+Alt+Super+KeyV"
        };
        Self::Combination { key: key.into() }
    }
}
