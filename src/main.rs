#![warn(clippy::explicit_into_iter_loop)]
#![warn(clippy::explicit_iter_loop)]
#![warn(clippy::if_then_some_else_none)]
#![warn(clippy::implicit_clone)]
#![warn(clippy::redundant_else)]
#![warn(clippy::single_match_else)]
#![warn(clippy::try_err)]
#![warn(clippy::unreadable_literal)]

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::iter;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::rc::Rc;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod newtype;
use newtype::HostPath;

mod cli;

mod config;
use config::Config;

mod randname;
use randname::RandomNameGenerator;

mod runner;
use runner::{CheckedRunner, EnvFilesSummary, EnvironmentExists, Runner, RunnerCommand};

mod bytes;
use bytes::Bytes;

mod fs_util;
use fs_util::DirSummary;

mod packages;
use packages::{write_package_list_tar, PackageName, PackageNameSet};

mod scoped_child;

#[cfg(target_os = "linux")]
mod bubblewrap;
#[cfg(target_os = "linux")]
use bubblewrap::Bubblewrap;

mod docker;
use docker::Docker;

mod user;
use user::User;

// This struct is split in two so that the runner may also keep a reference to
// `shared`.
struct Cubicle {
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
    timezone: String,
    user: String,
    package_cache: HostPath,
    code_package_dir: HostPath,
    user_package_dir: HostPath,
    random_name_gen: RandomNameGenerator,
}

#[derive(Clone, Copy, PartialEq)]
struct Quiet(bool);

#[derive(Clone, Copy, PartialEq)]
struct Clean(bool);

fn get_hostname() -> Option<String> {
    #[cfg(unix)]
    {
        let uname = rustix::process::uname();
        if let Ok(node_name) = uname.nodename().to_str() {
            if !node_name.is_empty() {
                return Some(node_name.to_owned());
            }
        }
    }
    None
}

impl Cubicle {
    fn new(config: Config) -> Result<Self> {
        let hostname = get_hostname();
        let home = HostPath::try_from(std::env::var("HOME").context("Invalid $HOME")?)?;
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

        let exe = std::env::current_exe()?;
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

        let timezone = match std::fs::read_to_string("/etc/timezone") {
            Ok(s) => s.trim().to_owned(),
            Err(e) => {
                println!(
                    "Warning: Falling back to UTC due to failure reading /etc/timezone: {}",
                    e
                );
                String::from("Etc/UTC")
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
            timezone,
            user,
            package_cache,
            code_package_dir,
            user_package_dir,
            random_name_gen,
        });

        let runner = CheckedRunner::new(match shared.config.runner {
            #[cfg(target_os = "linux")]
            RunnerKind::Bubblewrap => Box::new(Bubblewrap::new(shared.clone())?),
            RunnerKind::Docker => Box::new(Docker::new(shared.clone())?),
            RunnerKind::User => Box::new(User::new(shared.clone())?),
        });

        Ok(Cubicle { runner, shared })
    }

