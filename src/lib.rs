#![warn(
    clippy::explicit_into_iter_loop,
    clippy::explicit_iter_loop,
    clippy::if_then_some_else_none,
    clippy::implicit_clone,
    clippy::redundant_else,
    clippy::single_match_else,
    clippy::try_err,
    clippy::unreadable_literal
)]
#![warn(missing_docs)]

//! This crate is the library underneath the Cubicle command-line program.
//!
//! It is split from the main program as a generally recommended practice in
//! Rust and to allow for system-level tests. Most people should probably use
//! the command-line program instead.
//!
//! The remainder of this header reproduces the README from the command-line
//! program. Skip below to learn about the the library API.
#![doc = include_str!("../README.md")]

use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::{self, Debug, Display};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::rc::Rc;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod somehow;
pub use somehow::Result;
use somehow::{somehow as anyhow, warn, Context, Error};

mod paths;
use paths::HostPath;

pub mod config;
use config::Config;

mod randname;
use randname::RandomNameGenerator;

mod runner;
use runner::{CheckedRunner, EnvFilesSummary, EnvironmentExists, Init, Runner, RunnerCommand};

mod bytes;
use bytes::Bytes;

mod fs_util;
use fs_util::DirSummary;

mod os_util;
use os_util::{get_hostname, host_home_dir};

mod packages;
use packages::write_package_list_tar;
pub use packages::{
    FullPackageName, ListPackagesFormat, PackageDetails, PackageName, PackageNamespace,
    PackageSpec, PackageSpecs, ShouldPackageUpdate, UpdatePackagesConditions,
};

mod command_ext;

#[cfg(target_os = "linux")]
mod bubblewrap;
#[cfg(target_os = "linux")]
use bubblewrap::Bubblewrap;

mod docker;
use docker::Docker;

mod user;
use user::User;

mod apt;

/// The main Cubicle program functionality.
///
// This struct is split in two so that the runner may also keep a reference to
// `shared`.
pub struct Cubicle {
    shared: Rc<CubicleShared>,
    runner: CheckedRunner,
}

struct CubicleShared {
    config: Config,
    shell: String,
    script_name: String,
    script_path: HostPath,
    hostname: Option<String>,
    home: HostPath,
    user: String,
    package_cache: HostPath,
    code_package_dir: HostPath,
    user_package_dir: HostPath,
    random_name_gen: RandomNameGenerator,
}

/// Named boolean flag for [`Cubicle::purge_environment`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Quiet(pub bool);

impl Cubicle {
    /// Creates a new instance.
    ///
    /// Note that this function and the rest of this library may read from
    /// stdin and write to stdout and stderr. These effects are not currently
    /// modeled through the type system.
    ///
    /// # Errors
    ///
    /// - Reading and parsing environment variables.
    /// - Loading and initializing filesystem structures.
    /// - Creating a runner.
    pub fn new(config: Config) -> Result<Self> {
        let hostname = get_hostname();
        let home = host_home_dir().clone();
        let user = std::env::var("USER").context("Invalid $USER")?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/sh"));

        let xdg_cache_home = match std::env::var("XDG_CACHE_HOME") {
            Ok(path) => HostPath::try_from(path)?,
            Err(_) => home.join(".cache"),
        };

        let xdg_data_home = match std::env::var("XDG_DATA_HOME") {
            Ok(path) => HostPath::try_from(path)?,
            Err(_) => home.join(".local").join("share"),
        };

        let exe = std::env::current_exe().todo_context()?;
        let script_name = match exe.file_name() {
            Some(path) => path.to_string_lossy().into_owned(),
            None => {
                return Err(anyhow!(
                    "could not get executable name from current_exe: {:?}",
                    exe
                ));
            }
        };
        let script_path = match exe.ancestors().nth(3) {
            Some(path) => HostPath::try_from(path.to_owned())?,
            None => {
                return Err(anyhow!(
                    "could not find project root. binary run from unexpected location: {:?}",
                    exe
                ));
            }
        };

        let package_cache = xdg_cache_home.join("cubicle").join("packages");
        let code_package_dir = script_path.join("packages");
        let user_package_dir = xdg_data_home.join("cubicle").join("packages");

        let eff_word_list_dir = xdg_cache_home.join("cubicle");
        let random_name_gen = RandomNameGenerator::new(eff_word_list_dir);

        let shared = Rc::new(CubicleShared {
            config,
            shell,
            script_name,
            script_path,
            hostname,
            home,
            user,
            package_cache,
            code_package_dir,
            user_package_dir,
            random_name_gen,
        });

        let runner = CheckedRunner::new(match shared.config.runner {
            RunnerKind::Bubblewrap => {
                #[cfg(not(target_os = "linux"))]
                return Err(anyhow!("The Bubblewrap runner is only available on Linux"));
                #[cfg(target_os = "linux")]
                Box::new(Bubblewrap::new(shared.clone())?)
            }
            RunnerKind::Docker => Box::new(Docker::new(shared.clone())?),
            RunnerKind::User => Box::new(User::new(shared.clone())?),
        });

        Ok(Cubicle { runner, shared })
    }

