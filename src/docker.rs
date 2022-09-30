use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use std::rc::Rc;
use std::str::FromStr;
use std::time::{Duration, UNIX_EPOCH};

use super::command_ext::Command;
use super::fs_util::{rmtree, summarize_dir, try_exists, try_iterdir, DirSummary};
use super::os_util::{get_timezone, get_uids, Uids};
use super::paths::EnvPath;
use super::runner::{EnvFilesSummary, EnvironmentExists, Init, Runner, RunnerCommand};
use super::{CubicleShared, EnvironmentName, ExitStatusError, HostPath};
use crate::somehow::{somehow as anyhow, warn, Context, LowLevelResult, Result};

mod names;
use names::{ContainerName, ImageName, VolumeName};

pub struct Docker {
    pub(super) program: Rc<CubicleShared>,
    user: String,
    uids: Uids,
    timezone: String,
    mounts: Mounts,
    base_image: ImageName,
    container_home: EnvPath,
}

enum Mounts {
    BindMounts {
        home_dirs: HostPath,
        work_dirs: HostPath,
    },
    Volumes,
}

enum EnvMounts {
    BindMounts {
        host_home: HostPath,
        host_work: HostPath,
    },
    Volumes {
        home_volume: VolumeName,
        work_volume: VolumeName,
    },
}

impl Docker {
    pub(super) fn new(program: Rc<CubicleShared>) -> Result<Self> {
        let host_user = std::env::var("USER").context("Invalid $USER")?;
        let (user, uids) = if host_user == "root" {
            (
                String::from("cubicle"),
                Uids {
                    real_user: 1000,
                    group: 1000,
                },
            )
        } else {
            (host_user, get_uids())
        };

        let timezone = get_timezone();

        let mounts = if program.config.docker.bind_mounts {
            let xdg_cache_home = match std::env::var("XDG_CACHE_HOME") {
                Ok(path) => HostPath::try_from(path)?,
                Err(_) => program.home.join(".cache"),
            };
            let home_dirs = xdg_cache_home.join("cubicle").join("home");

            let xdg_data_home = match std::env::var("XDG_DATA_HOME") {
                Ok(path) => HostPath::try_from(path)?,
                Err(_) => program.home.join(".local").join("share"),
            };
            let work_dirs = xdg_data_home.join("cubicle").join("work");
            Mounts::BindMounts {
                home_dirs,
                work_dirs,
            }
        } else {
            Mounts::Volumes
        };

        let base_image = ImageName::new(format!("{}cubicle-base", program.config.docker.prefix));

        let container_home = EnvPath::try_from(String::from("/home"))
            .unwrap()
            .join(&user);

        if let Some(path) = &program.config.docker.seccomp {
            // Better give an early error message if this isn't configured right.
            std::fs::metadata(path)
                .with_context(|| format!("could not read Docker seccomp policy: {path:?}"))?;
        };

        Ok(Self {
            program,
            user,
            uids,
            timezone,
            mounts,
            base_image,
            container_home,
        })
    }

    fn container_from_environment(&self, env: &EnvironmentName) -> ContainerName {
        ContainerName::new(format!(
            "{}{}",
            self.program.config.docker.prefix,
            env.as_str()
        ))
    }

    fn mounts(&self, env: &EnvironmentName) -> EnvMounts {
        match &self.mounts {
            Mounts::BindMounts {
                home_dirs,
                work_dirs,
            } => {
                let encoded = env.as_filename();
                EnvMounts::BindMounts {
                    host_home: home_dirs.join(&encoded),
                    host_work: work_dirs.join(&encoded),
                }
            }

            Mounts::Volumes => EnvMounts::Volumes {
                home_volume: VolumeName::new(format!(
                    "{}{}-home",
                    self.program.config.docker.prefix,
                    env.as_str()
                )),
                work_volume: VolumeName::new(format!(
                    "{}{}-work",
                    self.program.config.docker.prefix,
                    env.as_str()
                )),
            },
        }
    }

    fn is_container(&self, name: &ContainerName) -> Result<bool> {
        self.is_container_(name)
            .with_context(|| format!("failed to check if {name} is an existing container"))
    }