    fn enter_environment(&self, name: &EnvironmentName) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => Err(anyhow!("Environment {name} does not exist")),
            PartiallyExists => Err(anyhow!(
                "Environment {name} in broken state (try '{} reset')",
                self.shared.script_name
            )),
            FullyExists => self.run(name, &RunCommand::Interactive),
        }
    }

    fn exec_environment(&self, name: &EnvironmentName, command: &[String]) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => Err(anyhow!("Environment {name} does not exist")),
            PartiallyExists => Err(anyhow!(
                "Environment {name} in broken state (try '{} reset')",
                self.shared.script_name
            )),
            FullyExists => self.run(name, &RunCommand::Exec(command)),
        }
    }

    fn list_environments(&self, format: ListFormat) -> Result<()> {
        let names = {
            let mut names = self.runner.list()?;
            names.sort_unstable();
            names
        };

        if format == ListFormat::Names {
            // fast path for shell completions
            for name in names {
                println!("{}", name);
            }
            return Ok(());
        }

        #[derive(Debug, Serialize)]
        struct Env {
            home_dir: Option<PathBuf>,
            home_dir_du_error: bool,
            home_dir_size: u64,
            #[serde(serialize_with = "time_serialize_opt")]
            home_dir_mtime: Option<SystemTime>,
            work_dir: Option<PathBuf>,
            work_dir_du_error: bool,
            work_dir_size: u64,
            #[serde(serialize_with = "time_serialize_opt")]
            work_dir_mtime: Option<SystemTime>,
        }

        let envs = names.iter().map(|name| {
            let summary = match self.runner.files_summary(name) {
                Ok(summary) => summary,
                Err(e) => {
                    println!("Warning: Failed to summarize disk usage for {name}: {e}");
                    EnvFilesSummary {
                        home_dir_path: None,
                        home_dir: DirSummary::new_with_errors(),
                        work_dir_path: None,
                        work_dir: DirSummary::new_with_errors(),
                    }
                }
            };
            (
                name,
                Env {
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
        });

        match format {
            ListFormat::Names => unreachable!("handled above"),

            ListFormat::Json => {
                let envs = envs
                    .map(|(name, value)| (name.0.clone(), value))
                    .collect::<BTreeMap<String, _>>();
                println!("{}", serde_json::to_string_pretty(&envs)?);
            }

            ListFormat::Default => {
                let nw = names
                    .iter()
                    .map(|name| name.0.len())
                    .chain(iter::once(10))
                    .max()
                    .unwrap();
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
                        name,
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

    fn new_environment(
        &self,
        name: &EnvironmentName,
        packages: Option<PackageNameSet>,
    ) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => {}
            PartiallyExists => {
                return Err(anyhow!(
                    "Environment {name} in broken state (try '{} reset')",
                    self.shared.script_name
                ))
            }
            FullyExists => {
                return Err(anyhow!(
                    "Environment {name} already exists (did you mean '{} reset'?)",
                    self.shared.script_name
                ))
            }
        }

        self.runner.create(name)?;

        let packages = match packages {
            Some(p) => p,
            None => PackageNameSet::from([PackageName::from_str("default").unwrap()]),
        };
        self.update_packages(&packages, &self.scan_packages()?)?;
        let packages_txt = write_package_list_tar(&packages)?;
        self.run(
            name,
            &RunCommand::Init {
                packages: &packages,
                extra_seeds: &[&HostPath::try_from(packages_txt.path().to_owned())?],
            },
        )
        .with_context(|| format!("Failed to initialize new environment {name}"))
    }

    fn create_enter_tmp_environment(&self, packages: Option<PackageNameSet>) -> Result<()> {
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
        self.run(&name, &RunCommand::Interactive)
    }

    fn purge_environment(&self, name: &EnvironmentName, quiet: Quiet) -> Result<()> {
        if !quiet.0 && self.runner.exists(name)? == EnvironmentExists::NoEnvironment {
            println!("Warning: environment {name} does not exist (nothing to purge)");
        }
        // Call purge regardless in case it disagrees with `exists` and finds
        // something useful to do.
        self.runner.purge(name)?;
        assert_eq!(
            self.runner.exists(name)?,
            EnvironmentExists::NoEnvironment,
            "Environment should not exist after purge"
        );
        Ok(())
    }

    fn reset_environment(
        &self,
        name: &EnvironmentName,
        packages: &Option<PackageNameSet>,
        clean: Clean,
    ) -> Result<()> {
        if self.runner.exists(name)? == EnvironmentExists::NoEnvironment {
            return Err(anyhow!(
                "Environment {name} does not exist (did you mean '{} new'?)",
                self.shared.script_name,
            ));
        }

        if clean.0 {
            return self.runner.reset(name);
        }

        let (mut changed, mut packages) = match packages {
            Some(packages) => (true, packages.clone()),
            None => match self
                .read_package_list_from_env(name)
                .with_context(|| format!("Failed to parse packages.txt from {name}"))?
            {
                None => (
                    true,
                    PackageNameSet::from([PackageName::from_str("default").unwrap()]),
                ),
                Some(packages) => (false, packages),
            },
        };

        self.runner.reset(name)?;

        match name.extract_builder_package_name() {
            None => {
                self.update_packages(&packages, &self.scan_packages()?)?;
            }
            Some(package_name) => {
                let specs = self.scan_packages()?;
                let spec = match specs.get(&package_name) {
                    Some(spec) => spec,
                    None => {
                        return Err(anyhow!("Could not find package source for {package_name}"))
                    }
                };
                let start_len = packages.len();
                packages.extend(spec.build_depends.iter().cloned());
                packages.extend(spec.depends.iter().cloned());
                changed = changed || packages.len() != start_len;
                self.update_packages(&packages, &specs)?;
                self.update_package(&package_name, spec)?;
            }
        }

        let mut extra_seeds = Vec::new();
        let packages_txt;
        let packages_txt_path;
        if changed {
            packages_txt = write_package_list_tar(&packages)?;
            packages_txt_path = HostPath::try_from(packages_txt.path().to_owned())?;
            extra_seeds.push(&packages_txt_path);
        }

        self.run(
            name,
            &RunCommand::Init {
                packages: &packages,
                extra_seeds: &extra_seeds,
            },
        )
    }

    fn run(&self, name: &EnvironmentName, command: &RunCommand) -> Result<()> {
        let runner_command = match command {
            RunCommand::Interactive => RunnerCommand::Interactive,
            RunCommand::Init {
                packages,
                extra_seeds,
            } => {
                let mut seeds = self.packages_to_seeds(packages)?;
                for seed in extra_seeds.iter() {
                    seeds.push((**seed).clone());
                }
                RunnerCommand::Init {
                    seeds,
                    script: self.shared.script_path.join("dev-init.sh"),
                }
            }
            RunCommand::Exec(cmd) => RunnerCommand::Exec(cmd),
        };

        self.runner.run(name, &runner_command)
    }
}

enum RunCommand<'a> {
    Interactive,
    Init {
        packages: &'a PackageNameSet,
        extra_seeds: &'a [&'a HostPath],
    },
    Exec(&'a [String]),
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

impl fmt::Display for ExitStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Non-zero exit status ({}) from {}",
            self.status.code().unwrap(),
            self.context
        )
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EnvironmentName(String);

impl FromStr for EnvironmentName {
    type Err = anyhow::Error;
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
                "environment name cannot contain special characters"
            ));
        }

        let path = Path::new(s);
        let mut components = path.components();
        let first = components.next();
        if components.next().is_some() {
            return Err(anyhow!("environment name cannot have slashes"));
        }
        if !matches!(first, Some(std::path::Component::Normal(_))) {
            return Err(anyhow!("environment name cannot manipulate path"));
        }

        Ok(Self(s.to_owned()))
    }
}