    /// Corresponds to `cub enter`.
    pub fn enter_environment(&self, name: &EnvironmentName) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => Err(anyhow!("Environment {name} does not exist")),
            PartiallyExists => Err(anyhow!(
                "Environment {name} in broken state (try '{} reset')",
                self.shared.script_name
            )),
            FullyExists => self.runner.run(name, &RunnerCommand::Interactive),
        }
    }

    /// Corresponds to `cub exec`.
    pub fn exec_environment(&self, name: &EnvironmentName, command: &[String]) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => Err(anyhow!("Environment {name} does not exist")),
            PartiallyExists => Err(anyhow!(
                "Environment {name} in broken state (try '{} reset')",
                self.shared.script_name
            )),
            FullyExists => self.runner.run(name, &RunnerCommand::Exec(command)),
        }
    }

    /// Returns a list of existing environment names.
    pub fn get_environment_names(&self) -> Result<BTreeSet<EnvironmentName>> {
        Ok(self.runner.list()?.into_iter().collect())
    }

    /// Returns a detailed description of the current environments.
    pub fn get_environments(&self) -> Result<BTreeMap<EnvironmentName, EnvironmentDetails>> {
        Ok(self
            .get_environment_names()?
            .into_iter()
            .map(|name| {
                let summary = self.runner.files_summary(&name).unwrap_or_else(|e| {
                    warn(e.context(format!("failed to summarize disk usage for {name}")));
                    EnvFilesSummary {
                        home_dir_path: None,
                        home_dir: DirSummary::new_with_errors(),
                        work_dir_path: None,
                        work_dir: DirSummary::new_with_errors(),
                    }
                });
                (
                    name,
                    EnvironmentDetails {
                        home_dir: summary.home_dir_path.map(|p| p.as_host_raw().to_owned()),
                        home_dir_du_error: summary.home_dir.errors,
                        home_dir_size: summary.home_dir.total_size,
                        home_dir_mtime: nonzero_time(summary.home_dir.last_modified),
                        work_dir: summary.work_dir_path.map(|p| p.as_host_raw().to_owned()),
                        work_dir_du_error: summary.work_dir.errors,
                        work_dir_size: summary.work_dir.total_size,
                        work_dir_mtime: nonzero_time(summary.work_dir.last_modified),
                    },
                )
            })
            .collect())
    }

    /// Corresponds to `cub list`.
    pub fn list_environments(&self, format: ListFormat) -> Result<()> {
        match format {
            ListFormat::Names => {
                for name in self.get_environment_names()? {
                    println!("{}", name.as_str());
                }
            }

            ListFormat::Json => {
                let envs = self.get_environments()?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&envs)
                        .context("failed to serialize JSON while listing environments")?
                );
            }

            ListFormat::Default => {
                let envs = self.get_environments()?;
                let nw = envs
                    .keys()
                    .map(|name| name.as_str().len())
                    .max()
                    .unwrap_or(10);
                let now = SystemTime::now();
                println!(
                    "{:<nw$} | {:^24} | {:^24}",
                    "", "home directory", "work directory",
                );
                println!(
                    "{:<nw$} | {:>10} {:>13} | {:>10} {:>13}",
                    "name", "size", "modified", "size", "modified",
                );
                println!("{0:-<nw$} + {0:-<10} {0:-<13} + {0:-<10} {0:-<13}", "",);

                // `Bytes` doesn't implement width/alignment, so it needs an
                // extra `to_string()`.
                #[allow(clippy::to_string_in_format_args)]
                for (name, env) in envs {
                    println!(
                        "{:<nw$} | {:>9}{} {:>13} | {:>9}{} {:>13}",
                        name.as_str(),
                        Bytes(env.home_dir_size).to_string(),
                        if env.home_dir_du_error { '+' } else { ' ' },
                        match env.home_dir_mtime {
                            Some(mtime) => rel_time(now.duration_since(mtime).ok()),
                            None => String::from("N/A"),
                        },
                        Bytes(env.work_dir_size).to_string(),
                        if env.work_dir_du_error { '+' } else { ' ' },
                        match env.work_dir_mtime {
                            Some(mtime) => rel_time(now.duration_since(mtime).ok()),
                            None => String::from("N/A"),
                        },
                    );
                }
            }
        }
        Ok(())
    }

    /// Corresponds to `cub new`.
    pub fn new_environment(
        &self,
        name: &EnvironmentName,
        packages: Option<&BTreeSet<FullPackageName>>,
    ) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => {}
            PartiallyExists => {
                return Err(anyhow!(
                    "environment {name} in broken state (try '{} reset')",
                    self.shared.script_name
                ))
            }
            FullyExists => {
                return Err(anyhow!(
                    "environment {name} already exists (did you mean '{} reset'?)",
                    self.shared.script_name
                ))
            }
        }

        let default;
        let packages = match packages {
            Some(p) => p,
            None => {
                default = BTreeSet::from([FullPackageName::from_str("default").unwrap()]);
                &default
            }
        };
        let specs = self.scan_packages()?;
        self.update_packages(
            packages,
            &specs,
            UpdatePackagesConditions {
                dependencies: ShouldPackageUpdate::IfStale,
                named: ShouldPackageUpdate::IfStale,
            },
        )?;
        let packages_txt = write_package_list_tar(packages)?;
        let debian_packages = self.resolve_debian_packages(packages, &specs)?;

        let mut seeds = self.packages_to_seeds(packages)?;
        seeds.push(HostPath::try_from(packages_txt.path().to_owned())?);

        self.runner
            .create(
                name,
                &Init {
                    debian_packages: debian_packages
                        .iter()
                        .map(|name| name.as_str().to_owned())
                        .collect(),
                    env_vars: Vec::new(),
                    seeds,
                    script: self.shared.script_path.join("dev-init.sh"),
                },
            )
            .with_context(|| format!("failed to initialize new environment {name}"))
    }

    /// Corresponds to `cub tmp`.
    pub fn create_enter_tmp_environment(
        &self,
        packages: Option<&BTreeSet<FullPackageName>>,
    ) -> Result<()> {
        let name = {
            let name = self
                .shared
                .random_name_gen
                .random_name(|name| {
                    if name.starts_with("cub") {
                        // that'd be confusing
                        return Ok(false);
                    }
                    match EnvironmentName::from_str(&format!("tmp-{name}")) {
                        Ok(env) => {
                            let exists = self.runner.exists(&env)?;
                            Ok(exists == EnvironmentExists::NoEnvironment)
                        }
                        Err(_) => Ok(false),
                    }
                })
                .context("Failed to generate random environment name")?;
            EnvironmentName::from_str(&format!("tmp-{name}")).unwrap()
        };
        self.new_environment(&name, packages)?;
        self.runner.run(&name, &RunnerCommand::Interactive)
    }

    /// Corresponds to `cub purge`.
    pub fn purge_environment(&self, name: &EnvironmentName, quiet: Quiet) -> Result<()> {
        if !quiet.0 && self.runner.exists(name)? == EnvironmentExists::NoEnvironment {
            warn(anyhow!(
                "environment {name} does not exist (nothing to purge)"
            ));
        }
        // Call purge regardless in case it disagrees with `exists` and finds
        // something useful to do.
        self.runner.purge(name)?;
        Ok(())
    }

    /// Corresponds to `cub reset`.
    pub fn reset_environment(
        &self,
        name: &EnvironmentName,
        packages: Option<&BTreeSet<FullPackageName>>,
    ) -> Result<()> {
        if self.runner.exists(name)? == EnvironmentExists::NoEnvironment {
            return Err(anyhow!(
                "Environment {name} does not exist (did you mean '{} new'?)",
                self.shared.script_name,
            ));
        }

        let changed = packages.is_some();
        let packages = match packages {
            Some(packages) => packages.clone(),
            None => self
                .read_package_list_from_env(name)
                .with_context(|| format!("failed to parse `packages.txt` from {name}"))?,
        };

        let specs = self.scan_packages()?;
        self.update_packages(
            &packages,
            &specs,
            UpdatePackagesConditions {
                dependencies: ShouldPackageUpdate::IfStale,
                named: ShouldPackageUpdate::IfStale,
            },
        )?;
        let debian_packages = self.resolve_debian_packages(&packages, &specs)?;
        let mut seeds = self.packages_to_seeds(&packages)?;

        let packages_txt: tempfile::NamedTempFile;
        if changed {
            packages_txt = write_package_list_tar(&packages)?;
            seeds.push(HostPath::try_from(packages_txt.path().to_owned())?);
        }

        self.runner.reset(
            name,
            &Init {
                debian_packages: debian_packages
                    .iter()
                    .map(|name| name.as_str().to_owned())
                    .collect(),
                env_vars: Vec::new(),
                seeds,
                script: self.shared.script_path.join("dev-init.sh"),
            },
        )
    }
}

