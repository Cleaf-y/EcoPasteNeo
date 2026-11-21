use anyhow::{Result, bail};
use ecopaste_neo_app::locale::Locale;
use ecopaste_neo_core::panel_shortcut::TapModifier;

/// Double-tap activation has no Windows listener yet, so no value of this type can exist.
/// Settings disable the choice through `DOUBLE_TAP_SUPPORTED`; a copied configuration that
/// still selects it fails with an explanation instead of silently using another shortcut.
pub enum ModifierListener {}

impl ModifierListener {
    pub fn new(_modifier: TapModifier, _request: bool, locale: Locale) -> Result<Self> {
        bail!("{}", locale.shortcut_double_unsupported())
    }

    pub fn modifier(&self) -> TapModifier {
        match *self {}
    }

    pub fn available(&self) -> bool {
        match *self {}
    }

    pub fn arm(&self, _enabled: bool) {
        match *self {}
    }

    pub fn take(&self) -> bool {
        match *self {}
    }
}
