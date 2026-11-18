use std::fs;
use std::path::Path;

use ecopaste_neo_core::identity::{
    APPLICATION_ID, CUSTOM_DATA_DIR_NAME, PRODUCT_NAME, STORAGE_IDENTITY_FILENAME,
};
use ecopaste_neo_core::paths::{custom_data_dir, Environment, Storage};
use serde_json::{json, Value};
use tempfile::TempDir;

const OLD_MARKER: &str = ".ecopaste-storage.json";

fn storage(parent: &Path, environment: Environment) -> Storage {
    Storage::new(parent, environment).unwrap()
}

fn replace_json_field(path: &Path, field: &str, value: Value) {
    let mut document: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    document[field] = value;
    fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
}

#[test]
fn default_paths_use_formal_identity_and_separate_environments() {
    let temp = TempDir::new().unwrap();
    assert_eq!(PRODUCT_NAME, "EcoPasteNeo");
    assert_eq!(APPLICATION_ID, "io.github.cleaf-y.ecopaste-neo");

    for environment in [Environment::Dev, Environment::Prod] {
        let resolver = storage(temp.path(), environment);
        let paths = resolver.open().unwrap();
        let root = temp
            .path()
            .join(APPLICATION_ID)
            .join(environment.directory_name());
        assert_eq!(paths.root(), root);
        assert_eq!(paths.db_path(), root.join("db/clipboard.db"));
        assert_eq!(paths.settings_path(), root.join("config/settings.json"));
        assert_eq!(paths.resources_dir(), root.join("resources"));
        assert_eq!(paths.state_dir(), root.join("state"));
        assert!(root.join("db").is_dir());
        assert!(root.join("config").is_dir());
        assert!(paths.resources_dir().is_dir());
        assert!(paths.state_dir().is_dir());
        assert!(!paths.db_path().exists());
        let identity: Value =
            serde_json::from_slice(&fs::read(root.join(STORAGE_IDENTITY_FILENAME)).unwrap())
                .unwrap();
        assert_eq!(identity["applicationId"], APPLICATION_ID);
        assert_eq!(identity["environment"], environment.directory_name());
        assert_eq!(resolver.open().unwrap(), paths);
    }
}

#[test]
fn custom_directory_and_selection_survive_a_new_resolver() {
    let temp = TempDir::new().unwrap();
    let platform = temp.path().join("platform");
    let parent = temp.path().join("chosen");

    for environment in [Environment::Dev, Environment::Prod] {
        let resolver = storage(&platform, environment);
        let paths = resolver.open_custom(&parent).unwrap();
        assert_eq!(CUSTOM_DATA_DIR_NAME, "EcoPasteNeoData");
        assert_eq!(
            paths.root(),
            parent
                .join(CUSTOM_DATA_DIR_NAME)
                .join(environment.directory_name())
        );
        assert_eq!(custom_data_dir(&parent, environment), paths.root());
        let marker = paths.root().join(STORAGE_IDENTITY_FILENAME);
        let original_marker = fs::read(&marker).unwrap();
        fs::write(paths.db_path(), b"test database placeholder").unwrap();
        fs::write(paths.settings_path(), b"test settings placeholder").unwrap();

        let reopened = storage(&platform, environment).open().unwrap();
        assert_eq!(reopened, paths);
        assert_eq!(resolver.open_custom(&parent).unwrap(), paths);
        assert_eq!(fs::read(&marker).unwrap(), original_marker);
        assert_eq!(
            fs::read(paths.db_path()).unwrap(),
            b"test database placeholder"
        );
        assert_eq!(
            fs::read(paths.settings_path()).unwrap(),
            b"test settings placeholder"
        );
    }
}

#[test]
fn existing_empty_custom_directory_is_initialized() {
    let temp = TempDir::new().unwrap();
    let target = custom_data_dir(temp.path(), Environment::Dev);
    fs::create_dir_all(&target).unwrap();

    let paths = storage(&temp.path().join("platform"), Environment::Dev)
        .open_custom(temp.path())
        .unwrap();

    assert_eq!(paths.root(), target);
    assert!(target.join(STORAGE_IDENTITY_FILENAME).is_file());
}

