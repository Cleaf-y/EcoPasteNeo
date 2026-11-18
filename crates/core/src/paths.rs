//! Tauri-independent storage selection. Callers supply the platform's local data
//! parent (Application Support on macOS, LocalAppData on Windows), without an app
//! identifier. Resolve storage before opening SQLite or settings.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identity::{APPLICATION_ID, CUSTOM_DATA_DIR_NAME, STORAGE_IDENTITY_FILENAME};

const STORAGE_MANIFEST_FILENAME: &str = "storage.json";
const STORAGE_VERSION: u16 = 1;
const DB_DIR: &str = "db";
const RESOURCES_DIR: &str = "resources";
const CONFIG_DIR: &str = "config";
const STATE_DIR: &str = "state";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Dev,
    Prod,
}

impl Environment {
    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Prod => "prod",
        }
    }
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage paths must be absolute: {0}")]
    RelativePath(PathBuf),
    #[error("storage directory is not owned by EcoPasteNeo: {0}")]
    UnrecognizedDirectory(PathBuf),
    #[error(
        "storage metadata does not match the EcoPasteNeo identity, version or environment: {0}"
    )]
    IdentityMismatch(PathBuf),
    #[error("storage metadata must be a regular file: {0}")]
    InvalidMetadataFile(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageIdentity {
    version: u16,
    application_id: String,
    environment: Environment,
}

impl StorageIdentity {
    fn new(environment: Environment) -> Self {
        Self {
            version: STORAGE_VERSION,
            application_id: APPLICATION_ID.to_owned(),
            environment,
        }
    }

    fn validate(&self, environment: Environment, path: &Path) -> Result<()> {
        if self.version != STORAGE_VERSION
            || self.application_id != APPLICATION_ID
            || self.environment != environment
        {
            return Err(StorageError::IdentityMismatch(path.to_owned()));
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageManifest {
    #[serde(flatten)]
    identity: StorageIdentity,
    data_dir: PathBuf,
}

/// Resolves one environment using only caller-provided platform directories.
/// It never reads a legacy EcoPaste manifest or infers a directory from the CWD.
#[derive(Debug, Clone)]
pub struct Storage {
    default_dir: PathBuf,
    environment: Environment,
}

impl Storage {
    /// `platform_data_parent` must exclude the application identifier.
    pub fn new(platform_data_parent: &Path, environment: Environment) -> Result<Self> {
        require_absolute(platform_data_parent)?;

        Ok(Self {
            default_dir: platform_data_parent
                .join(APPLICATION_ID)
                .join(environment.directory_name()),
            environment,
        })
    }

    pub fn default_data_dir(&self) -> &Path {
        &self.default_dir
    }

    /// Opens the saved Neo directory, or initializes a new default directory.
    /// Invalid metadata and missing custom directories are errors, never a silent
    /// fallback to a different database or permission to adopt unmarked files.
    pub fn open(&self) -> Result<DataPaths> {
        initialize_directory(&self.default_dir, self.environment)?;

        let manifest_path = self.default_dir.join(STORAGE_MANIFEST_FILENAME);
        let Some(content) = read_metadata(&manifest_path)? else {
            return DataPaths::prepare(self.default_dir.clone());
        };
        let manifest: StorageManifest = serde_json::from_slice(&content)?;
        manifest
            .identity
            .validate(self.environment, &manifest_path)?;
        require_absolute(&manifest.data_dir)?;
        validate_directory(&manifest.data_dir, self.environment)?;

        DataPaths::prepare(manifest.data_dir)
    }

    /// Selects `<parent>/EcoPasteNeoData/<env>`. Only a new/empty directory or a
    /// matching Neo directory may be selected; existing identity is never rewritten.
    /// Persist the selection only after the target is ready for database use.
    pub fn open_custom(&self, parent: &Path) -> Result<DataPaths> {
        require_absolute(parent)?;
        let target = custom_data_dir(parent, self.environment);
        initialize_directory(&self.default_dir, self.environment)?;
        initialize_directory(&target, self.environment)?;
        let paths = DataPaths::prepare(target.clone())?;
        let manifest = StorageManifest {
            identity: StorageIdentity::new(self.environment),
            data_dir: target,
        };
        let manifest_path = self.default_dir.join(STORAGE_MANIFEST_FILENAME);
        // Reject a linked or non-file selector instead of following it.
        read_metadata(&manifest_path)?;
        write_json(&manifest_path, &manifest, false)?;

        Ok(paths)
    }
}

pub fn custom_data_dir(parent: &Path, environment: Environment) -> PathBuf {
    parent
        .join(CUSTOM_DATA_DIR_NAME)
        .join(environment.directory_name())
}

/// Paths returned only after Neo ownership has been established. Keep this value
/// for the selected database/settings session rather than resolving each file anew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    fn prepare(root: PathBuf) -> Result<Self> {
        for name in [DB_DIR, RESOURCES_DIR, CONFIG_DIR, STATE_DIR] {
            fs::create_dir_all(root.join(name))?;
        }

        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn db_path(&self) -> PathBuf {
        self.root.join(DB_DIR).join("clipboard.db")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.root.join(CONFIG_DIR).join("settings.json")
    }

    pub fn resources_dir(&self) -> PathBuf {
        self.root.join(RESOURCES_DIR)
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join(STATE_DIR)
    }
}

fn require_absolute(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(StorageError::RelativePath(path.to_owned()));
    }

    Ok(())
}

/// Missing metadata is distinct from an unreadable, corrupt or linked file.
fn read_metadata(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(fs::read(path)?)),
        Ok(_) => Err(StorageError::InvalidMetadataFile(path.to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Validate only: reopening a saved location must not create or repair identity.
fn validate_directory(directory: &Path, environment: Environment) -> Result<()> {
    let path = directory.join(STORAGE_IDENTITY_FILENAME);
    let content = read_metadata(&path)?
        .ok_or_else(|| StorageError::UnrecognizedDirectory(directory.to_owned()))?;
    let identity: StorageIdentity = serde_json::from_slice(&content)?;

    identity.validate(environment, &path)
}

/// Check emptiness before writing anything into a previously unowned directory.
fn initialize_directory(directory: &Path, environment: Environment) -> Result<()> {
    let identity_path = directory.join(STORAGE_IDENTITY_FILENAME);
    if read_metadata(&identity_path)?.is_some() {
        return validate_directory(directory, environment);
    }

    match fs::read_dir(directory) {
        Ok(mut entries) => {
            if let Some(entry) = entries.next() {
                entry?;
                return Err(StorageError::UnrecognizedDirectory(directory.to_owned()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(directory)?;
        }
        Err(error) => return Err(error.into()),
    }

    match write_json(&identity_path, &StorageIdentity::new(environment), true) {
        Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
            validate_directory(directory, environment)
        }
        result => result,
    }
}

/// Write complete JSON beside its destination, then atomically promote it.
/// Identity uses no-clobber promotion so reopening cannot replace its contents.
fn write_json(path: &Path, value: &impl Serialize, create_only: bool) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().expect("metadata has a parent"))?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    let result = if create_only {
        file.persist_noclobber(path)
    } else {
        file.persist(path)
    };
    result.map_err(|error| error.error)?;

    Ok(())
}