impl fmt::Display for EnvironmentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::convert::AsRef<str> for EnvironmentName {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl std::convert::AsRef<Path> for EnvironmentName {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl std::convert::AsRef<OsStr> for EnvironmentName {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

impl EnvironmentName {
    fn extract_builder_package_name(&self) -> Option<PackageName> {
        self.0
            .strip_prefix("package-")
            .and_then(|s| PackageName::from_str(s).ok())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ValueEnum)]
enum ListFormat {
    #[default]
    Default,
    Json,
    Names,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
enum RunnerKind {
    #[cfg(target_os = "linux")]
    #[serde(alias = "bubblewrap")]
    #[serde(alias = "bwrap")]
    Bubblewrap,
    #[serde(alias = "docker")]
    Docker,
    #[serde(alias = "user")]
    #[serde(alias = "Users")]
    #[serde(alias = "users")]
    User,
}

fn time_serialize<S>(time: &SystemTime, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let time = time.duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
    ser.serialize_f64(time)
}

fn time_serialize_opt<S>(time: &Option<SystemTime>, ser: S) -> Result<S::Ok, S::Error>
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

fn main() -> Result<()> {
    let args = cli::parse();
    let config = Config::read_from_file(args.config.as_ref())?;
    let program = Cubicle::new(config)?;
    cli::run(args, &program)
}
