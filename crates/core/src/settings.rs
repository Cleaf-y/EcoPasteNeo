//! Neo preferences belong only to the paths validated by Storage.

use std::{fs, io::Write, path::PathBuf, sync::RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    clipboard::{
        settings::{Capture, CaptureKind, Filters, Sensitive},
        CaptureSettings,
    },
    paths::DataPaths,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub capture_enabled: bool,
    pub capture: Capture,
    pub sensitive: Sensitive,
    pub filters: Filters,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            capture_enabled: true,
            capture: Capture::default(),
            sensitive: Sensitive::default(),
            filters: Filters::default(),
        }
    }
}

/// A window sends only its edited field, never an old complete snapshot.
#[derive(Debug, Clone)]
pub enum SettingsChange {
    CaptureEnabled(bool),
    CollectSecrets(bool),
    RedactSecrets(bool),
    CaptureType(CaptureKind, bool),
    AddExcludedApp(String),
    RemoveExcludedApp(String),
    ClearExcludedApps,
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("{0}")]
    Unavailable(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("settings must be a JSON object")]
    InvalidObject,
    #[error("application identifier must not be empty or contain whitespace")]
    InvalidAppId,
}

struct Loaded {
    document: Value,
    settings: Settings,
}

/// Serializes field updates and publishes a snapshot only after atomic persistence.
/// A failed initial read leaves capture disabled until an explicit successful reload.
pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Result<Loaded, String>>,
}

impl SettingsStore {
    /// Load the current Neo file or save first-run defaults; retain any error as paused state.
    pub fn open(paths: &DataPaths) -> Self {
        let path = paths.settings_path();
        let current = Self::load(&path).map_err(|error| error.to_string());
        Self {
            path,
            current: RwLock::new(current),
        }
    }

    pub fn snapshot(&self) -> Result<Settings, SettingsError> {
        let current = self.current.read().expect("settings poisoned");
        current
            .as_ref()
            .map(|loaded| loaded.settings.clone())
            .map_err(|error| SettingsError::Unavailable(error.clone()))
    }

    /// Called for each native notification; no filesystem access or listener restart.
    pub fn capture_settings(&self) -> Option<CaptureSettings> {
        let current = self.current.read().expect("settings poisoned");
        let settings = &current.as_ref().ok()?.settings;
        if !settings.capture_enabled {
            return None;
        }
        Some(CaptureSettings {
            capture: settings.capture.clone(),
            sensitive: settings.sensitive,
            excluded_app_ids: settings.filters.excluded_app_ids.clone(),
            plain_only: false,
        })
    }

    /// Retry a failed read after the user repairs the file; never overwrite bad JSON.
    pub fn reload(&self) -> Result<Settings, SettingsError> {
        let mut current = self.current.write().expect("settings poisoned");
        match Self::load(&self.path) {
            Ok(loaded) => {
                let settings = loaded.settings.clone();
                *current = Ok(loaded);
                Ok(settings)
            }
            Err(error) => {
                *current = Err(error.to_string());
                Err(error)
            }
        }
    }

    /// Merge only the requested leaf into the latest document, preserving other rules.
    pub fn update(&self, change: SettingsChange) -> Result<Settings, SettingsError> {
        let mut current = self.current.write().expect("settings poisoned");
        let loaded = current
            .as_ref()
            .map_err(|error| SettingsError::Unavailable(error.clone()))?;
        let mut document = loaded.document.clone();
        match change {
            SettingsChange::CaptureEnabled(value) => document["captureEnabled"] = value.into(),
            SettingsChange::CaptureType(kind, value) => {
                let key = match kind {
                    CaptureKind::Text => "text",
                    CaptureKind::Html => "html",
                    CaptureKind::Rtf => "rtf",
                    CaptureKind::Image => "image",
                    CaptureKind::Files => "files",
                };
                if document.get("capture").is_none() {
                    document["capture"] = serde_json::json!({});
                }
                document["capture"][key] = value.into();
            }
            change @ (SettingsChange::CollectSecrets(_) | SettingsChange::RedactSecrets(_)) => {
                let (key, value) = match change {
                    SettingsChange::CollectSecrets(value) => ("collectSecrets", value),
                    SettingsChange::RedactSecrets(value) => ("redactSecrets", value),
                    _ => unreachable!(),
                };
                if document.get("sensitive").is_none() {
                    document["sensitive"] = serde_json::json!({});
                }
                document["sensitive"][key] = value.into();
            }
            change @ (SettingsChange::AddExcludedApp(_)
            | SettingsChange::RemoveExcludedApp(_)
            | SettingsChange::ClearExcludedApps) => {
                let mut ids = loaded.settings.filters.excluded_app_ids.clone();
                match change {
                    SettingsChange::AddExcludedApp(id) => {
                        let id = id.trim();
                        if id.is_empty() || id.chars().any(char::is_whitespace) {
                            return Err(SettingsError::InvalidAppId);
                        }
                        if !ids.iter().any(|existing| existing == id) {
                            ids.push(id.to_owned());
                        }
                    }
                    SettingsChange::RemoveExcludedApp(id) => ids.retain(|existing| existing != &id),
                    SettingsChange::ClearExcludedApps => ids.clear(),
                    _ => unreachable!(),
                }
                if document.get("filters").is_none() {
                    document["filters"] = serde_json::json!({});
                }
                document["filters"]["excludedAppIds"] = serde_json::to_value(ids)?;
            }
        }
        let next = Self::parse(document)?;
        Self::write(&self.path, &next.document)?;
        let settings = next.settings.clone();
        *current = Ok(next);
        Ok(settings)
    }

    fn parse(document: Value) -> Result<Loaded, SettingsError> {
        if !document.is_object() {
            return Err(SettingsError::InvalidObject);
        }
        let settings = serde_json::from_value(document.clone())?;
        Ok(Loaded { document, settings })
    }

    /// Existing valid files stay byte-for-byte intact; only absence permits initialization.
    fn load(path: &std::path::Path) -> Result<Loaded, SettingsError> {
        match fs::read(path) {
            Ok(bytes) => Self::parse(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A dangling link is an existing, unreadable configuration, not first run.
                match fs::symlink_metadata(path) {
                    Ok(_) => return Err(error.into()),
                    Err(metadata) if metadata.kind() == std::io::ErrorKind::NotFound => {}
                    Err(metadata) => return Err(metadata.into()),
                }
                let loaded = Self::parse(serde_json::to_value(Settings::default())?)?;
                Self::write(path, &loaded.document)?;
                Ok(loaded)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// A unique file in the same directory is flushed before atomic replacement.
    fn write(path: &std::path::Path, document: &Value) -> Result<(), SettingsError> {
        let parent = path
            .parent()
            .expect("settings must have a parent directory");
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temporary, document)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}
