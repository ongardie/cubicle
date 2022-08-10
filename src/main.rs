use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Cubicle {
    shell: String,
    script_path: PathBuf,
    home: PathBuf,
    home_dirs: PathBuf,
    work_dirs: PathBuf,
    timezone: String,
    user: String,
    runner: RunnerKind,
}

impl Cubicle {
    fn new() -> Result<Cubicle> {
        let home = PathBuf::from(std::env::var("HOME").context("Invalid $HOME")?);
        let user = std::env::var("USER").context("Invalid $USER")?;
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

        let timezone = match fs::read_to_string("/etc/timezone") {
            Ok(s) => s.trim().to_owned(),
            Err(e) => {
                println!(
                    "Warning: Falling back to UTC due to failure reading /etc/timezone: {}",
                    e
                );
                String::from("Etc/UTC")
            }
        };

        let runner = get_runner(&script_path)?;

        let program = Cubicle {
            shell,
            script_path,
            home,
            home_dirs,
            work_dirs,
            timezone,
            user,
            runner,
        };

        Ok(program)
    }

    fn enter_environment(&self, name: &EnvironmentName) -> Result<()> {
        if !self.work_dirs.join(name).exists() {
            return Err(anyhow!("Environment {} does not exist", name));
        }
        self.run(
            name,
            &RunArgs {
                command: RunCommand::Interactive,
            },
        )
    }

    fn exec_environment(&self, name: &EnvironmentName, command: &[String]) -> Result<()> {
        if !self.work_dirs.join(name).exists() {
            return Err(anyhow!("Environment {} does not exist", name));
        }
        self.run(
            name,
            &RunArgs {
                command: RunCommand::Exec(command),
            },
        )
    }

    fn list_environments(&self, format: ListFormat) -> Result<()> {
        if format == ListFormat::Names {
            // fast path for shell completions
            let readdir = fs::read_dir(&self.work_dirs);
            if matches!(&readdir, Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
                return Ok(());
            };
            let mut names = readdir?
                .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().to_string()))
                .collect::<std::io::Result<Vec<_>>>()?;
            names.sort_unstable();
            for name in names {
                println!("{}", name);
            }
            return Ok(());
        }
        todo!("list format={:#?}", format);
    }

    fn run(&self, name: &EnvironmentName, args: &RunArgs) -> Result<()> {
        let host_home = self.home_dirs.join(name);
        let host_work = self.work_dirs.join(name);

        fs::create_dir_all(&host_home)?;

        // TODO: seeds
        let runner = match self.runner {
            RunnerKind::Bubblewrap => todo!("bubblewrap runner"),
            RunnerKind::Docker => Box::new(Docker { program: self }),
        };

        runner.run(
            name,
            &RunnerRunArgs {
                command: args.command,
                host_home: &host_home,
                host_work: &host_work,
            },
        )
    }
}

struct RunArgs<'a> {
    command: RunCommand<'a>,
}

#[derive(Clone, Copy)]
enum RunCommand<'a> {
    Interactive,
    Init(&'a Path),
    Exec(&'a [String]),
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
    fn kill(&self, name: &EnvironmentName) -> Result<()>;
    fn run(&self, name: &EnvironmentName, args: &RunnerRunArgs) -> Result<()>;
}

struct RunnerRunArgs<'a> {
    command: RunCommand<'a>,
    host_home: &'a Path,
    host_work: &'a Path,
}

struct Docker<'a> {
    program: &'a Cubicle,
}

impl<'a> Docker<'a> {
    fn is_running(&self, name: &EnvironmentName) -> Result<bool> {
        let status = Command::new("docker")
            .args(["inspect", "--type", "container", name.as_ref()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    fn base_mtime(&self) -> Result<Option<SystemTime>> {
        let mut command = Command::new("docker");
        command.arg("inspect");
        command.args(["--type", "image"]);
        command.args(["--format", "{{ $.Metadata.LastTagTime.Unix }}"]);
        command.arg("cubicle-base");
        let output = command.output()?;
        let status = output.status;
        if !status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if status.code() == Some(1) && stderr == "Error: No such image: cubicle-base" {
                return Ok(None);
            }
            return Err(anyhow!(
                "failed to get last build time for cubicle-base Docker image: \
                docker inspect exited with status {:#?} and output: {}",
                status.code(),
                stderr
            ));
        }
        let timestamp: String = String::from_utf8(output.stdout)?;
        let timestamp: u64 = u64::from_str(timestamp.trim())?;
        Ok(Some(UNIX_EPOCH + Duration::from_secs(timestamp)))
    }

    fn build_base(&self) -> Result<()> {
        let dockerfile_path = self.program.script_path.join("Dockerfile.in");
        let base_mtime = self.base_mtime()?.unwrap_or(UNIX_EPOCH);
        let image_fresh =
            base_mtime.elapsed().unwrap_or(Duration::ZERO) < Duration::from_secs(60 * 60 * 12);
        let dockerfile_mtime = fs::metadata(&dockerfile_path)?.modified()?;
        if image_fresh && dockerfile_mtime < base_mtime {
            return Ok(());
        }
        let dockerfile = fs::read_to_string(dockerfile_path)?
            .replace("@@TIMEZONE@@", &self.program.timezone)
            .replace("@@USER@@", &self.program.user);
        let mut child = Command::new("docker")
            .args(["build", "--tag", "cubicle-base", "-"])
            .stdin(Stdio::piped())
            .spawn()?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("failed to open stdin"))?;
            stdin.write_all(dockerfile.as_bytes())?;
        }

        let status = child.wait()?;
        if !status.success() {
            Err(ExitStatusError::new(status))?;
            return Err(anyhow!(
                "failed to build cubicle-base Docker image: \
                docker build exited with status {:#?}",
                status.code(),
            ));
        }
        Ok(())
    }