    fn is_container_(&self, name: &ContainerName) -> Result<bool> {
        let status = Command::new("docker")
            .arg("inspect")
            .args(["--type", "container"])
            .args(["--format", "{{ .Name }}"])
            .arg(name.encoded())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(anyhow!("`docker inspect ...` exited with {status}")),
        }
    }

    fn ps(&self) -> Result<Vec<EnvironmentName>> {
        self.ps_().context("failed to list Docker containers")
    }

    fn ps_(&self) -> LowLevelResult<Vec<EnvironmentName>> {
        let output = Command::new("docker")
            .args(["ps", "--all", "--format", "{{ .Names }}"])
            .output()?;
        let status = output.status;
        if !status.success() {
            return Err(anyhow!(
                "`docker ps` exited with {}. Output: {}",
                status,
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        let mut envs = Vec::new();
        for line in output.stdout.lines() {
            let line = line.context("could not read `docker ps` output")?;
            if let Some(container_name) = ContainerName::decode(&line) {
                if let Some(name) = container_name
                    .decoded()
                    .strip_prefix(&self.program.config.docker.prefix)
                {
                    if let Ok(env) = EnvironmentName::from_str(name) {
                        envs.push(env);
                    }
                }
            }
        }
        Ok(envs)
    }

    fn build_base(&self, debian_packages: &[String]) -> LowLevelResult<()> {
        let mut child = Command::new("docker")
            .args(["build", "--tag", &self.base_image.encoded(), "-"])
            .stdin(Stdio::piped())
            .scoped_spawn()?;

        {
            let mut stdin = child.stdin().take().unwrap();
            let mut packages: BTreeSet<&str> = BASE_PACKAGES.iter().copied().collect();
            packages.extend(debian_packages.iter().map(String::as_str));
            write_dockerfile(
                &mut stdin,
                DockerfileArgs {
                    packages: &packages,
                    timezone: &self.timezone,
                    user: &self.user,
                    uids: &self.uids,
                },
            )
            .and_then(|_| stdin.flush())
            .context("failed to write Dockerfile for base image")?;
        }

        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow!("`docker build` exited with {status}").into());
        }
        Ok(())
    }

    fn spawn(&self, env_name: &EnvironmentName) -> LowLevelResult<()> {
        let container_name = self.container_from_environment(env_name);

        let mut command = Command::new("docker");
        command.arg("run");
        command.arg("--detach");
        command.args(["--env", &format!("CUBICLE={}", env_name.as_str())]);
        command.arg("--init");
        command.args(["--name", &container_name.encoded()]);
        command.arg("--rm");
        if let Some(seccomp_json) = &self.program.config.docker.seccomp {
            command.args([
                "--security-opt",
                &format!("seccomp={}", seccomp_json.display()),
            ]);
        }
        // The default `/dev/shm` is limited to only 64 MiB under
        // Docker (v20.10.5), which causes many crashes in Chromium
        // and Electron-based programs. See
        // <https://github.com/ongardie/cubicle/issues/3>.
        command.args(["--shm-size", &1_000_000_000.to_string()]);
        command.args(["--user", &self.user]);

        command.args(["--volume", "/tmp/.X11-unix:/tmp/.X11-unix:ro"]);

        let container_home_str = self
            .container_home
            .as_env_raw()
            .to_str()
            .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", self.program.home))?;

        let container_work = self.container_home.join("w");
        let container_work_str = container_work
            .as_env_raw()
            .to_str()
            .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", container_work))?;

        match &self.mounts(env_name) {
            EnvMounts::BindMounts {
                host_home,
                host_work,
            } => {
                command.args([
                    "--mount",
                    &format!(
                        r#""type=bind","source={}","target={}""#,
                        host_home
                            .as_host_raw()
                            .to_str()
                            .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", host_home))?,
                        container_home_str,
                    ),
                ]);
                command.args([
                    "--mount",
                    &format!(
                        r#""type=bind","source={}","target={}""#,
                        host_work
                            .as_host_raw()
                            .to_str()
                            .ok_or_else(|| anyhow!("path not valid UTF-8: {:#?}", host_work))?,
                        container_work_str,
                    ),
                ]);
            }

            EnvMounts::Volumes {
                home_volume,
                work_volume,
            } => {
                command.args([
                    "--mount",
                    &format!(
                        r#""type=volume","source={}","target={}""#,
                        home_volume.encoded(),
                        container_home_str,
                    ),
                ]);
                command.args([
                    "--mount",
                    &format!(
                        r#""type=volume","source={}","target={}""#,
                        work_volume.encoded(),
                        container_work_str,
                    ),
                ]);
            }
        }

        command.arg("--workdir").arg(&container_work.as_env_raw());
        command.arg(self.base_image.encoded());
        command.args(["sleep", "90d"]);
        command.stdout(Stdio::null());
        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(ExitStatusError::new(status, "docker run").into())
        }
    }

    fn init(
        &self,
        env_name: &EnvironmentName,
        Init {
            debian_packages,
            env_vars,
            seeds,
        }: &Init,
    ) -> Result<()> {
        let container_name = self.container_from_environment(env_name);
        self.build_base(debian_packages)
            .with_context(|| format!("failed to build {} Docker image", self.base_image))?;
        self.spawn(env_name)
            .with_context(|| format!("failed to start Docker container {container_name}"))?;

        let script_path = "../.cubicle-init";

        let copy_init = || -> Result<()> {
            let mut child = Command::new("docker")
                .arg("exec")
                .arg("--interactive")
                .arg(container_name.encoded())
                .args([
                    "sh",
                    "-c",
                    &format!("cat > '{script_path}' && chmod +x '{script_path}'"),
                ])
                .stdin(Stdio::piped())
                .scoped_spawn()?;

            {
                let mut stdin = child.stdin().take().unwrap();
                stdin
                    .write_all(self.program.env_init_script)
                    .todo_context()?;
            }

            let status = child.wait()?;
            if !status.success() {
                return Err(anyhow!("`docker exec ...` exited with {status}"));
            }
            Ok(())
        };
        copy_init().with_context(|| {
            format!("failed to copy init script into Docker container {container_name}")
        })?;

        self.copy_seeds(&container_name, seeds).with_context(|| {
            format!("failed to copy package seeds into Docker container {container_name}")
        })?;

        self.run_(
            env_name,
            &RunnerCommand::Exec {
                command: &[script_path.to_owned()],
                env_vars,
            },
        )
    }

    fn list_volumes(&self) -> Result<Vec<VolumeName>> {
        self.list_volumes_()
            .context("failed to list Docker volumes")
    }

    fn list_volumes_(&self) -> LowLevelResult<Vec<VolumeName>> {
        let output = Command::new("docker")
            .args(["volume", "ls", "--format", "{{ .Name }}"])
            .output()?;
        let status = output.status;
        if !status.success() {
            return Err(anyhow!(
                "`docker volume ls` exited with {} and output: {}",
                status,
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        output
            .stdout
            .lines()
            .filter_map(|line| {
                line.map(|name| VolumeName::decode(&name))
                    .context("failed to read `docker volume ls` output")
                    .map_err(|e| e.into())
                    .transpose()
            })
            .collect()
    }

    fn volume_exists(&self, name: &VolumeName) -> Result<bool> {
        self.volume_mountpoint(name).map(|o| o.is_some())
    }

    fn volume_mountpoint(&self, name: &VolumeName) -> Result<Option<HostPath>> {
        self.volume_mountpoint_(name)
            .with_context(|| format!("failed to get mountpoint of Docker volume {name}"))
    }

    fn volume_mountpoint_(&self, name: &VolumeName) -> LowLevelResult<Option<HostPath>> {
        let output = Command::new("docker")
            .arg("volume")
            .arg("inspect")
            .args(["--format", "{{ .Mountpoint }}"])
            .arg(name.encoded())
            .output()?;
        let status = output.status;
        if !status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if status.code() == Some(1) && stderr.starts_with("Error: No such volume") {
                return Ok(None);
            }
            return Err(anyhow!(
                "`docker volume inspect` exited with {status} and stderr: {stderr}"
            )
            .into());
        }
        let stdout = String::from_utf8(output.stdout)
            .context("failed to read `docker volume inspect` output")?
            .trim()
            .to_owned();
        Ok(Some(HostPath::try_from(stdout)?))
    }

    fn volume_du(&self, name: &VolumeName) -> Result<DirSummary> {
        self.volume_du_(name)
            .with_context(|| format!("failed to summarize disk usage of Docker volume {name}"))
    }
    fn volume_du_(&self, name: &VolumeName) -> LowLevelResult<DirSummary> {
        let output = Command::new("docker")
            .arg("run")
            .arg("--mount")
            .arg(format!(
                r#""type=volume","source={}","target=/v""#,
                name.encoded()
            ))
            .arg("--rm")
            .arg("debian:11")
            .arg("du")
            .arg("--block-size=1")
            .arg("--summarize")
            .arg("--time")
            .arg("--time-style=+%s")
            .arg("/v")
            .output()?;

        // ignore permissions errors
        let errors = !&output.stderr.is_empty();

        let status = output.status;
        if !status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(anyhow!(
                "`docker run ... -- du ...` exited with {status} and stderr: {stderr}",
            )
            .into());
        }

        let stdout = String::from_utf8(output.stdout)
            .context("failed to read `docker run ... -- du ...` output")?;

        lazy_static! {
            static ref RE: Regex =
                RegexBuilder::new(r#"^(?P<size>[0-9]+)\t(?P<mtime>[0-9]+)\t/v$"#)
                    .build()
                    .unwrap();
        }
        match RE.captures(stdout.trim_end()) {
            Some(caps) => {
                let size = caps.name("size").unwrap().as_str();
                let size = u64::from_str(size).unwrap();
                let mtime = caps.name("mtime").unwrap().as_str();
                let mtime = u64::from_str(mtime).unwrap();
                let mtime = UNIX_EPOCH + Duration::from_secs(mtime);
                Ok(DirSummary {
                    errors,
                    total_size: size,
                    last_modified: mtime,
                })
            }
            None => {
                Err(anyhow!("unexpected output from `docker run ... -- du ...`: {stdout:?}").into())
            }
        }
    }

    fn ensure_volume_exists(&self, name: &VolumeName) -> Result<()> {
        self.ensure_volume_exists_(name)
            .with_context(|| format!("failed to create Docker volume {name}"))
    }

    fn ensure_volume_exists_(&self, name: &VolumeName) -> LowLevelResult<()> {
        let status = Command::new("docker")
            .arg("volume")
            .arg("create")
            .arg(name.encoded())
            .stdout(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(anyhow!("`docker volume create` exited with {status}").into());
        }
        Ok(())
    }

    fn ensure_no_volume(&self, name: &VolumeName) -> Result<()> {
        self.ensure_no_volume_(name)
            .with_context(|| format!("failed to remove Docker volume {name}"))
    }

    fn ensure_no_volume_(&self, name: &VolumeName) -> LowLevelResult<()> {
        let status = Command::new("docker")
            .arg("volume")
            .arg("rm")
            .arg("--force")
            .arg(name.encoded())
            .stdout(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(anyhow!("`docker volume rm` exited with {status}").into());
        }
        Ok(())
    }

    fn copy_out_from_volume(
        &self,
        volume: &VolumeName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> LowLevelResult<()> {
        // Note: This used to use `docker cp`. That's a bit annoying because (1) it
        // requires a container to exist, and (2) Docker creates a tarfile when
        // using stdout.
        let mut child = Command::new("docker")
            .arg("run")
            .arg("--mount")
            .arg(format!(
                r#""type=volume","source={}","target=/v""#,
                volume.encoded()
            ))
            .arg("--rm")
            .args(["--workdir", "/v"])
            .arg("debian:11")
            .arg("cat")
            .arg(path)
            .stdout(Stdio::piped())
            .scoped_spawn()?;

        let mut stdout = child.stdout().take().unwrap();
        io::copy(&mut stdout, w).context("error reading/writing data")?;

        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow!("`docker run ... cat` exited with {status}").into());
        }
        Ok(())
    }

    fn copy_seeds(
        &self,
        container_name: &ContainerName,
        seeds: &Vec<HostPath>,
    ) -> LowLevelResult<()> {
        if seeds.is_empty() {
            return Ok(());
        }
        println!("Copying/extracting seed tarball");

        // Use pv from inside the container since it may not be
        // installed on the host. Since it's reading from a stream, it
        // needs to know the total size to display a good progress bar.
        #[cfg(not(unix))]
        let size: Option<u64> = None;
        #[cfg(unix)]
        let size: Option<u64> = Some({
            let mut size: u64 = 0;
            for path in seeds {
                use std::os::unix::fs::MetadataExt;
                let metadata = std::fs::metadata(path.as_host_raw()).todo_context()?;
                size += metadata.size();
            }
            size
        });

        let mut child = Command::new("docker")
            .arg("exec")
            .arg("--interactive")
            .arg(container_name.encoded())
            .args([
                "sh",
                "-c",
                &format!(
                    "pv --interval 0.1 --force {} | \
                    tar --ignore-zero --directory ~ --extract",
                    match size {
                        Some(size) => format!("--size {size}"),
                        None => String::new(),
                    },
                ),
            ])
            .stdin(Stdio::piped())
            .scoped_spawn()?;

        {
            let mut stdin = child.stdin().take().unwrap();
            for path in seeds {
                let mut file = std::fs::File::open(path.as_host_raw()).todo_context()?;
                io::copy(&mut file, &mut stdin).todo_context()?;
            }
        }

        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow!("`docker exec ... -- 'pv | tar'` exited with {status}").into());
        }
        Ok(())
    }

    fn run_(&self, env_name: &EnvironmentName, run_command: &RunnerCommand) -> Result<()> {
        let container_name = self.container_from_environment(env_name);
        assert!(self.is_container(&container_name)?);

        let mut command = Command::new("docker");
        command.arg("exec");

        command.args(["--env", "DISPLAY"]);
        command.args(["--env", "LANG"]);
        command
            .arg("--env")
            .arg(fallback_path(&self.container_home));
        command.args(["--env", "SHELL"]);
        command.args(["--env", "USER"]);
        command.args(["--env", "TERM"]);
        match run_command {
            RunnerCommand::Interactive => {}
            RunnerCommand::Exec { env_vars, .. } => {
                for (var, value) in env_vars.iter() {
                    command.arg("--env").arg(format!("{}={}", var, value));
                }
            }
        }

        command.arg("--interactive");

        // If we really don't have a TTY, Docker will exit with status 1 when
        // we request one.
        if atty::is(atty::Stream::Stdin)
            || atty::is(atty::Stream::Stdout)
            || atty::is(atty::Stream::Stderr)
        {
            command.arg("--tty");
        }

        command.arg(container_name.encoded());
        command.args([&self.program.shell, "-l"]);
        match run_command {
            RunnerCommand::Interactive => {}
            RunnerCommand::Exec { command: exec, .. } => {
                command.arg("-c");
                command.arg(shlex::join(exec.iter().map(|a| a.as_str())));
            }
        }

        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(ExitStatusError::new(status, "docker exec").into())
        }
    }
}

