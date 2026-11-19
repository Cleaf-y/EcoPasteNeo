//! Persisted macOS panel activation choice. No platform permission or registration here.

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

impl Default for PanelShortcut {
    fn default() -> Self {
        Self::Combination {
            key: "Control+Alt+Super+KeyV".into(),
        }
    }
}