    fn spawn(&self, name: &EnvironmentName, args: &DockerSpawnArgs) -> Result<()> {
        let seccomp_json = self.program.script_path.join("seccomp.json");
        let mut command = Command::new("docker");
        command.arg("run");
        command.arg("--detach");
        command.args(["--env", &format!("SANDBOX={}", name)]);
        // TODO: Python version did f"{name}.{HOSTNAME}", but this isn't
        // available in Rust's stdlib.
        command.args(["--hostname", name.as_ref()]);
        command.arg("--init");
        command.args(["--name", name.as_ref()]);
        command.arg("--rm");
        if seccomp_json.exists() {
            command.args([
                "--security-opt",
                &format!(
                    "seccomp={}",
                    seccomp_json
                        .to_str()
                        .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", seccomp_json))?
                ),
            ]);
        }
        // The default `/dev/shm` is limited to only 64 MiB under
        // Docker (v20.10.5), which causes many crashes in Chromium
        // and Electron-based programs. See
        // <https://github.com/ongardie/cubicle/issues/3>.
        command.args(["--shm-size", &1_000_000_000.to_string()]);
        command.args(["--user", &self.program.user]);
        command.args(["--volume", "/tmp/.X11-unix:/tmp/.X11-unix:ro"]);
        command.args([
            "--volume",
            &format!(
                "{}:{}",
                args.host_home
                    .to_str()
                    .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", args.host_home))?,
                &self
                    .program
                    .home
                    .to_str()
                    .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", self.program.home))?,
            ),
        ]);
        let container_work = self.program.home.join(name);
        let container_work_str = container_work
            .to_str()
            .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", container_work))?;
        command.args([
            "--volume",
            &format!(
                "{}:{}",
                args.host_work
                    .to_str()
                    .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", args.host_work))?,
                container_work_str,
            ),
        ]);
        command.args(["--workdir", container_work_str]);
        command.arg("cubicle-base");
        command.args(["sleep", "90d"]);
        command.stdout(Stdio::null());
        let status = command.status()?;
        if !status.success() {
            Err(ExitStatusError::new(status))?;
        }
        Ok(())
    }
}

struct DockerSpawnArgs<'a> {
    host_home: &'a Path,
    host_work: &'a Path,
}

impl<'a> Runner for Docker<'a> {
    fn kill(&self, name: &EnvironmentName) -> Result<()> {
        if self.is_running(name)? {
            let status = Command::new("docker")
                .args(["kill", name.as_ref()])
                .stdout(Stdio::null())
                .status()?;
            if !status.success() {
                Err(ExitStatusError::new(status))?;
                return Err(anyhow!(
                    "failed to stop Docker container {}: \
                    docker kill exited with status {:#?}",
                    name,
                    status.code(),
                ));
            }
        }
        Ok(())
    }

    fn run(&self, name: &EnvironmentName, args: &RunnerRunArgs) -> Result<()> {
        if !self.is_running(name)? {
            self.build_base()?;
            self.spawn(
                name,
                &DockerSpawnArgs {
                    host_home: args.host_home,
                    host_work: args.host_work,
                },
            )?;
        }

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
        command.args(["--env", "USER"]);
        command.args(["--env", "TERM"]);
        command.arg("--interactive");
        command.arg("--tty");
        command.arg(name);
        command.args([&self.program.shell, "-l"]);
        match args.command {
            RunCommand::Interactive => {}
            RunCommand::Init(_init) => todo!("init"),
            RunCommand::Exec(exec) => {
                command.arg("-c");
                // `shlex.join` doesn't work directly since `exec` has
                // `String`s, not `str`s.
                command.arg(
                    exec.iter()
                        .map(|a| shlex::quote(a))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }

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

    /// Run a command in an existing environment.
    Exec {
        /// Environment name.
        name: EnvironmentName,
        /// Command and arguments to run.
        #[clap(last = true, required(true))]
        command: Vec<String>,
    },

    /// Show existing environments.
    List {
        /// Set output format.
        #[clap(long, value_enum, default_value_t)]
        format: ListFormat,
    },
}

#[derive(Debug)]
struct EnvironmentName(String);

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

        Ok(EnvironmentName(s.to_owned()))
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

impl std::convert::AsRef<std::ffi::OsStr> for EnvironmentName {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ValueEnum)]
enum ListFormat {
    #[default]
    Default,
    Json,
    Names,
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
    use Commands::*;
    match &args.command {
        Enter { name } => program.enter_environment(name),
        Exec { name, command } => program.exec_environment(name, command),
        List { format } => program.list_environments(*format),
    }
}