impl Runner for Docker {
    fn copy_out_from_home(
        &self,
        env_name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        match &self.mounts(env_name) {
            EnvMounts::BindMounts { host_home, .. } => {
                let home_dir = cap_std::fs::Dir::open_ambient_dir(
                    host_home.as_host_raw(),
                    cap_std::ambient_authority(),
                )
                .with_context(|| format!("failed to open directory {host_home}"))?;
                let mut file = home_dir
                    .open(path)
                    .with_context(|| format!("failed to open file {}", host_home.join(path)))?;
                io::copy(&mut file, w).context("failed to copy data")?;
                Ok(())
            }

            EnvMounts::Volumes { home_volume, .. } => self
                .copy_out_from_volume(home_volume, path, w)
                .enough_context(),
        }
    }

    fn copy_out_from_work(
        &self,
        env_name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        match &self.mounts(env_name) {
            EnvMounts::BindMounts { host_work, .. } => {
                let work_dir = cap_std::fs::Dir::open_ambient_dir(
                    host_work.as_host_raw(),
                    cap_std::ambient_authority(),
                )
                .with_context(|| format!("failed to open directory {host_work}"))?;
                let mut file = work_dir
                    .open(path)
                    .with_context(|| format!("failed to open file {}", host_work.join(path)))?;
                io::copy(&mut file, w).context("failed to copy data")?;
                Ok(())
            }

            EnvMounts::Volumes { work_volume, .. } => self
                .copy_out_from_volume(work_volume, path, w)
                .enough_context(),
        }
    }

