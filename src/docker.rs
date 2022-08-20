use anyhow::{anyhow, Result};
use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::fs_util::{rmtree, summarize_dir, try_iterdir, DirSummary};
use super::runner::{EnvFilesSummary, EnvironmentExists, Runner, RunnerCommand};
use super::scoped_child::ScopedSpawn;
use super::{CubicleShared, EnvironmentName, ExitStatusError};

pub struct Docker {
    pub(super) program: Rc<CubicleShared>,
    home_dirs: PathBuf,
    work_dirs: PathBuf,
}

impl Docker {
    pub(super) fn new(program: Rc<CubicleShared>) -> Self {
        let xdg_cache_home = match std::env::var("XDG_CACHE_HOME") {
            Ok(path) => PathBuf::from(path),
            Err(_) => program.home.join(".cache"),
        };
        let home_dirs = xdg_cache_home.join("cubicle").join("home");

        let xdg_data_home = match std::env::var("XDG_DATA_HOME") {
            Ok(path) => PathBuf::from(path),
            Err(_) => program.home.join(".local").join("share"),
        };
        let work_dirs = xdg_data_home.join("cubicle").join("work");

        Self {
            program,
            home_dirs,
            work_dirs,
        }
    }

    fn is_container(&self, name: &EnvironmentName) -> Result<bool> {
        let status = Command::new("docker")
            .args(["inspect", "--type", "container", name.as_ref()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    fn ps(&self) -> Result<Vec<EnvironmentName>> {
        let output = Command::new("docker")
            .args(["ps", "--all", "--format", "{{ .Names }}"])
            .output()?;
        let status = output.status;
        if !status.success() {
            return Err(anyhow!(
                "Failed to list Docker containers: \
                docker ps exited with status {:?} and output: {}",
                status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let mut envs = Vec::new();
        for line in output.stdout.lines() {
            if let Ok(env) = EnvironmentName::from_str(&line?) {
                envs.push(env);
            }
        }
        Ok(envs)
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
                "Failed to get last build time for cubicle-base Docker image: \
                docker inspect exited with status {:?} and output: {}",
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
        let dockerfile_mtime = std::fs::metadata(&dockerfile_path)?.modified()?;
        if image_fresh && dockerfile_mtime < base_mtime {
            return Ok(());
        }
        let dockerfile = std::fs::read_to_string(dockerfile_path)?
            .replace("@@TIMEZONE@@", &self.program.timezone)
            .replace("@@USER@@", &self.program.user);
        let mut child = Command::new("docker")
            .args(["build", "--tag", "cubicle-base", "-"])
            .stdin(Stdio::piped())
            .scoped_spawn()?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("Failed to open stdin"))?;
            stdin.write_all(dockerfile.as_bytes())?;
        }

        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow!(
                "Failed to build cubicle-base Docker image: \
                docker build exited with status {:?}",
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
        command.arg("--hostname");
        match &self.program.hostname {
            Some(hostname) => command.arg(format!("{name}.{hostname}")),
            None => command.arg(name),
        };
        command.arg("--init");
        command.args(["--name", name.as_ref()]);
        command.arg("--rm");
        if seccomp_json.exists() {
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
        if status.success() {
            Ok(())
        } else {
            Err(ExitStatusError::new(status, "docker run").into())
        }
    }
}

struct DockerSpawnArgs<'a> {
    host_home: &'a Path,
    host_work: &'a Path,
}

impl Runner for Docker {
    fn copy_out_from_home(
        &self,
        name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        let home_dir = cap_std::fs::Dir::open_ambient_dir(
            &self.home_dirs.join(name),
            cap_std::ambient_authority(),
        )?;
        let mut file = home_dir.open(path)?;
        io::copy(&mut file, w)?;
        Ok(())
    }

    fn copy_out_from_work(
        &self,
        name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        let work_dir = cap_std::fs::Dir::open_ambient_dir(
            &self.work_dirs.join(name),
            cap_std::ambient_authority(),
        )?;
        let mut file = work_dir.open(path)?;
        io::copy(&mut file, w)?;
        Ok(())
    }

    fn create(&self, name: &EnvironmentName) -> Result<()> {
        if self.is_container(name)? {
            return Err(anyhow!("Docker container {name} already exists"));
        }
        std::fs::create_dir_all(&self.home_dirs)?;
        std::fs::create_dir_all(&self.work_dirs)?;
        let host_home = self.home_dirs.join(name);
        let host_work = self.work_dirs.join(name);
        std::fs::create_dir(&host_home)?;
        std::fs::create_dir(&host_work)?;
        Ok(())
    }

    fn exists(&self, name: &EnvironmentName) -> Result<EnvironmentExists> {
        let is_container = self.is_container(name)?;
        let has_home_dir = self.home_dirs.join(name).try_exists()?;
        let has_work_dir = self.work_dirs.join(name).try_exists()?;

        use EnvironmentExists::*;
        Ok(if has_home_dir && has_work_dir {
            FullyExists
        } else if is_container || has_home_dir || has_work_dir {
            PartiallyExists
        } else {
            NoEnvironment
        })
    }

    fn stop(&self, name: &EnvironmentName) -> Result<()> {
        if self.is_container(name)? {
            let status = Command::new("docker")
                .args(["rm", "--force", name.as_ref()])
                .stdout(Stdio::null())
                .status()?;
            if !status.success() {
                return Err(anyhow!(
                    "Failed to stop Docker container {}: \
                    docker rm exited with status {:?}",
                    name,
                    status.code(),
                ));
            }
        }
        Ok(())
    }

    fn list(&self) -> Result<Vec<EnvironmentName>> {
        let mut envs = BTreeSet::from_iter(self.ps()?);

        for name in try_iterdir(&self.home_dirs)? {
            let env = name
                .to_str()
                .ok_or_else(|| anyhow!("Path not UTF-8: {:?}", self.home_dirs.join(&name)))
                .and_then(EnvironmentName::from_str)?;
            envs.insert(env);
        }

        for name in try_iterdir(&self.work_dirs)? {
            let env = name
                .to_str()
                .ok_or_else(|| anyhow!("Path not UTF-8: {:?}", self.work_dirs.join(&name)))
                .and_then(EnvironmentName::from_str)?;
            envs.insert(env);
        }

        Ok(Vec::from_iter(envs))
    }

    fn files_summary(&self, name: &EnvironmentName) -> Result<EnvFilesSummary> {
        let home_dir = self.home_dirs.join(name);
        let home_dir_exists = home_dir.exists();
        let home_dir_summary = if home_dir_exists {
            summarize_dir(&home_dir)?
        } else {
            DirSummary::new_with_errors()
        };

        let work_dir = self.work_dirs.join(name);
        let work_dir_exists = work_dir.exists();
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

    fn reset(&self, name: &EnvironmentName) -> Result<()> {
        self.stop(name)?;
        let host_home = self.home_dirs.join(name);
        rmtree(&host_home)?;
        std::fs::create_dir(&host_home)?;
        Ok(())
    }

    fn purge(&self, name: &EnvironmentName) -> Result<()> {
        self.stop(name)?;
        rmtree(&self.home_dirs.join(name))?;
        rmtree(&self.work_dirs.join(name))
    }

    fn run(&self, name: &EnvironmentName, run_command: &RunnerCommand) -> Result<()> {
        let host_home = self.home_dirs.join(name);
        let host_work = self.work_dirs.join(name);

        if !self.is_container(name)? {
            self.build_base()?;
            self.spawn(
                name,
                &DockerSpawnArgs {
                    host_home: &host_home,
                    host_work: &host_work,
                },
            )?;
        }

        if let RunnerCommand::Init { script, seeds } = run_command {
            let status = Command::new("docker")
                .arg("cp")
                .arg("--archive")
                .arg(script)
                .arg(format!("{name}:/.cubicle-init"))
                .status()?;
            if !status.success() {
                return Err(anyhow!(
                    "Failed to copy init script into Docker container: \
                    docker cp exited with status {:?}",
                    status.code(),
                ));
            }

            if !seeds.is_empty() {
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
                        let metadata = std::fs::metadata(path)?;
                        size += metadata.size();
                    }
                    size
                });

                let mut child = Command::new("docker")
                    .arg("exec")
                    .arg("--interactive")
                    .arg(name)
                    .args([
                        "sh",
                        "-c",
                        &format!(
                            "pv --interval 0.1 --force {} | \
                            tar --ignore-zero --directory ~ --extract",
                            match size {
                                Some(size) => format!("--size {size}"),
                                None => String::from(""),
                            },
                        ),
                    ])
                    .stdin(Stdio::piped())
                    .scoped_spawn()?;
                {
                    let mut stdin = child
                        .stdin
                        .take()
                        .ok_or_else(|| anyhow!("failed to open stdin"))?;
                    for path in seeds {
                        let mut file = std::fs::File::open(path)?;
                        io::copy(&mut file, &mut stdin)?;
                    }
                }
                let status = child.wait()?;
                if !status.success() {
                    return Err(anyhow!(
                        "Failed to copy package seeds into Docker container: \
                        docker exec (pv | tar) exited with status {:?}",
                        status.code(),
                    ));
                }
            }
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
        match run_command {
            RunnerCommand::Interactive => {}
            RunnerCommand::Init { .. } => {
                command.args(["-c", "/.cubicle-init"]);
            }
            RunnerCommand::Exec(exec) => {
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
        if status.success() {
            Ok(())
        } else {
            Err(ExitStatusError::new(status, "docker exec").into())
        }
    }
}
