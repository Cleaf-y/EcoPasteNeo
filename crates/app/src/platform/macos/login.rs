//! `SMAppService.mainAppService` login item for the packaged app.
use anyhow::{Result, bail};
use ecopaste_neo_app::locale::Locale;
use ecopaste_neo_core::identity::{APPLICATION_ID, PRODUCT_NAME};
use objc2::{rc::Retained, runtime::AnyClass};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSBundle, NSProcessInfo};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use std::path::Path;

use crate::platform::LoginStatus;

const OPEN_APPLICATION: u32 = u32::from_be_bytes(*b"oapp");
const PROPERTY_DATA: u32 = u32::from_be_bytes(*b"prdt");
const LOGIN_ITEM: u32 = u32::from_be_bytes(*b"lgit");

/// Read during GPUI's applicationDidFinishLaunching callback, while the launch event is current.
pub fn launched_at_login() -> bool {
    let event = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent();
    is_login_event(event.as_deref())
}

fn is_login_event(event: Option<&NSAppleEventDescriptor>) -> bool {
    event.is_some_and(|event| {
        event.eventID() == OPEN_APPLICATION
            && event
                .paramDescriptorForKeyword(PROPERTY_DATA)
                .is_some_and(|property| property.enumCodeValue() == LOGIN_ITEM)
    })
}

enum LoginItem {
    Available(Retained<SMAppService>),
    Unavailable(LoginStatus),
}

impl LoginItem {
    /// Check runtime availability before referencing the macOS 13 class. Only the running .app qualifies.
    fn current() -> Self {
        if NSProcessInfo::processInfo()
            .operatingSystemVersion()
            .majorVersion
            < 13
            || AnyClass::get(c"SMAppService").is_none()
        {
            return Self::Unavailable(LoginStatus::Unsupported);
        }
        let bundle = NSBundle::mainBundle();
        let valid = bundle
            .bundleIdentifier()
            .is_some_and(|identifier| identifier.to_string() == APPLICATION_ID)
            && std::env::current_exe().is_ok_and(|executable| {
                matches_bundle(Path::new(&bundle.bundlePath().to_string()), &executable)
            });
        if !valid {
            return Self::Unavailable(LoginStatus::Unpackaged);
        }
        // SAFETY: macOS version/class availability was checked and the main bundle is Neo.
        Self::Available(unsafe { SMAppService::mainAppService() })
    }

    fn status(&self, locale: Locale) -> Result<LoginStatus> {
        match self {
            // SAFETY: the service was created only on a supported OS, and remains retained.
            Self::Available(service) => decode_status(unsafe { service.status() }, locale),
            Self::Unavailable(status) => Ok(*status),
        }
    }
}

fn matches_bundle(bundle: &Path, executable: &Path) -> bool {
    if bundle
        .extension()
        .is_none_or(|extension| extension != "app")
    {
        return false;
    }
    let expected = bundle.join("Contents/MacOS").join(PRODUCT_NAME);
    match (expected.canonicalize(), executable.canonicalize()) {
        (Ok(expected), Ok(executable)) => expected == executable,
        _ => false,
    }
}

fn decode_status(status: SMAppServiceStatus, locale: Locale) -> Result<LoginStatus> {
    match status {
        SMAppServiceStatus::NotRegistered => Ok(LoginStatus::NotRegistered),
        SMAppServiceStatus::Enabled => Ok(LoginStatus::Enabled),
        SMAppServiceStatus::RequiresApproval => Ok(LoginStatus::RequiresApproval),
        SMAppServiceStatus::NotFound => Ok(LoginStatus::NotFound),
        _ => bail!("{} ({})", locale.login_unknown(), status.0),
    }
}

/// Query ServiceManagement, including changes made outside Neo. This never registers anything.
pub fn status(locale: Locale) -> Result<LoginStatus> {
    LoginItem::current().status(locale)
}

/// Mutate only on an explicit user action; pending approval is never retried automatically.
pub fn set_enabled(enabled: bool, locale: Locale) -> Result<()> {
    let item = LoginItem::current();
    let status = item.status(locale)?;
    let LoginItem::Available(service) = item else {
        bail!("{}", status.label(locale));
    };
    if (enabled && matches!(status, LoginStatus::Enabled | LoginStatus::RequiresApproval))
        || (!enabled && status == LoginStatus::NotRegistered)
    {
        return Ok(());
    }
    // SAFETY: the service belongs to this validated main app. Unlike an agent, unregistering
    // mainAppService does not terminate the running app. These calls need no AppKit UI thread.
    let result = unsafe {
        if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|error| {
        anyhow::anyhow!(
            "{}: {}",
            locale.login_failed(),
            error.localizedDescription()
        )
    })
}

/// Open the system approval screen only in response to its dedicated Settings button.
pub fn open_system_settings() {
    if let LoginItem::Available(_) = LoginItem::current() {
        // SAFETY: availability was checked above; the caller is an explicit UI action.
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_login_open_event_counts_as_login_startup() {
        assert!(!is_login_event(None));
        for id in [OPEN_APPLICATION, u32::from_be_bytes(*b"rapp")] {
            let event = NSAppleEventDescriptor::appleEventWithEventClass_eventID_targetDescriptor_returnID_transactionID(
                u32::from_be_bytes(*b"aevt"), id, None, -1, 0,
            );
            assert!(!is_login_event(Some(&event)));
            event.setParamDescriptor_forKeyword(
                &NSAppleEventDescriptor::descriptorWithEnumCode(LOGIN_ITEM),
                PROPERTY_DATA,
            );
            assert_eq!(is_login_event(Some(&event)), id == OPEN_APPLICATION);
            event.setParamDescriptor_forKeyword(
                &NSAppleEventDescriptor::descriptorWithEnumCode(u32::from_be_bytes(*b"none")),
                PROPERTY_DATA,
            );
            assert!(!is_login_event(Some(&event)));
        }
    }

    #[test]
    fn native_states_do_not_treat_approval_or_missing_service_as_enabled() {
        for (native, expected) in [
            (
                SMAppServiceStatus::NotRegistered,
                LoginStatus::NotRegistered,
            ),
            (SMAppServiceStatus::Enabled, LoginStatus::Enabled),
            (
                SMAppServiceStatus::RequiresApproval,
                LoginStatus::RequiresApproval,
            ),
            (SMAppServiceStatus::NotFound, LoginStatus::NotFound),
        ] {
            assert_eq!(decode_status(native, Locale::ZhCn).unwrap(), expected);
            assert!(!expected.label(Locale::ZhCn).is_empty());
            assert!(!expected.label(Locale::EnUs).is_empty());
        }
        assert!(decode_status(SMAppServiceStatus(99), Locale::EnUs).is_err());
    }

    #[test]
    fn a_debug_binary_cannot_be_registered_as_the_packaged_app() {
        let temporary = tempfile::tempdir().unwrap();
        let bundle = temporary.path().join("EcoPasteNeo.app");
        let packaged = bundle.join("Contents/MacOS").join(PRODUCT_NAME);
        std::fs::create_dir_all(packaged.parent().unwrap()).unwrap();
        std::fs::write(&packaged, "fixture").unwrap();
        let debug = temporary.path().join("target/debug/EcoPasteNeo");
        std::fs::create_dir_all(debug.parent().unwrap()).unwrap();
        std::fs::write(&debug, "fixture").unwrap();
        assert!(matches_bundle(&bundle, &packaged));
        assert!(!matches_bundle(&bundle, &debug));
        assert!(!matches_bundle(temporary.path(), &packaged));
    }
}