    fn create(&self, env_name: &EnvironmentName, init: &Init) -> Result<()> {
        let container_name = self.container_from_environment(env_name);
        if self.is_container(&container_name)? {
            return Err(anyhow!("Docker container {container_name} already exists"));
        }
        match &self.mounts(env_name) {
            EnvMounts::BindMounts {
                host_home,
                host_work,
            } => {
                std::fs::create_dir_all(host_home.as_host_raw()).todo_context()?;
                std::fs::create_dir_all(host_work.as_host_raw()).todo_context()?;
            }

            EnvMounts::Volumes {
                home_volume,
                work_volume,
            } => {
                self.ensure_volume_exists(home_volume)?;
                self.ensure_volume_exists(work_volume)?;
            }
        }

        self.init(env_name, init)
    }

    fn exists(&self, env_name: &EnvironmentName) -> Result<EnvironmentExists> {
        let container_name = self.container_from_environment(env_name);
        let is_container = self.is_container(&container_name)?;

        let has_home_dir;
        let has_work_dir;
        match &self.mounts(env_name) {
            EnvMounts::BindMounts {
                host_home,
                host_work,
            } => {
                has_home_dir = try_exists(host_home).todo_context()?;
                has_work_dir = try_exists(host_work).todo_context()?;
            }

            EnvMounts::Volumes {
                home_volume,
                work_volume,
            } => {
                has_home_dir = self.volume_exists(home_volume)?;
                has_work_dir = self.volume_exists(work_volume)?;
            }
        }

        use EnvironmentExists::*;
        Ok(if has_home_dir && has_work_dir {
            FullyExists
        } else if is_container || has_home_dir || has_work_dir {
            PartiallyExists
        } else {
            NoEnvironment
        })
    }

