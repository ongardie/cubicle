use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Write;
use std::iter;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod bytes;
use bytes::Bytes;

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
}

fn rmtree(path: &Path) -> Result<()> {
    // This is a bit challenging for a few reasons:
    //
    // 1. Symlinks leading out of the `path` directory must not cause this
    //    function to affect files outside the `path` directory.
    //
    // 2. `remove_dir_all` won't remove the contents of read-only directories,
    //    such as Go's packages. See
    //    <https://github.com/golang/go/issues/27161>.
    //
    // 3. Docker might leave empty directories owned by root. Specifically, it
    //    seems to often leave one where a volume was mounted, like a Cubicle
    //    container's work directory within its home directory. These are
    //    removable but their permissions can't be altered.

    let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
    match dir.remove_open_dir_all() {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // continue below
        }
        Err(e) => return Err(e.into()),
    }

    fn rm_contents(dir: &cap_std::fs::Dir) -> Result<()> {
        for entry in dir.entries()? {
            let entry = entry?;
            let file_name = entry.file_name();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                let metadata = entry.metadata()?;
                let mut permissions = metadata.permissions();
                if permissions.readonly() {
                    permissions.set_readonly(false);
                    // This may fail for empty directories owned by root.
                    // Continue anyway.
                    let _ = dir.set_permissions(&file_name, permissions);
                }
                let child_dir = entry.open_dir()?;
                rm_contents(&child_dir)?;
                dir.remove_dir(file_name)?;
            } else {
                dir.remove_file(file_name)?;
            }
        }
        Ok(())
    }

    let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
    let _ = rm_contents(&dir); // ignore this error
    dir.remove_open_dir_all()?; // prefer this one
    Ok(())
}

impl Cubicle {
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
}

struct DiskUsage {
    error: bool,
    size: usize,
    mtime: SystemTime,
}

fn du(path: &Path) -> Result<DiskUsage> {
    let output = Command::new("du")
        .args(["-cs", "--block-size=1", "--time", "--time-style=+%s"])
        .arg(path)
        .output()?;
    let error = !&output.stderr.is_empty();

    let stdout = String::from_utf8(output.stdout)?;

    lazy_static! {
        static ref RE: Regex = RegexBuilder::new(r#"^(?P<size>[0-9]+)\t(?P<mtime>[0-9]+)\ttotal$"#)
            .multi_line(true)
            .build()
            .unwrap();
    }
    match RE.captures(&stdout) {
        Some(caps) => {
            let size = caps.name("size").unwrap().as_str();
            let size = usize::from_str(size).unwrap();
            let mtime = caps.name("mtime").unwrap().as_str();
            let mtime = u64::from_str(mtime).unwrap();
            let mtime = UNIX_EPOCH + Duration::from_secs(mtime);
            Ok(DiskUsage { error, size, mtime })
        }
        None => Err(anyhow!("Unexpected output from du: {:#?}", stdout)),
    }
}

fn try_iterdir(path: &Path) -> Result<Vec<OsString>> {
    let readdir = fs::read_dir(path);
    if matches!(&readdir, Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
        return Ok(Vec::new());
    };
    let mut names = readdir?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort_unstable();
    Ok(names)
}

fn time_serialize<S>(time: &Option<SystemTime>, ser: S) -> Result<S::Ok, S::Error>
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
    return format!("{duration:.0} days");
}

