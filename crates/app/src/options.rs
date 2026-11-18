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
}

impl Options {
    /// Parse only supported flags, refusing ambiguous paths or unknown arguments.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut args = arguments.into_iter();
        let mut data_parent = std::env::var_os("ECOPASTENEO_DATA_PARENT").map(PathBuf::from);
        let mut environment = if cfg!(debug_assertions) {
            Environment::Dev
        } else {
            Environment::Prod
        };
        let mut locale = Locale::ZhCn;
        let mut show = std::env::var_os("ECOPASTENEO_SHOW").is_some_and(|value| value == "1");
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--show" => show = true,
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
        })
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
        let options = Options::parse(args(&[
            "--data-parent",
            "/tmp/neo-options-test",
            "--environment",
            "dev",
            "--locale",
            "en-US",
            "--show",
        ]))
        .unwrap();
        assert_eq!(options.data_parent, PathBuf::from("/tmp/neo-options-test"));
        assert_eq!(options.environment, Environment::Dev);
        assert!(matches!(options.locale, Locale::EnUs));
        assert!(options.show);
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
}