    fn stop(&self, env_name: &EnvironmentName) -> Result<()> {
        let container_name = self.container_from_environment(env_name);
        let do_stop = || {
            let status = Command::new("docker")
                .args(["rm", "--force", &container_name.encoded()])
                .stdout(Stdio::null())
                .status()?;
            if !status.success() {
                return Err(anyhow!("`docker rm` exited with {status}"));
            }
            Ok(())
        };
        do_stop().with_context(|| format!("failed to remove Docker container {container_name}"))
    }

    fn list(&self) -> Result<Vec<EnvironmentName>> {
        let mut envs = BTreeSet::from_iter(self.ps()?);

        match &self.mounts {
            Mounts::BindMounts {
                home_dirs,
                work_dirs,
            } => {
                for name in try_iterdir(home_dirs)? {
                    let env = EnvironmentName::from_filename(&name).with_context(|| {
                        format!(
                            "error parsing environment name from path {}",
                            home_dirs.join(&name)
                        )
                    })?;
                    envs.insert(env);
                }

                for name in try_iterdir(work_dirs)? {
                    let env = EnvironmentName::from_filename(&name).with_context(|| {
                        format!(
                            "error parsing environment name from path {}",
                            work_dirs.join(&name)
                        )
                    })?;
                    envs.insert(env);
                }
            }

            Mounts::Volumes => {
                for name in self.list_volumes()? {
                    if let Some(name) = name
                        .decoded()
                        .strip_prefix(&self.program.config.docker.prefix)
                    {
                        if let Some(env) = name.strip_suffix("-home") {
                            envs.insert(EnvironmentName::from_str(env)?);
                        }
                        if let Some(env) = name.strip_suffix("-work") {
                            envs.insert(EnvironmentName::from_str(env)?);
                        }
                    }
                }
            }
        }

        Ok(Vec::from_iter(envs))
    }

