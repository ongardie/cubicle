//! Main Cubicle program configuration.

use serde::Deserialize;
use std::path::{Path, PathBuf};

use super::RunnerKind;
use crate::somehow::{somehow as anyhow, Context, Result};

/// Main Cubicle program configuration, normally read from a `cubicle.toml`
/// file.
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Which runner to use.
    pub runner: RunnerKind,

    /// Configuration specific to the Bubblewrap runner. Set to `None` for
    /// other runners.
    #[serde(default)]
    pub bubblewrap: Option<Bubblewrap>,

    /// Configuration specific to the Docker runner. Set to `Docker::default()`
    /// for other runners.
    #[serde(default)]
    pub docker: Docker,
}

/// Configuration specific to the Bubblewrap runner.
///
/// See the [Configuration](#configuration) section below for details.
/// This documentation is included from `docs/Bubblewrap.md`.
#[doc = include_str!("../docs/Bubblewrap.md")]
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Bubblewrap {
    pub seccomp: PathOrDisabled,
}

/// Like an `Option<PathBuf>` but more opinionated about recommending a path be
/// set.
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(from = "String")]
pub enum PathOrDisabled {
    /// Against our recommendations, the user has insisted on disabling this.
    /// It may be a poor choice for security or maybe they know best.
    DangerouslyDisabled,
    /// Host path.
    Path(PathBuf),
}

impl std::convert::From<String> for PathOrDisabled {
    fn from(s: String) -> Self {
        if s == "dangerously-disabled" {
            PathOrDisabled::DangerouslyDisabled
        } else {
            PathOrDisabled::Path(PathBuf::from(s))
        }
    }
}

/// Configuration specific to the Docker runner.
///
/// See the [Configuration](#configuration) section below for details.
/// This documentation is included from `docs/Docker.md`.
#[doc = include_str!("../docs/Docker.md")]
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Docker {
    #[serde(default)]
    pub bind_mounts: bool,

    #[serde(default)]
    pub extra_packages: Vec<String>,

    #[serde(default = "cub_dash")]
    pub prefix: String,

    #[serde(default)]
    pub slim: bool,
}

impl Default for Docker {
    fn default() -> Self {
        Self {
            bind_mounts: Default::default(),
            extra_packages: Vec::default(),
            prefix: cub_dash(),
            slim: false,
        }
    }
}

fn cub_dash() -> String {
    String::from("cub-")
}

impl Config {
    /// Parses and validates a TOML-formatted string into a Config.
    ///
    /// The returned error message lacks context.
    fn from_str(s: &str) -> Result<Self> {
        let config: Config = toml::from_str(s).enough_context()?;

        match config.runner {
            RunnerKind::Bubblewrap => {
                if config.bubblewrap.is_none() {
                    return Err(anyhow!("Bubblewrap settings are required for that runner. See `docs/Bubblewrap.md`."));
                }
            }
            RunnerKind::Docker => {}
            RunnerKind::User => {}
        }

        Ok(config)
    }

    /// Parses a TOML-formatted config file.
    pub fn read_from_file(path: &Path) -> Result<Self> {
        let buf = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {path:?}"))?;
        Self::from_str(&buf)
            .with_context(|| format!("Failed to parse/validate config file: {path:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_from_str_bad_runner() {
        assert_eq!(
            "missing field `runner`",
            Config::from_str("").unwrap_err().to_string()
        );
        assert_eq!(
            "unknown variant `q`, expected one of `Bubblewrap`, `Docker`, `User` for key `runner` at line 1 column 1",
            Config::from_str("runner = 'q'").unwrap_err().to_string()
        );
    }

    #[test]
    #[should_panic(expected = "unknown field `asdf`")]
    fn config_from_str_unknown_field() {
        Config::from_str(
            "
            runner = 'docker'
            asdf = 'what?'
            ",
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "Bubblewrap settings are required")]
    fn config_from_str_missing_bubblewrap() {
        Config::from_str("runner = 'bubblewrap'").unwrap();
    }

    #[test]
    fn config_from_str_ok() {
        let expected = Config {
            runner: RunnerKind::Docker,
            bubblewrap: None,
            docker: Docker::default(),
        };
        assert_eq!(expected, Config::from_str("runner = 'docker'").unwrap());
        assert_eq!(
            expected,
            Config::from_str(
                "
                    runner = 'docker'
                    [docker]
                "
            )
            .unwrap()
        );
    }

    #[test]
    fn config_from_str_full() {
        assert_eq!(
            Config {
                runner: RunnerKind::Docker,
                bubblewrap: Some(Bubblewrap {
                    seccomp: PathOrDisabled::Path(PathBuf::from("/tmp/seccomp.bpf")),
                }),
                docker: Docker {
                    bind_mounts: true,
                    extra_packages: vec![String::from("foo"), String::from("bar")],
                    prefix: String::from("p"),
                    slim: true,
                },
            },
            Config::from_str(
                "
                runner = 'docker'

                [bubblewrap]
                seccomp = '/tmp/seccomp.bpf'

                [docker]
                bind_mounts = true
                extra_packages = ['foo', 'bar']
                prefix = 'p'
                slim = true
                "
            )
            .unwrap()
        );
    }

    #[test]
    fn config_from_str_full_seccomp_disabled() {
        assert_eq!(
            PathOrDisabled::DangerouslyDisabled,
            Config::from_str(
                "
                runner = 'bubblewrap'
                [bubblewrap]
                seccomp = 'dangerously-disabled'
                "
            )
            .unwrap()
            .bubblewrap
            .unwrap()
            .seccomp
        );
    }
}