#[derive(Debug)]
struct ExitStatusError {
    status: ExitStatus,
    context: String,
}

impl ExitStatusError {
    fn new(status: ExitStatus, context: &str) -> Self {
        assert!(matches!(status.code(), Some(c) if c != 0));
        Self {
            status,
            context: context.to_owned(),
        }
    }
}

impl std::error::Error for ExitStatusError {}

impl Display for ExitStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Non-zero exit status ({}) from {}",
            self.status.code().unwrap(),
            self.context
        )
    }
}

impl From<ExitStatusError> for somehow::Error {
    fn from(error: ExitStatusError) -> somehow::Error {
        anyhow!(error)
    }
}

/// The name of a potential Cubicle sandbox/isolation environment.
///
/// Other than '-' and '_' and some non-ASCII characters, values of this type
/// may not contain whitespace or special characters.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct EnvironmentName(String);

impl EnvironmentName {
    /// Returns a string slice representing the environment name.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns a string representing the environment name for use in a domain
    /// name.
    fn as_hostname(&self) -> String {
        self.0.clone()
    }

    /// Returns a string representing the environment name for use as a
    /// filename.
    fn as_filename(&self) -> String {
        self.0.clone()
    }

    /// Returns the environment name encoded in the given filename, if valid.
    fn from_filename(s: &OsStr) -> Result<Self> {
        let s = s.to_str().ok_or_else(|| anyhow!("invalid UTF-8"))?;
        Self::from_str(s)
    }

