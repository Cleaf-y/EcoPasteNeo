#![cfg(feature = "capture")]

use std::fs;

use ecopaste_neo_core::{
    clipboard::settings::CaptureKind,
    paths::{Environment, Storage},
    settings::{Settings, SettingsChange, SettingsStore},
};
use serde_json::{json, Value};

#[test]
fn excluded_apps_deduplicate_remove_and_stay_empty_after_restart() {
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let store = SettingsStore::open(&paths);
    let original = store.snapshot().unwrap();
    #[cfg(target_os = "macos")]
    assert_eq!(
        original.filters.excluded_app_ids,
        ["com.apple.keychainaccess", "com.apple.Passwords"]
    );
    for id in [
        "com.apple.TextEdit",
        "com.example.browser",
        "com.apple.TextEdit",
    ] {
        store
            .update(SettingsChange::AddExcludedApp(id.into()))
            .unwrap();
    }
    let ids = store.snapshot().unwrap().filters.excluded_app_ids;
    assert_eq!(
        ids.iter().filter(|id| *id == "com.apple.TextEdit").count(),
        1
    );
    store
        .update(SettingsChange::RemoveExcludedApp(
            "com.apple.TextEdit".into(),
        ))
        .unwrap();
    let reopened = SettingsStore::open(&paths);
    assert!(!reopened
        .capture_settings()
        .unwrap()
        .excluded_app_ids
        .contains(&"com.apple.TextEdit".into()));
    assert!(reopened
        .capture_settings()
        .unwrap()
        .excluded_app_ids
        .contains(&"com.example.browser".into()));
    reopened.update(SettingsChange::ClearExcludedApps).unwrap();
    let reopened = SettingsStore::open(&paths);
    let settings = reopened.snapshot().unwrap();
    assert!(settings.filters.excluded_app_ids.is_empty());
    assert_eq!(settings.capture_enabled, original.capture_enabled);
    assert_eq!(settings.capture, original.capture);
    assert_eq!(settings.sensitive, original.sensitive);
    let document: Value =
        serde_json::from_slice(&fs::read(paths.settings_path()).unwrap()).unwrap();
    assert_eq!(document["filters"]["excludedAppIds"], json!([]));
    for id in ["", " ", "com.example.some app"] {
        assert!(reopened
            .update(SettingsChange::AddExcludedApp(id.into()))
            .is_err());
        assert!(reopened
            .capture_settings()
            .unwrap()
            .excluded_app_ids
            .is_empty());
    }
}