#[test]
fn old_marker_is_never_renamed_or_adopted() {
    let temp = TempDir::new().unwrap();
    let resolver = storage(&temp.path().join("platform"), Environment::Dev);
    let parent = temp.path().join("chosen");
    let target = custom_data_dir(&parent, Environment::Dev);
    fs::create_dir_all(&target).unwrap();
    let old_bytes = br#"{"version":1,"environment":"dev","createdAt":"2026-01-01T00:00:00Z"}"#;
    fs::write(target.join(OLD_MARKER), old_bytes).unwrap();

    assert!(resolver.open_custom(&parent).is_err());
    assert_eq!(fs::read(target.join(OLD_MARKER)).unwrap(), old_bytes);
    assert!(!target.join(STORAGE_IDENTITY_FILENAME).exists());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
    assert!(!resolver.default_data_dir().join("storage.json").exists());
}

#[test]
fn unrelated_nonempty_default_and_custom_directories_are_unchanged() {
    for is_custom in [false, true] {
        let temp = TempDir::new().unwrap();
        let resolver = storage(&temp.path().join("platform"), Environment::Prod);
        let parent = temp.path().join("chosen");
        let target = if is_custom {
            custom_data_dir(&parent, Environment::Prod)
        } else {
            resolver.default_data_dir().to_owned()
        };
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("important.txt"), b"keep me").unwrap();

        let result = if is_custom {
            resolver.open_custom(&parent)
        } else {
            resolver.open()
        };

        assert!(result.is_err());
        assert_eq!(fs::read(target.join("important.txt")).unwrap(), b"keep me");
        assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
    }
}

#[test]
fn invalid_identity_is_rejected_without_rewriting_it() {
    for (field, value) in [
        ("applicationId", json!("com.ayangweb.eco-paste")),
        ("environment", json!("prod")),
        ("version", json!(2)),
    ] {
        let temp = TempDir::new().unwrap();
        let resolver = storage(&temp.path().join("platform"), Environment::Dev);
        let parent = temp.path().join("chosen");
        let paths = resolver.open_custom(&parent).unwrap();
        let marker = paths.root().join(STORAGE_IDENTITY_FILENAME);
        replace_json_field(&marker, field, value);
        let original = fs::read(&marker).unwrap();

        assert!(resolver.open().is_err());
        assert!(resolver.open_custom(&parent).is_err());
        assert_eq!(fs::read(&marker).unwrap(), original);
    }
}

#[test]
fn corrupt_default_identity_is_not_repaired() {
    let temp = TempDir::new().unwrap();
    let resolver = storage(temp.path(), Environment::Dev);
    let paths = resolver.open().unwrap();
    let marker = paths.root().join(STORAGE_IDENTITY_FILENAME);
    fs::write(&marker, b"broken json").unwrap();

    assert!(resolver.open().is_err());
    assert_eq!(fs::read(marker).unwrap(), b"broken json");
}

#[test]
fn corrupt_or_mismatched_selector_is_not_reset_to_default() {
    for (field, value) in [
        ("applicationId", json!("com.ayangweb.eco-paste")),
        ("environment", json!("prod")),
        ("version", json!(2)),
        ("dataDir", json!("relative/path")),
    ] {
        let temp = TempDir::new().unwrap();
        let resolver = storage(&temp.path().join("platform"), Environment::Dev);
        resolver.open_custom(&temp.path().join("chosen")).unwrap();
        let manifest = resolver.default_data_dir().join("storage.json");
        replace_json_field(&manifest, field, value);
        let original = fs::read(&manifest).unwrap();

        assert!(resolver.open().is_err());
        assert_eq!(fs::read(&manifest).unwrap(), original);
        fs::write(&manifest, b"broken json").unwrap();
        assert!(resolver.open().is_err());
        assert_eq!(fs::read(&manifest).unwrap(), b"broken json");
    }
}