    /// Returns the name of the environment used to build the package.
    pub fn for_builder_package(FullPackageName(ns, name): &FullPackageName) -> Self {
        Self::from_str(&if ns == &PackageNamespace::Root {
            format!("package-{}", name.as_str())
        } else {
            format!("package-{}-{}", ns.as_str(), name.as_str())
        })
        .unwrap()
    }
}

impl FromStr for EnvironmentName {
    type Err = Error;
    fn from_str(mut s: &str) -> Result<Self> {
        s = s.trim();
        if s.is_empty() {
            return Err(anyhow!("environment name cannot be empty"));
        }

        if s.contains(|c: char| {
            (c.is_ascii() && !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_'))
                || c.is_control()
                || c.is_whitespace()
        }) {
            return Err(anyhow!(
                "environment name cannot contain special characters (got {s:?})"
            ));
        }

        let path = Path::new(s);
        let mut components = path.components();
        let first = components.next();
        if components.next().is_some() {
            return Err(anyhow!("environment name cannot have slashes (got {s:?})"));
        }
        if !matches!(first, Some(std::path::Component::Normal(_))) {
            return Err(anyhow!(
                "environment name cannot manipulate path (got {s:?})"
            ));
        }

        Ok(Self(s.to_owned()))
    }
}

impl Display for EnvironmentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl std::convert::AsRef<str> for EnvironmentName {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

/// Allowed formats for [`Cubicle::list_environments`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ListFormat {
    /// Human-formatted table.
    #[default]
    Default,
    /// Detailed JSON output for machine consumption.
    Json,
    /// Newline-delimited list of environment names only.
    Names,
}

/// The type of runner to use to run isolated environments.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum RunnerKind {
    /// Use the Bubblewrap runner (Linux only).
    #[serde(alias = "bubblewrap")]
    #[serde(alias = "bwrap")]
    Bubblewrap,

