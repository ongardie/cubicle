use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

use super::RunnerKind;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub(super) runner: RunnerKind,
    #[serde(default)]
    pub docker: Docker,
}

/// See <docs/Docker.md> for details.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Docker {
    #[serde(default)]
    pub bind_mounts: bool,

    #[serde(default = "cub_dash")]
    pub prefix: String,
}

impl Default for Docker {
    fn default() -> Self {
        Self {
            bind_mounts: Default::default(),
            prefix: cub_dash(),
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
        let config = toml::from_str(s)?;
        Ok(config)
    }

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

        #[cfg(target_os = "linux")]
        let expected =
            "unknown variant `q`, expected one of `Bubblewrap`, `Docker`, `User` for key `runner` at line 1 column 1";
        #[cfg(not(target_os = "linux"))]
        let expected =
            "unknown variant `q`, expected `Docker` or `User` for key `runner` at line 1 column 1";
        assert_eq!(
            expected,
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
    fn config_from_str_ok() {
        let expected = Config {
            runner: RunnerKind::Docker,
            docker: Docker {
                bind_mounts: false,
                prefix: String::from("cub-"),
            },
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
                docker: Docker {
                    bind_mounts: true,
                    prefix: String::from("p"),
                },
            },
            Config::from_str(
                "
                runner = 'docker'
                [docker]
                bind_mounts = true
                prefix = 'p'
                "
            )
            .unwrap()
        );
    }
}