#[test]
fn saved_target_with_only_old_identity_is_rejected_without_writes() {
    let temp = TempDir::new().unwrap();
    let resolver = storage(&temp.path().join("platform"), Environment::Dev);
    let paths = resolver.open_custom(&temp.path().join("chosen")).unwrap();
    let old = temp.path().join("EcoPasteData/dev");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join(OLD_MARKER), b"legacy identity").unwrap();
    fs::write(old.join("clipboard.db"), b"legacy database").unwrap();
    let manifest = resolver.default_data_dir().join("storage.json");
    replace_json_field(&manifest, "dataDir", json!(old));
    let original_manifest = fs::read(&manifest).unwrap();

    assert!(resolver.open().is_err());
    assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
    assert_eq!(fs::read(old.join(OLD_MARKER)).unwrap(), b"legacy identity");
    assert_eq!(
        fs::read(old.join("clipboard.db")).unwrap(),
        b"legacy database"
    );
    assert_eq!(fs::read_dir(&old).unwrap().count(), 2);
    assert!(paths.root().join(STORAGE_IDENTITY_FILENAME).exists());
}

#[test]
fn missing_saved_directory_is_not_recreated() {
    let temp = TempDir::new().unwrap();
    let resolver = storage(&temp.path().join("platform"), Environment::Dev);
    resolver.open_custom(&temp.path().join("chosen")).unwrap();
    let missing = temp.path().join("missing");
    let manifest = resolver.default_data_dir().join("storage.json");
    replace_json_field(&manifest, "dataDir", json!(missing));
    let original = fs::read(&manifest).unwrap();

    assert!(resolver.open().is_err());
    assert!(!missing.exists());
    assert_eq!(fs::read(&manifest).unwrap(), original);
}

#[test]
fn neo_default_leaves_legacy_tauri_storage_and_configuration_unchanged() {
    let temp = TempDir::new().unwrap();
    let config: Value =
        serde_json::from_str(include_str!("../../../src-tauri/tauri.conf.json")).unwrap();
    let legacy_id = config["identifier"].as_str().unwrap();
    assert_eq!(legacy_id, "com.ayangweb.eco-paste");
    assert_eq!(config["productName"], "EcoPaste");
    let legacy = temp.path().join(legacy_id).join("dev");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join(OLD_MARKER), b"legacy identity").unwrap();
    fs::write(legacy.join("storage.json"), b"legacy selector").unwrap();
    fs::write(legacy.join("clipboard.db"), b"legacy database").unwrap();

    let paths = storage(temp.path(), Environment::Dev).open().unwrap();

    assert_ne!(paths.root(), legacy);
    assert_eq!(
        fs::read(legacy.join(OLD_MARKER)).unwrap(),
        b"legacy identity"
    );
    assert_eq!(
        fs::read(legacy.join("storage.json")).unwrap(),
        b"legacy selector"
    );
    assert_eq!(
        fs::read(legacy.join("clipboard.db")).unwrap(),
        b"legacy database"
    );
    assert_eq!(fs::read_dir(&legacy).unwrap().count(), 3);
}

#[test]
fn relative_platform_or_custom_parent_is_rejected_before_writes() {
    assert!(Storage::new(Path::new("relative"), Environment::Dev).is_err());
    let temp = TempDir::new().unwrap();
    let resolver = storage(temp.path(), Environment::Dev);

    assert!(resolver.open_custom(Path::new("relative")).is_err());
    assert!(!resolver.default_data_dir().exists());
}

#[test]
fn a_file_cannot_be_used_as_a_directory() {
    let temp = TempDir::new().unwrap();
    let parent = temp.path().join("file");
    fs::write(&parent, b"keep me").unwrap();
    let resolver = storage(temp.path(), Environment::Dev);

    assert!(resolver.open_custom(&parent).is_err());
    assert_eq!(fs::read(parent).unwrap(), b"keep me");
}

#[cfg(unix)]
#[test]
fn linked_metadata_is_rejected_without_changing_the_destination() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let resolver = storage(temp.path(), Environment::Dev);
    let root = resolver.default_data_dir();
    fs::create_dir_all(root).unwrap();
    let original = temp.path().join("original.json");
    fs::write(&original, b"keep me").unwrap();
    symlink(&original, root.join(STORAGE_IDENTITY_FILENAME)).unwrap();

    assert!(resolver.open().is_err());
    assert_eq!(fs::read(&original).unwrap(), b"keep me");
}