    fn files_summary(&self, name: &EnvironmentName) -> Result<EnvFilesSummary> {
        match self.mounts(name) {
            EnvMounts::BindMounts {
                host_home: home_dir,
                host_work: work_dir,
            } => {
                let home_dir_exists = try_exists(&home_dir).todo_context()?;
                let home_dir_summary = if home_dir_exists {
                    summarize_dir(&home_dir)?
                } else {
                    DirSummary::new_with_errors()
                };

                let work_dir_exists = try_exists(&work_dir).todo_context()?;
                let work_dir_summary = if work_dir_exists {
                    summarize_dir(&work_dir)?
                } else {
                    DirSummary::new_with_errors()
                };

                Ok(EnvFilesSummary {
                    home_dir_path: home_dir_exists.then_some(home_dir),
                    home_dir: home_dir_summary,
                    work_dir_path: work_dir_exists.then_some(work_dir),
                    work_dir: work_dir_summary,
                })
            }

            EnvMounts::Volumes {
                home_volume,
                work_volume,
            } => Ok(EnvFilesSummary {
                home_dir_path: self.volume_mountpoint(&home_volume)?,
                home_dir: self.volume_du(&home_volume)?,
                work_dir_path: self.volume_mountpoint(&work_volume)?,
                work_dir: self.volume_du(&work_volume)?,
            }),
        }
    }