    /// Use the Docker runner.
    #[serde(alias = "docker")]
    Docker,

    /// Use the system user account runner.
    #[serde(alias = "user")]
    #[serde(alias = "Users")]
    #[serde(alias = "users")]
    User,
}

fn time_serialize_opt<S>(time: &Option<SystemTime>, ser: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match time {
        Some(time) => {
            let time = time.duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
            ser.serialize_some(&time)
        }
        None => ser.serialize_none(),
    }
}

fn rel_time(duration: Option<Duration>) -> String {
    let mut duration = match duration {
        Some(duration) => duration.as_secs_f64(),
        None => return String::from("N/A"),
    };
    duration /= 60.0;
    if duration < 59.5 {
        return format!("{duration:.0} minutes");
    }
    duration /= 60.0;
    if duration < 23.5 {
        return format!("{duration:.0} hours");
    }
    duration /= 24.0;
    format!("{duration:.0} days")
}

fn nonzero_time(t: SystemTime) -> Option<SystemTime> {
    if t == UNIX_EPOCH {
        None
    } else {
        Some(t)
    }
}

/// Description of an environment as returned by [`Cubicle::get_environments`].
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct EnvironmentDetails {
    /// The path on the host of the environment's home directory, if available.
    pub home_dir: Option<PathBuf>,
    /// If true, at least one error was encountered while calculating the
    /// `home_dir_size` and `home_dir_mtime` fields.
    pub home_dir_du_error: bool,
    /// The total size in bytes of `home_dir`.
    pub home_dir_size: u64,
    /// The most recent time that `home_dir` or any file or directory within
    /// it was modified.
    #[serde(serialize_with = "time_serialize_opt")]
    pub home_dir_mtime: Option<SystemTime>,
    /// The path on the host of the environment's work directory, if available.
    pub work_dir: Option<PathBuf>,
    /// If true, at least one error was encountered while calculating the
    /// `work_dir_size` and `work_dir_mtime` fields.
    pub work_dir_du_error: bool,
    /// The total size in bytes of `work_dir`.
    pub work_dir_size: u64,
    /// The most recent time that `work_dir` or any file or directory within
    /// it was modified.
    #[serde(serialize_with = "time_serialize_opt")]
    pub work_dir_mtime: Option<SystemTime>,
}

/// These things are public out of convenience but probably shouldn't be.
#[doc(hidden)]
pub mod hidden {
    use std::path::Path;
    /// Returns the path to the home directory on the host.
    ///
    /// Panics for errors locating the home directory, such as problems reading
    /// the environment variable `HOME`.
    // Note: This is public because the `cli` mod makes use of it.
    pub fn host_home_dir() -> &'static Path {
        super::host_home_dir().as_host_raw()
    }
}