impl Cubicle {
    fn list_environments(&self, format: ListFormat) -> Result<()> {
        if format == ListFormat::Names {
            // fast path for shell completions
            for name in try_iterdir(&self.work_dirs)? {
                println!("{}", name.to_string_lossy());
            }
            return Ok(());
        }

        let names: Vec<EnvironmentName> = {
            let mut names = try_iterdir(&self.work_dirs)?;
            let mut home_dirs_only: Vec<OsString> = Vec::new();
            for name in try_iterdir(&self.home_dirs)? {
                if names.binary_search(&name).is_err() {
                    home_dirs_only.push(name);
                }
            }
            names.extend(home_dirs_only);
            names.sort_unstable();
            names
                .iter()
                .map(|name| EnvironmentName::from_str(name.to_string_lossy().into_owned().as_ref()))
                .collect::<Result<Vec<_>>>()?
        };

        #[derive(Debug, Serialize)]
        struct Env {
            home_dir: Option<PathBuf>,
            home_dir_du_error: bool,
            home_dir_size: Option<usize>,
            #[serde(serialize_with = "time_serialize")]
            home_dir_mtime: Option<SystemTime>,
            work_dir: Option<PathBuf>,
            work_dir_du_error: bool,
            work_dir_size: Option<usize>,
            #[serde(serialize_with = "time_serialize")]
            work_dir_mtime: Option<SystemTime>,
        }

        let envs = names.iter().map(|name| {
            let home_dir = self.home_dirs.join(name);
            let (home_dir_du_error, home_dir_size, home_dir_mtime) = match du(&home_dir) {
                Ok(DiskUsage {
                    error: true,
                    size: 0,
                    ..
                })
                | Err(_) => (true, None, None),
                Ok(DiskUsage { error, size, mtime }) => (error, Some(size), Some(mtime)),
            };
            let work_dir = self.work_dirs.join(name);
            let (work_dir_du_error, work_dir_size, work_dir_mtime) = match du(&work_dir) {
                Ok(DiskUsage {
                    error: true,
                    size: 0,
                    ..
                })
                | Err(_) => (true, None, None),
                Ok(DiskUsage { error, size, mtime }) => (error, Some(size), Some(mtime)),
            };
            (
                name,
                Env {
                    home_dir: if home_dir.exists() {
                        Some(home_dir)
                    } else {
                        None
                    },
                    home_dir_du_error,
                    home_dir_size,
                    home_dir_mtime,
                    work_dir: if work_dir.exists() {
                        Some(work_dir)
                    } else {
                        None
                    },
                    work_dir_du_error,
                    work_dir_size,
                    work_dir_mtime,
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
                    "",
                    "home directory",
                    "work directory",
                    nw = nw
                );
                println!(
                    "{:<nw$} | {:>10} {:>13} | {:>10} {:>13}",
                    "name",
                    "size",
                    "modified",
                    "size",
                    "modified",
                    nw = nw,
                );
                println!(
                    "{0:-<nw$} + {0:-<10} {0:-<13} + {0:-<10} {0:-<13}",
                    "",
                    nw = nw
                );
                for (name, env) in envs {
                    println!(
                        "{:<nw$} | {:>10} {:>13} | {:>10} {:>13}",
                        name,
                        match env.home_dir_size {
                            Some(size) => {
                                let mut size = Bytes(size as u64).to_string();
                                if env.home_dir_du_error {
                                    size.push('+');
                                }
                                size
                            }
                            None => String::from("N/A"),
                        },
                        match env.home_dir_mtime {
                            Some(mtime) => rel_time(now.duration_since(mtime).ok()),
                            None => String::from("N/A"),
                        },
                        match env.work_dir_size {
                            Some(size) => {
                                let mut size = Bytes(size as u64).to_string();
                                if env.work_dir_du_error {
                                    size.push('+');
                                }
                                size
                            }
                            None => String::from("N/A"),
                        },
                        match env.work_dir_mtime {
                            Some(mtime) => rel_time(now.duration_since(mtime).ok()),
                            None => String::from("N/A"),
                        },
                        nw = nw,
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(PartialEq)]
struct Quiet(bool);

impl Cubicle {
    fn purge_environment(&self, name: &EnvironmentName, quiet: Quiet) -> Result<()> {
        let host_home = self.home_dirs.join(name);
        let host_work = self.work_dirs.join(name);
        if !host_home.exists() && !host_work.exists() {
            if quiet == Quiet(false) {
                println!("Warning: environment {name} does not exist (nothing to purge)");
            }
            return Ok(());
        }
        self.with_runner(|runner| runner.kill(name))?;
        if host_work.exists() {
            rmtree(&host_work)?;
        }
        if host_home.exists() {
            rmtree(&host_home)?;
        }
        Ok(())
    }

    fn with_runner<F, T>(&self, func: F) -> Result<T>
    where
        F: Fn(&dyn Runner) -> Result<T>,
    {
        match self.runner {
            RunnerKind::Bubblewrap => todo!("bubblewrap runner"),
            RunnerKind::Docker => func(&Docker { program: self }),
        }
    }

    fn run(&self, name: &EnvironmentName, args: &RunArgs) -> Result<()> {
        let host_home = self.home_dirs.join(name);
        let host_work = self.work_dirs.join(name);

        fs::create_dir_all(&host_home)?;

        // TODO: seeds
        self.with_runner(|runner| {
            runner.run(
                name,
                &RunnerRunArgs {
                    command: args.command,
                    host_home: &host_home,
                    host_work: &host_work,
                },
            )
        })
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

    /// Delete environment(s) and their work directories.
    Purge {
        /// Environment name(s).
        #[clap(required(true))]
        names: Vec<EnvironmentName>,
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
        Purge { names } => {
            for name in names {
                program.purge_environment(name, Quiet(false))?;
            }
            Ok(())
        }
    }
}