    fn reset(&self, name: &EnvironmentName, init: &Init) -> Result<()> {
        self.stop(name)?;
        match &self.mounts(name) {
            EnvMounts::BindMounts { host_home, .. } => {
                rmtree(host_home)?;
                std::fs::create_dir(host_home.as_host_raw()).todo_context()?;
            }
            EnvMounts::Volumes { home_volume, .. } => {
                self.ensure_no_volume(home_volume)?;
                self.ensure_volume_exists(home_volume)?;
            }
        }
        self.init(name, init)
    }

    fn purge(&self, name: &EnvironmentName) -> Result<()> {
        self.stop(name)?;
        match &self.mounts(name) {
            EnvMounts::BindMounts {
                host_home,
                host_work,
            } => {
                rmtree(host_home)?;
                rmtree(host_work)
            }

            EnvMounts::Volumes {
                home_volume,
                work_volume,
            } => {
                self.ensure_no_volume(home_volume)?;
                self.ensure_no_volume(work_volume)
            }
        }
    }

    fn run(&self, env_name: &EnvironmentName, run_command: &RunnerCommand) -> Result<()> {
        self.run_(env_name, run_command)
    }
}

fn fallback_path(container_home: &EnvPath) -> OsString {
    let home_bin = container_home.join("bin");
    let paths = [
        home_bin.as_env_raw(),
        // The debian:11 image hasn't gone through usrmerge, so
        // /usr/bin and /bin are distinct there.
        Path::new("/bin"),
        Path::new("/sbin"),
        Path::new("/usr/bin"),
        Path::new("/usr/sbin"),
    ];
    let joined = match std::env::join_paths(&paths)
        .with_context(|| format!("unable to add container home dir ({container_home:?}) to $PATH"))
    {
        Ok(joined) => joined,
        Err(e) => {
            warn(e);
            std::env::join_paths(&paths[1..]).unwrap()
        }
    };
    [OsStr::new("PATH="), &joined].into_iter().collect()
}

/// Debian packages that many packages might depend on for basic functionality.
/// They are installed in the CI system.
const BASE_PACKAGES: &[&str] = &[
    "bzip2",
    "ca-certificates",
    "curl",
    "git",
    "jq",
    "lz4",
    "procps",
    "pv",
    "sudo",
    "unzip",
    "vim",
    "wget",
    "xz-utils",
    "zip",
    "zstd",
];

struct DockerfileArgs<'a> {
    packages: &'a BTreeSet<&'a str>,
    timezone: &'a str,
    user: &'a str,
    uids: &'a Uids,
}

