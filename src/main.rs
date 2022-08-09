use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

struct Cubicle {
    shell: String,
    script_path: PathBuf,
    home: PathBuf,
    home_dirs: PathBuf,
    work_dirs: PathBuf,
    runner: RunnerKind,
}

impl Cubicle {
    fn new() -> Result<Cubicle> {
        let home = PathBuf::from(std::env::var("HOME")?);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/sh"));

        let xdg_cache_home = match std::env::var("XDG_CACHE_HOME") {
            Ok(path) => PathBuf::from(path),
            Err(_) => home.join(".cache"),
        };

        let xdg_data_home = match std::env::var("XDG_DATA_HOME") {
            Ok(path) => PathBuf::from(path),
            Err(_) => home.join(".local").join("share"),
        };

        let script_path = {
            let exe = std::env::current_exe()?;
            match exe.ancestors().nth(3) {
                Some(path) => path.to_owned(),
                None => {
                    return Err(anyhow!(
                        "could not find project root. binary run from unexpected location: {:?}",
                        exe
                    ))
                }
            }
        };

        let home_dirs = xdg_cache_home.join("cubicle").join("home");

        let work_dirs = xdg_data_home.join("cubicle").join("work");

        let runner = get_runner(&script_path)?;

        let program = Cubicle {
            shell,
            script_path,
            home,
            home_dirs,
            work_dirs,
            runner,
        };

        Ok(program)
    }

    fn enter_environment(&self, name: &EnvironmentName) -> Result<()> {
        if !self.work_dirs.join(name).exists() {
            return Err(anyhow!("Environment {} does not exist", name));
        }
        self.run(name)
    }

    fn run(&self, name: &EnvironmentName) -> Result<()> {
        let host_home = self.home_dirs.join(name);
        let host_work = self.work_dirs.join(name);

        fs::create_dir_all(host_home)?;

        // TODO: seeds
        let runner = match self.runner {
            RunnerKind::Bubblewrap => todo!("bubblewrap runner"),
            RunnerKind::Docker => Box::new(Docker { program: self }),
        };

        runner.run(name)
    }
}

#[derive(Debug)]
struct ExitStatusError(ExitStatus);

impl ExitStatusError {
    fn new(status: ExitStatus) -> Self {
        assert!(matches!(status.code(), Some(c) if c != 0));
        Self(status)
    }
}

impl std::error::Error for ExitStatusError {}

impl fmt::Display for ExitStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "non-zero exit status ({})", self.0.code().unwrap())
    }
}

trait Runner {
    fn run(&self, name: &EnvironmentName) -> Result<()>;
}

struct Docker<'a> {
    program: &'a Cubicle,
}

impl<'a> Runner for Docker<'a> {
    fn run(&self, name: &EnvironmentName) -> Result<()> {
        // TODO:
        // if not self.is_running(name):
        //    self.build_base()
        //    self.spawn(...)

        let fallback_path = std::env::join_paths(&[
            self.program.home.join("bin").as_path(),
            // The debian:11 image hasn't gone through usrmerge, so
            // /usr/bin and /bin are distinct there.
            Path::new("/bin"),
            Path::new("/sbin"),
            Path::new("/usr/bin"),
            Path::new("/usr/sbin"),
        ])?
        .into_string()
        .map_err(|e| anyhow!("Non-UTF8 path: {:#?}", e))?;

        let mut command = Command::new("docker");
        command.arg("exec");
        command.args(["--env", "DISPLAY"]);
        command.args(["--env", &format!("PATH={}", fallback_path)]);
        command.args(["--env", "SHELL"]);
        command.args(["--env", "TERM"]);
        command.arg("--interactive");
        command.arg("--tty");
        command.arg(name);
        command.args([&self.program.shell, "-l"]);

        let status = command.status()?;
        if !status.success() {
            Err(ExitStatusError::new(status))?;
        }
        Ok(())
    }
}

/// Manage sandboxed development environments.
#[derive(Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run a shell in an existing environment.
    Enter {
        /// Environment name.
        name: EnvironmentName,
    },
}

#[derive(Debug)]
struct EnvironmentName(String);

impl std::str::FromStr for EnvironmentName {
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

        Ok(EnvironmentName(s.to_owned()))
    }
}

impl fmt::Display for EnvironmentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::convert::AsRef<Path> for EnvironmentName {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl std::convert::AsRef<std::ffi::OsStr> for EnvironmentName {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_ref()
    }
}

enum RunnerKind {
    Bubblewrap,
    Docker,
}

fn get_runner(script_path: &Path) -> Result<RunnerKind> {
    let runner_path = script_path.join(".RUNNER");
    let runners = "'bubblewrap' or 'docker'";
    match fs::read_to_string(&runner_path)
        .with_context(|| format!("Could not read {:?}. Expected {}.", runner_path, runners))?
        .trim()
    {
        "bubblewrap" => Ok(RunnerKind::Bubblewrap),
        "docker" => Ok(RunnerKind::Docker),
        r => Err(anyhow!(
            "Unknown runner in {:?}: {:?}. Expected {}.",
            runner_path,
            r,
            runners
        )),
    }
}

fn main() -> Result<()> {
    let program = Cubicle::new()?;
    let args = Args::parse();
    match args.command {
        Commands::Enter { name } => program.enter_environment(&name),
    }
}