#[test]
fn exclusion_actions_merge_latest_list_and_preserve_unknown_filter_fields() {
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    fs::write(paths.settings_path(), br#"{"filters":{"futureRule":42}}"#).unwrap();
    let store = std::sync::Arc::new(SettingsStore::open(&paths));
    std::thread::scope(|scope| {
        for id in ["test.one", "test.two", "test.one"] {
            let store = store.clone();
            scope.spawn(move || {
                store
                    .update(SettingsChange::AddExcludedApp(id.into()))
                    .unwrap()
            });
        }
    });
    let ids = store.snapshot().unwrap().filters.excluded_app_ids;
    for id in ["test.one", "test.two"] {
        assert_eq!(ids.iter().filter(|saved| *saved == id).count(), 1);
    }
    let before = fs::read(paths.settings_path()).unwrap();
    let backup = paths.settings_path().with_extension("saved");
    fs::rename(paths.settings_path(), &backup).unwrap();
    fs::create_dir(paths.settings_path()).unwrap();
    assert!(store.update(SettingsChange::ClearExcludedApps).is_err());
    assert_eq!(store.capture_settings().unwrap().excluded_app_ids, ids);
    fs::remove_dir(paths.settings_path()).unwrap();
    fs::rename(backup, paths.settings_path()).unwrap();
    assert_eq!(fs::read(paths.settings_path()).unwrap(), before);
    store.update(SettingsChange::ClearExcludedApps).unwrap();
    let document: Value =
        serde_json::from_slice(&fs::read(paths.settings_path()).unwrap()).unwrap();
    assert_eq!(document["filters"]["futureRule"], 42);
    assert_eq!(document["filters"]["excludedAppIds"], json!([]));
}

#[test]
fn defaults_reopen_environment_isolation_and_legacy_file_untouched() {
    let parent = tempfile::tempdir().unwrap();
    let legacy = parent
        .path()
        .join("com.ayangweb.eco-paste/dev/config/settings.json");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    let legacy_bytes = br#"{"captureEnabled":false,"capture":{"text":false}}"#;
    fs::write(&legacy, legacy_bytes).unwrap();
    let dev = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let prod = Storage::new(parent.path(), Environment::Prod)
        .unwrap()
        .open()
        .unwrap();
    let store = SettingsStore::open(&dev);
    assert_eq!(store.snapshot().unwrap(), Settings::default());
    store
        .update(SettingsChange::CaptureType(CaptureKind::Image, false))
        .unwrap();
    store.update(SettingsChange::CaptureEnabled(false)).unwrap();
    assert!(store.capture_settings().is_none());
    let bytes = fs::read(dev.settings_path()).unwrap();
    let reopened = SettingsStore::open(&dev);
    assert!(!reopened.snapshot().unwrap().capture_enabled);
    assert!(!reopened.snapshot().unwrap().capture.image);
    assert_eq!(bytes, fs::read(dev.settings_path()).unwrap());
    reopened
        .update(SettingsChange::CaptureEnabled(true))
        .unwrap();
    assert!(!reopened.capture_settings().unwrap().capture.image);
    assert!(reopened.capture_settings().unwrap().capture.text);
    assert_eq!(
        SettingsStore::open(&prod).snapshot().unwrap(),
        Settings::default()
    );
    assert_ne!(dev.settings_path(), prod.settings_path());
    assert_eq!(fs::read(legacy).unwrap(), legacy_bytes);
}

#[test]
fn field_updates_preserve_rules_unknown_fields_and_other_window_changes() {
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let document = json!({
        "capture":{"text":true,"maxTextMb":7,"maxImageMb":17,"order":["text","files"],"futureType":"keep"},
        "sensitive":{"collectSecrets":false,"redactSecrets":false},
        "filters":{"excludedAppIds":["test.custom.source"],"futureRule":42},
        "futureGroup":{"enabled":true}
    });
    fs::write(
        paths.settings_path(),
        serde_json::to_vec(&document).unwrap(),
    )
    .unwrap();
    let store = SettingsStore::open(&paths);
    let old_window = store.snapshot().unwrap();
    store
        .update(SettingsChange::CaptureType(CaptureKind::Html, false))
        .unwrap();
    // A second window's single-field action cannot restore its old HTML value.
    assert!(old_window.capture.html);
    store
        .update(SettingsChange::CaptureType(CaptureKind::Image, false))
        .unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(paths.settings_path()).unwrap()).unwrap();
    assert_eq!(saved["capture"]["html"], false);
    assert_eq!(saved["capture"]["image"], false);
    for key in ["sensitive", "filters", "futureGroup"] {
        assert_eq!(saved[key], document[key]);
    }
    for key in ["maxTextMb", "maxImageMb", "order", "futureType"] {
        assert_eq!(saved["capture"][key], document["capture"][key]);
    }
    let capture = store.capture_settings().unwrap();
    assert_eq!(capture.excluded_app_ids, ["test.custom.source"]);
    assert!(!capture.sensitive.collect_secrets);
    assert!(!capture.sensitive.redact_secrets);
}

#[test]
fn corrupt_or_invalid_existing_files_fail_closed_and_are_not_overwritten() {
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    for invalid in [
        "{broken",
        "null",
        "[]",
        r#"{"capture":{"image":"yes"}}"#,
        r#"{"captureEnabled":null}"#,
        r#"{"capture":{"order":["bogus"]}}"#,
    ] {
        fs::write(paths.settings_path(), invalid).unwrap();
        let store = SettingsStore::open(&paths);
        assert!(store.snapshot().is_err(), "{invalid}");
        assert!(store.capture_settings().is_none());
        assert!(store.update(SettingsChange::CaptureEnabled(true)).is_err());
        assert_eq!(fs::read_to_string(paths.settings_path()).unwrap(), invalid);
        fs::write(paths.settings_path(), r#"{"captureEnabled":false}"#).unwrap();
        assert!(!store.reload().unwrap().capture_enabled);
        assert!(store.capture_settings().is_none());
    }
}

#[test]
fn failed_atomic_replacement_keeps_saved_snapshot_and_listener_rules() {
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let store = SettingsStore::open(&paths);
    let previous = store.capture_settings().unwrap();
    let saved_file = paths.settings_path().with_extension("saved");
    fs::rename(paths.settings_path(), &saved_file).unwrap();
    fs::create_dir(paths.settings_path()).unwrap();
    assert!(store.update(SettingsChange::CaptureEnabled(false)).is_err());
    assert!(store.snapshot().unwrap().capture_enabled);
    assert_eq!(store.capture_settings().unwrap(), previous);
    fs::remove_dir(paths.settings_path()).unwrap();
    fs::rename(saved_file, paths.settings_path()).unwrap();
    assert_eq!(
        SettingsStore::open(&paths).snapshot().unwrap(),
        Settings::default()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn unwritable_directory_reports_error_without_publishing_unsaved_values() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let store = SettingsStore::open(&paths);
    let directory = paths.settings_path().parent().unwrap().to_path_buf();
    let before = fs::read(paths.settings_path()).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o500)).unwrap();
    let result = store.update(SettingsChange::CaptureType(CaptureKind::Text, false));
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert!(store.snapshot().unwrap().capture.text);
    assert!(store.capture_settings().unwrap().capture.text);
    assert_eq!(fs::read(paths.settings_path()).unwrap(), before);
}

