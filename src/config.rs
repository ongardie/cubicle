//! Main Cubicle program configuration.

use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Deserializer};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use super::os_util::host_home_dir;
use super::HostPath;
use super::RunnerKind;
use crate::somehow::{somehow as anyhow, Context, LowLevelResult, Result};

/// Main Cubicle program configuration, normally read from a `cubicle.toml`
/// file.
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Which runner to use.
    pub runner: RunnerKind,

    /// Packages will be re-built when accessed if they haven't been built for
    /// this amount of time.
    ///
    /// Set to `"never"` in TOML or `None` in code to disable.
    ///
    /// Default: 12 hours.
    #[serde(
        default = "twelve_hours",
        deserialize_with = "deserialize_opt_duration"
    )]
    pub auto_update: Option<Duration>,

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

fn twelve_hours() -> Option<Duration> {
    Some(Duration::from_secs(60 * 60 * 12))
}

fn deserialize_opt_duration<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    lazy_static! {
        static ref RE: Regex = RegexBuilder::new(
            r#"^(?x)
            # integer or decimal
            (?P<value>
                [0-9]+
                ( \. [0-9]+ )?
            )
            # optional space
            \ ?
            # required unit
            (?P<unit>
                s | sec s? | second s? |
                m | min s? | minute s? |
                h | hr s? | hour s? |
                d | day s?
            )
            $"#
        )
        .build()
        .unwrap();
    }

    let s = String::deserialize(deserializer)?;
    if s == "never" {
        return Ok(None);
    }

    match RE.captures(&s) {
        Some(caps) => {
            let value = caps.name("value").unwrap().as_str();
            let value = f64::from_str(value).unwrap();
            let unit = caps.name("unit").unwrap().as_str();
            let multiple = f64::from(match unit.chars().next().unwrap() {
                's' => 1,
                'm' => 60,
                'h' => 60 * 60,
                'd' => 60 * 60 * 24,
                _ => unreachable!(),
            });
            Ok(Some(Duration::from_secs_f64(value * multiple)))
        }

        None => Err(D::Error::custom(format!(
            "could not parse {s:?}, expected `never` or duration like \
            `10s`, `1.5m`, `2 hours`, `1 day`"
        ))),
    }
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
            PathOrDisabled::Path(tilde_expand(PathBuf::from(s), host_home_dir()))
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

    #[serde(default, deserialize_with = "deserialize_opt_path")]
    pub seccomp: Option<PathBuf>,

    #[serde(default)]
    pub strict_debian_packages: bool,

    #[serde(default = "cub_dash")]
    pub prefix: String,
}

impl Default for Docker {
    fn default() -> Self {
        Self {
            bind_mounts: Default::default(),
            seccomp: None,
            strict_debian_packages: false,
            prefix: cub_dash(),
        }
    }
}

fn cub_dash() -> String {
    String::from("cub-")
}

fn deserialize_opt_path<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<PathBuf>::deserialize(deserializer)?
        .map(|path| tilde_expand(path, host_home_dir())))
}

fn tilde_expand(path: PathBuf, home: &HostPath) -> PathBuf {
    if let Ok(suffix) = path.strip_prefix("~") {
        home.as_host_raw().join(suffix)
    } else {
        path
    }
}

impl Config {
    /// Parses and validates a TOML-formatted string into a Config.
    fn from_str(s: &str) -> LowLevelResult<Self> {
        let config: Config = toml::from_str(s)?;

        match config.runner {
            RunnerKind::Bubblewrap => {
                if config.bubblewrap.is_none() {
                    return Err(anyhow!(
                        "Bubblewrap settings are required for that runner. \
                        See `docs/Bubblewrap.md`."
                    )
                    .into());
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
    fn deserialize_opt_duration() {
        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct Test {
            #[serde(deserialize_with = "super::deserialize_opt_duration")]
            value: Option<Duration>,
        }

        for (input, expected) in [
            ("10s", 10),
            ("0.1m", 6),
            ("3h", 3 * 60 * 60),
            ("10d", 10 * 24 * 60 * 60),
            ("10 days", 10 * 24 * 60 * 60),
            ("10.5 day", 10 * 24 * 60 * 60 + 12 * 60 * 60),
        ] {
            assert_eq!(
                toml::from_str(&format!("value = '{input}'")),
                Ok(Test {
                    value: Some(Duration::from_secs(expected))
                }),
                "deserialize_opt_duration (left is actual, right is expected)"
            );
        }

        assert_eq!(
            toml::from_str("value = 'never'"),
            Ok(Test { value: None }),
            "deserialize_opt, duration('never')"
        );
    }

    #[test]
    fn tilde_expand() {
        let home = HostPath::try_from(PathBuf::from("/home/foo")).unwrap();
        let expand = |path| super::tilde_expand(PathBuf::from(path), &home);
        assert_eq!(PathBuf::from("/a/b"), expand("/a/b"));
        assert_eq!(PathBuf::from("a/b"), expand("a/b"));
        assert_eq!(PathBuf::from("/home/foo"), expand("~"));
        assert_eq!(PathBuf::from("/home/foo/hi"), expand("~/hi"));
        assert_eq!(PathBuf::from("~bar/baz"), expand("~bar/baz"));
        assert_eq!(PathBuf::from("/home/foo/~/baz"), expand("~/~/baz"));
        assert_eq!(PathBuf::from("/~/~/baz"), expand("/~/~/baz"));
    }

    #[test]
    fn config_from_str_bad_runner() {
        assert_eq!(
            "missing field `runner`",
            Config::from_str("")
                .enough_context()
                .unwrap_err()
                .to_string()
        );
        assert_eq!(
            "unknown variant `q`, expected one of `Bubblewrap`, `Docker`, `User` for key `runner` at line 1 column 1",
            Config::from_str("runner = 'q'").enough_context().unwrap_err().to_string()
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
        .enough_context()
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "Bubblewrap settings are required")]
    fn config_from_str_missing_bubblewrap() {
        Config::from_str("runner = 'bubblewrap'")
            .enough_context()
            .unwrap();
    }

    #[test]
    fn config_from_str_ok() {
        let expected = Config {
            runner: RunnerKind::Docker,
            auto_update: twelve_hours(),
            bubblewrap: None,
            docker: Docker::default(),
        };
        assert_eq!(
            expected,
            Config::from_str("runner = 'docker'")
                .enough_context()
                .unwrap()
        );
        assert_eq!(
            expected,
            Config::from_str(
                "
                    runner = 'docker'
                    [docker]
                "
            )
            .enough_context()
            .unwrap()
        );
    }

    #[test]
    fn config_from_str_full() {
        assert_eq!(
            Config {
                runner: RunnerKind::Docker,
                auto_update: Some(Duration::from_secs(60 * 60 * 24 * 10)),
                bubblewrap: Some(Bubblewrap {
                    seccomp: PathOrDisabled::Path(PathBuf::from("/tmp/seccomp.bpf")),
                }),
                docker: Docker {
                    bind_mounts: true,
                    prefix: String::from("p"),
                    seccomp: Some(PathBuf::from("/etc/seccomp.json")),
                    strict_debian_packages: true,
                },
            },
            Config::from_str(
                "
                runner = 'docker'
                auto_update = '10d'

                [bubblewrap]
                seccomp = '/tmp/seccomp.bpf'

                [docker]
                bind_mounts = true
                prefix = 'p'
                seccomp = '/etc/seccomp.json'
                strict_debian_packages = true
                "
            )
            .enough_context()
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
            .enough_context()
            .unwrap()
            .bubblewrap
            .unwrap()
            .seccomp
        );
    }
}