fn write_dockerfile<W: io::Write>(w: &mut W, args: DockerfileArgs) -> std::io::Result<()> {
    // Quote all the Strings that go into the file.
    let packages: Vec<String> = args
        .packages
        .iter()
        .map(|p| shlex::quote(p).into_owned())
        .collect();
    let timezone = shlex::quote(args.timezone);
    let user = shlex::quote(args.user);
    let has_apt_file = args.packages.contains("apt-file");
    let has_sudo = args.packages.contains("sudo");
    let uid = args.uids.real_user;
    let gid = args.uids.group;

    // Don't let the code below here access unquoted 'args'.
    #[allow(clippy::drop_non_drop)]
    std::mem::drop(args);

    // Note: If we wanted to trim this down even more for CI, we might be able
    // to use the '11-slim' base image here.
    writeln!(w, "FROM debian:11")?;

    // Set time zone.
    writeln!(w, "RUN echo {timezone} > /etc/timezone && \\")?;
    writeln!(
        w,
        "    ln -fs '/usr/share/zoneinfo/'{timezone} /etc/localtime"
    )?;

    // Set up a user account. Use the same UID as the host because that makes
    // the file permissions usable for bind mounts. The Debian convention is to
    // have a group with the same name as the user and put the user in it. Some
    // hosts use a GID with a small number for many users (GitHub Actions Mac
    // OS appears to have GID 20). If the group ID is taken on the Debian image
    // already, this falls back to any available GID, even if the group
    // permissions end up wonky for bind mounts.
    writeln!(
        w,
        "RUN addgroup --gid {gid} {user} || addgroup {user} && \\"
    )?;
    //
    // Prevent using gid below.
    #[allow(unused)]
    let gid: ();
    //
    writeln!(
        w,
        "    adduser --disabled-password --gecos '' --uid {uid} --ingroup {user} {user} && \\",
    )?;
    writeln!(w, "    adduser {user} sudo && \\")?;
    // For a Docker volume to be owned/writable by a regular user, a directory
    // needs to exist there before the volume is mounted. See
    // <https://github.com/moby/moby/issues/2259>.
    writeln!(w, "    mkdir /home/{user}/w && \\")?;
    writeln!(w, "    chown {user}:{user} /home/{user}/w")?;

    // Configure and Update apt.
    writeln!(
        w,
        r#"RUN sed -i 's/ main$/ main contrib non-free/' /etc/apt/sources.list"#
    )?;
    writeln!(w, "RUN apt-get update && apt-get upgrade --yes")?;

    // Install requested packages.
    if let Some((last, init)) = packages.split_last() {
        writeln!(w, "RUN apt-get install --no-install-recommends --yes \\")?;
        for package in init {
            writeln!(w, "    {package} \\")?;
        }
        writeln!(w, "    {last}")?;
    }

    // Update lists of package contents (after 'apt-file' is installed).
    if has_apt_file {
        writeln!(w, "RUN apt-file update")?;
    }

    // Configure sudo (after 'sudo' is installed, which creates the directory
    // with the right permissions).
    if has_sudo {
        writeln!(
            w,
            r#"RUN sh -c 'echo "Defaults umask = 0027" > /etc/sudoers.d/umask' && \"#
        )?;
        writeln!(
            w,
            r#"    sh -c 'echo "%sudo ALL=(ALL) CWD=* NOPASSWD: ALL" > /etc/sudoers.d/nopasswd'"#
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use std::path::PathBuf;

    #[test]
    fn fallback_path() {
        assert_snapshot!(
            super::fallback_path(&EnvPath::try_from(PathBuf::from("/home/foo")).unwrap()).to_string_lossy(),
            @"PATH=/home/foo/bin:/bin:/sbin:/usr/bin:/usr/sbin"
        );
        assert_snapshot!(
            super::fallback_path(&EnvPath::try_from(PathBuf::from("/home/fo:oo")).unwrap()).to_string_lossy(),
            @"PATH=/bin:/sbin:/usr/bin:/usr/sbin"
        );
    }

    #[test]
    fn write_dockerfile() {
        let mut buf: Vec<u8> = Vec::new();
        super::write_dockerfile(
            &mut buf,
            DockerfileArgs {
                packages: &BTreeSet::from(["apt-file", "pack#age1", "package2", "sudo"]),
                timezone: "Etc/Timez'one",
                user: "h#x*r",
                uids: &Uids {
                    real_user: 1337,
                    group: 7331,
                },
            },
        )
        .unwrap();
        let dockerfile = String::from_utf8(buf).unwrap();
        assert_snapshot!("Dockerfile", dockerfile);
    }
}