#[test]
fn concurrent_field_updates_are_serialized_without_lost_choices() {
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let store = std::sync::Arc::new(SettingsStore::open(&paths));
    std::thread::scope(|scope| {
        for kind in [
            CaptureKind::Text,
            CaptureKind::Html,
            CaptureKind::Rtf,
            CaptureKind::Image,
            CaptureKind::Files,
        ] {
            let store = store.clone();
            scope.spawn(move || {
                store
                    .update(SettingsChange::CaptureType(kind, false))
                    .unwrap();
            });
        }
    });
    let capture = SettingsStore::open(&paths)
        .capture_settings()
        .unwrap()
        .capture;
    assert!(!capture.text && !capture.html && !capture.rtf && !capture.image && !capture.files);
}

#[cfg(target_os = "macos")]
#[test]
fn failed_first_run_write_and_unreadable_existing_file_both_pause_capture() {
    use std::os::unix::fs::PermissionsExt;

    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let path = paths.settings_path();
    let directory = path.parent().unwrap();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o500)).unwrap();
    let first_run = SettingsStore::open(&paths);
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(first_run.snapshot().is_err());
    assert!(first_run.capture_settings().is_none());
    assert!(!path.exists());

    let bytes = br#"{"captureEnabled":false,"capture":{"image":false}}"#;
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let unreadable = SettingsStore::open(&paths);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(unreadable.snapshot().is_err());
    assert!(unreadable.capture_settings().is_none());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(!unreadable.reload().unwrap().capture_enabled);
}

#[cfg(target_os = "macos")]
#[test]
fn dangling_settings_link_is_not_replaced_by_first_run_defaults() {
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    let target = parent.path().join("missing-settings.json");
    std::os::unix::fs::symlink(&target, paths.settings_path()).unwrap();
    let store = SettingsStore::open(&paths);
    assert!(store.snapshot().is_err());
    assert!(store.capture_settings().is_none());
    assert_eq!(fs::read_link(paths.settings_path()).unwrap(), target);
    assert!(!target.exists());
}

#[test]
fn sensitive_changes_preserve_independent_rules_and_failed_saves() {
    let parent = tempfile::tempdir().unwrap();
    let paths = Storage::new(parent.path(), Environment::Dev)
        .unwrap()
        .open()
        .unwrap();
    fs::write(
        paths.settings_path(),
        br#"{"sensitive":{"futureRule":7},"filters":{"excludedAppIds":["test.excluded"]}}"#,
    )
    .unwrap();
    let store = SettingsStore::open(&paths);
    let initial = store.snapshot().unwrap();
    assert!(initial.sensitive.collect_secrets && initial.sensitive.redact_secrets);
    for collect in [false, true] {
        for redact in [false, true] {
            store
                .update(SettingsChange::CollectSecrets(collect))
                .unwrap();
            store.update(SettingsChange::RedactSecrets(redact)).unwrap();
            let reopened = SettingsStore::open(&paths).snapshot().unwrap();
            assert_eq!(reopened.sensitive.collect_secrets, collect);
            assert_eq!(reopened.sensitive.redact_secrets, redact);
            assert_eq!(reopened.filters, initial.filters);
            assert_eq!(reopened.capture, initial.capture);
            assert_eq!(reopened.capture_enabled, initial.capture_enabled);
        }
    }
    let before = store.snapshot().unwrap();
    let backup = paths.settings_path().with_extension("saved");
    fs::rename(paths.settings_path(), &backup).unwrap();
    fs::create_dir(paths.settings_path()).unwrap();
    assert!(store.update(SettingsChange::RedactSecrets(false)).is_err());
    assert!(store.update(SettingsChange::CollectSecrets(false)).is_err());
    assert_eq!(store.snapshot().unwrap().sensitive, before.sensitive);
    fs::remove_dir(paths.settings_path()).unwrap();
    fs::rename(backup, paths.settings_path()).unwrap();
    let document: Value =
        serde_json::from_slice(&fs::read(paths.settings_path()).unwrap()).unwrap();
    assert_eq!(document["sensitive"]["futureRule"], 7);
}
