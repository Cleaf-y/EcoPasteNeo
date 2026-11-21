use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use ecopaste_neo_core::paths::Environment;

use crate::locale::Locale;

/// Startup selection is explicit; tests can point the app at an isolated parent.
#[derive(Debug)]
pub struct Options {
    pub data_parent: PathBuf,
    pub environment: Environment,
    pub locale: Locale,
    pub show: bool,
    pub auto_launch: bool,
    pub capture: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StartupWindows {
    pub panel: bool,
    pub settings: bool,
}

impl Options {
    /// Parse only supported flags, refusing ambiguous paths or unknown arguments.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        Self::parse_with_show(
            arguments,
            std::env::var_os("ECOPASTENEO_SHOW").is_some_and(|value| value == "1"),
        )
    }

    fn parse_with_show(
        arguments: impl IntoIterator<Item = String>,
        mut show: bool,
    ) -> Result<Self> {
        let mut args = arguments.into_iter();
        let mut data_parent = std::env::var_os("ECOPASTENEO_DATA_PARENT").map(PathBuf::from);
        let mut environment = if cfg!(debug_assertions) {
            Environment::Dev
        } else {
            Environment::Prod
        };
        let mut locale = Locale::ZhCn;
        let mut auto_launch = false;
        let mut capture =
            std::env::var_os("ECOPASTENEO_NO_CAPTURE").is_none_or(|value| value != "1");
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--show" => show = true,
                "--auto-launch" => auto_launch = true,
                "--no-capture" => capture = false,
                "--data-parent" => {
                    data_parent = Some(PathBuf::from(
                        args.next()
                            .context("--data-parent requires an absolute path")?,
                    ))
                }
                "--environment" => {
                    environment = match args.next().as_deref() {
                        Some("dev") => Environment::Dev,
                        Some("prod") => Environment::Prod,
                        _ => bail!("--environment requires dev or prod"),
                    }
                }
                "--locale" => {
                    locale = match args.next().as_deref() {
                        Some("zh-CN") => Locale::ZhCn,
                        Some("en-US") => Locale::EnUs,
                        _ => bail!("--locale requires zh-CN or en-US"),
                    }
                }
                _ => bail!("Unknown argument: {argument}"),
            }
        }
        let data_parent = match data_parent {
            Some(path) => path,
            None => platform_data_parent()?,
        };
        if !data_parent.is_absolute() {
            bail!("The data parent must be absolute");
        }
        Ok(Self {
            data_parent,
            environment,
            locale,
            show,
            auto_launch,
            capture,
        })
    }

    /// Login startup takes precedence over explicit flags, environment and recovery UI.
    pub fn startup_windows(
        &self,
        system_login: bool,
        settings_unavailable: bool,
    ) -> StartupWindows {
        let background = self.auto_launch || system_login;
        StartupWindows {
            panel: !background && self.show,
            settings: !background && settings_unavailable,
        }
    }
}

/// Resolve the platform parent only; core owns application identity and layout.
fn platform_data_parent() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Ok(
            PathBuf::from(std::env::var_os("HOME").context("HOME is unavailable")?)
                .join("Library/Application Support"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        Ok(PathBuf::from(
            std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?,
        ))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        bail!("EcoPasteNeo supports macOS and Windows only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }
    #[test]
    fn explicit_test_parent_and_locale_are_preserved() {
        let parent = std::env::temp_dir().join("neo-options-test");
        let options = Options::parse(args(&[
            "--data-parent",
            parent.to_str().unwrap(),
            "--environment",
            "dev",
            "--locale",
            "en-US",
            "--show",
            "--no-capture",
        ]))
        .unwrap();
        assert_eq!(options.data_parent, parent);
        assert_eq!(options.environment, Environment::Dev);
        assert!(matches!(options.locale, Locale::EnUs));
        assert!(options.show);
        assert!(!options.capture);
    }
    #[test]
    fn malformed_startup_arguments_do_not_fall_back_to_user_storage() {
        for values in [
            vec!["--data-parent", "relative"],
            vec!["--environment", "testing"],
            vec!["--locale", "xx"],
            vec!["--unknown"],
            vec!["--data-parent"],
        ] {
            assert!(Options::parse(args(&values)).is_err());
        }
    }

    #[test]
    fn login_startup_never_opens_windows_even_with_broken_settings() {
        for values in [
            vec![],
            vec!["--show"],
            vec!["--show", "--auto-launch"],
            vec!["--auto-launch", "--show"],
        ] {
            let options = Options::parse_with_show(args(&values), true).unwrap();
            for broken in [false, true] {
                assert_eq!(
                    options.startup_windows(true, broken),
                    StartupWindows {
                        panel: false,
                        settings: false
                    }
                );
                if options.auto_launch {
                    assert_eq!(
                        options.startup_windows(false, broken),
                        StartupWindows {
                            panel: false,
                            settings: false
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn manual_show_and_settings_recovery_remain_available() {
        let options = Options::parse_with_show(args(&["--show"]), false).unwrap();
        assert_eq!(
            options.startup_windows(false, false),
            StartupWindows {
                panel: true,
                settings: false
            }
        );
        assert_eq!(
            options.startup_windows(false, true),
            StartupWindows {
                panel: true,
                settings: true
            }
        );
        let options = Options::parse_with_show(args(&[]), false).unwrap();
        assert_eq!(
            options.startup_windows(false, true),
            StartupWindows {
                panel: false,
                settings: true
            }
        );
    }
}
