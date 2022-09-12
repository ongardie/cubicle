use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use std::rc::Rc;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use super::command_ext::Command;
use super::fs_util::{summarize_dir, DirSummary};
use super::runner::{EnvFilesSummary, EnvironmentExists, Init, Runner, RunnerCommand};
use super::{CubicleShared, EnvironmentName, ExitStatusError, HostPath};
use crate::somehow::{somehow as anyhow, Context, LowLevelResult, Result};

pub struct User {
    pub(super) program: Rc<CubicleShared>,
    username_prefix: &'static str,
    work_tars: HostPath,
}

mod newtypes {
    use super::super::newtype;
    newtype::name!(Username);
}
use newtypes::Username;

impl User {
    pub(super) fn new(program: Rc<CubicleShared>) -> Result<Self> {
        let xdg_data_home = match std::env::var("XDG_DATA_HOME") {
            Ok(path) => HostPath::try_from(path)?,
            Err(_) => program.home.join(".local").join("share"),
        };
        let work_tars = xdg_data_home.join("cubicle").join("work");

        Ok(Self {
            program,
            username_prefix: "cub-",
            work_tars,
        })
    }

    fn username_from_environment(&self, env: &EnvironmentName) -> Username {
        Username::new(format!("{}{}", self.username_prefix, env))
    }

    fn user_exists(&self, username: &Username) -> Result<bool> {
        self.user_exists_(username)
            .with_context(|| format!("failed to check if user `{username}` exists"))
    }

    fn user_exists_(&self, username: &Username) -> LowLevelResult<bool> {
        let status = Command::new("sudo")
            .args(["--user", username])
            .arg("--")
            .arg("true")
            .env_clear()
            .stderr(Stdio::null())
            .status()?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(anyhow!("`sudo --user {username} -- true` exited with {status}").into()),
        }
    }

    fn create_user(&self, username: &Username) -> Result<()> {
        self.create_user_(username)
            .with_context(|| format!("failed to create user `{username}`"))
    }

    fn create_user_(&self, username: &Username) -> LowLevelResult<()> {
        Command::new("sudo")
            .arg("--")
            .arg("adduser")
            .arg("--disabled-password")
            .args([
                "--gecos",
                &format!("Cubicle environment for user {}", self.program.user),
            ])
            .args(["--shell", &self.program.shell])
            .arg(username)
            .status()
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(anyhow!("`sudo adduser` exited with {status}"))
                }
            })?;

        Command::new("sudo")
            // See notes about `--chdir` elsewhere.
            .arg("--login")
            .args(["--user", username])
            .arg("--")
            .arg("mkdir")
            .arg("w")
            .env_clear()
            .status()
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(anyhow!("`sudo ... mkdir w` exited with {status}"))
                }
            })
            .with_context(|| format!("failed to create work directory for `{username}`"))?;

        Ok(())
    }

    fn kill_username(&self, username: &Username) -> Result<()> {
        // TODO: give processes a chance to handle SIGTERM first
        Command::new("sudo")
            .arg("--")
            .arg("pkill")
            .args(["--signal", "KILL"])
            .args(["--uid", username])
            .status()
            .and_then(|status| match status.code() {
                Some(0) | Some(1) => Ok(()),
                _ => Err(anyhow!("`sudo pkill` exited with {status}")),
            })
            .with_context(|| format!("failed to kill processes for user `{username}`"))
    }

    fn copy_in_seeds(&self, username: &Username, seeds: &[&HostPath]) -> Result<()> {
        self.copy_in_seeds_(username, seeds)
            .with_context(|| format!("failed to copy seed tarball into user `{username}`"))
    }

    fn copy_in_seeds_(&self, username: &Username, seeds: &[&HostPath]) -> LowLevelResult<()> {
        if seeds.is_empty() {
            return Ok(());
        }

        println!("Copying seed tarball");
        let mut source = Command::new("pv")
            .args(["-i", "0.1"])
            .args(seeds.iter().map(|s| s.as_host_raw()))
            .stdout(Stdio::piped())
            .scoped_spawn()?;
        let mut source_stdout = source.stdout().take().unwrap();

        let mut dest = Command::new("sudo")
            // This used to use `--chdir ~`, but that was introduced
            // relatively recently in sudo 1.9.3 (released 2020-09-21).
            // Now it uses `--login` instead, which does change directories
            // but has some other side effects.
            .arg("--login")
            .args(["--user", username])
            .arg("--")
            .arg("tar")
            .arg("--extract")
            .arg("--ignore-zero")
            .env_clear()
            .stdin(Stdio::piped())
            .scoped_spawn()?;

        {
            let mut dest_stdin = dest.stdin().take().unwrap();
            io::copy(&mut source_stdout, &mut dest_stdin)?;
            dest_stdin.flush()?;
        }

        let status = dest.wait()?;
        if !status.success() {
            return Err(anyhow!(
                "`sudo ... tar` exited with {status} while extracting tarball at destination"
            )
            .into());
        }

        let status = source.wait()?;
        if !status.success() {
            return Err(
                anyhow!("`pv` exited with {status} while reading seed tarballs at source").into(),
            );
        }

        Ok(())
    }

    fn copy_out(&self, username: &Username, path: &Path, w: &mut dyn io::Write) -> Result<()> {
        self.copy_out_(username, path, w)
            .with_context(|| format!("failed to copy file {path:?} from user `{username}`"))
    }

    fn copy_out_(
        &self,
        username: &Username,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> LowLevelResult<()> {
        let mut child = Command::new("sudo")
            // See notes about `--chdir` elsewhere.
            .arg("--login")
            .args(["--user", username])
            .arg("--")
            .arg("cat")
            .arg(path)
            .env_clear()
            .stdout(Stdio::piped())
            .scoped_spawn()?;
        let mut stdout = child.stdout().take().unwrap();
        io::copy(&mut stdout, w).todo_context()?;
        let status = child.wait().todo_context()?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("`sudo ... cat` exited with {status}").into())
        }
    }

    fn init(&self, env_name: &EnvironmentName, Init { seeds, script }: &Init) -> Result<()> {
        let username = self.username_from_environment(env_name);
        let script_tar = tempfile::NamedTempFile::new().todo_context()?;
        let mut builder = tar::Builder::new(script_tar.as_file());
        let mut script_file = std::fs::File::open(script.as_host_raw()).todo_context()?;
        builder
            .append_file(".cubicle-init-script", &mut script_file)
            .todo_context()?;
        builder
            .into_inner()
            .and_then(|mut f| f.flush())
            .todo_context()?;

        let mut seeds: Vec<&HostPath> = seeds.iter().collect();
        let script_tar_path = HostPath::try_from(script_tar.path().to_owned())?;
        seeds.push(&script_tar_path);
        self.copy_in_seeds(&username, &seeds)?;
        self.run(
            env_name,
            &RunnerCommand::Exec(&["../.cubicle-init-script".to_owned()]),
        )
    }
}

impl Runner for User {
    fn copy_out_from_home(
        &self,
        env_name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        let username = self.username_from_environment(env_name);
        self.copy_out(&username, path, w)
    }

    fn copy_out_from_work(
        &self,
        env_name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        let username = self.username_from_environment(env_name);
        self.copy_out(&username, &Path::new("w").join(path), w)
    }

    fn create(&self, env_name: &EnvironmentName, init: &Init) -> Result<()> {
        let username = self.username_from_environment(env_name);
        self.create_user(&username)?;
        self.init(env_name, init)
    }

    fn exists(&self, env_name: &EnvironmentName) -> Result<EnvironmentExists> {
        if !self.list()?.contains(env_name) {
            return Ok(EnvironmentExists::NoEnvironment);
        }
        let username = self.username_from_environment(env_name);
        if self.user_exists(&username)? {
            Ok(EnvironmentExists::FullyExists)
        } else {
            Ok(EnvironmentExists::PartiallyExists)
        }
    }

    fn list(&self) -> Result<Vec<EnvironmentName>> {
        let file = std::fs::File::open("/etc/passwd").todo_context()?;
        let reader = io::BufReader::new(file);
        let mut names = Vec::new();
        for line in reader.lines() {
            let line = line.todo_context()?;
            if let Some(env) = line
                .split_once(':')
                .and_then(|(username, _)| username.strip_prefix(self.username_prefix))
                .and_then(|env| EnvironmentName::from_str(env).ok())
            {
                names.push(env);
            }
        }
        Ok(names)
    }

    fn files_summary(&self, env_name: &EnvironmentName) -> Result<EnvFilesSummary> {
        let username = self.username_from_environment(env_name);
        let home: Option<HostPath> = {
            let file = std::fs::File::open("/etc/passwd").todo_context()?;
            let reader = io::BufReader::new(file);
            let mut home = None;
            for line in reader.lines() {
                let line = line.todo_context()?;
                let mut fields = line.split(':');
                if fields.next() != Some(&username) {
                    continue;
                }
                if let Some(h) = fields.nth(4) {
                    home = Some(HostPath::try_from(h.to_owned())?);
                }
                break;
            }
            home
        };

        match home {
            Some(home) => {
                // This should fail gracefully if this user can't read that
                // user's files. We should maybe just invoke `du` as that user,
                // but it'd need to be tolerant of different versions of `du`.
                let summary =
                    summarize_dir(&home).unwrap_or_else(|_| DirSummary::new_with_errors());
                let work_dir_path = Some(home.join("w"));
                Ok(EnvFilesSummary {
                    home_dir_path: Some(home),
                    home_dir: summary,
                    work_dir_path,
                    work_dir: DirSummary::new_with_errors(),
                })
            }
            None => Ok(EnvFilesSummary {
                home_dir_path: None,
                home_dir: DirSummary::new_with_errors(),
                work_dir_path: None,
                work_dir: DirSummary::new_with_errors(),
            }),
        }
    }

    fn stop(&self, env_name: &EnvironmentName) -> Result<()> {
        let username = self.username_from_environment(env_name);
        self.kill_username(&username)
    }

    fn reset(&self, env_name: &EnvironmentName, init: &Init) -> Result<()> {
        let username = self.username_from_environment(env_name);
        self.kill_username(&username)?;

        std::fs::create_dir_all(&self.work_tars.as_host_raw()).todo_context()?;
        let work_tar = self.work_tars.join(format!(
            "{}-{}.tar",
            env_name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        ));

        let save = || -> LowLevelResult<()> {
            println!("Saving work directory to {work_tar:?}");
            let mut child = Command::new("sudo")
                // See notes about `--chdir` elsewhere.
                .arg("--login")
                .args(["--user", &username])
                .arg("--")
                .arg("tar")
                .arg("--create")
                .arg("w")
                .env_clear()
                .stdout(Stdio::piped())
                .scoped_spawn()?;
            let mut stdout = child.stdout().take().unwrap();

            {
                let mut f = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&work_tar.as_host_raw())
                    .todo_context()?;
                io::copy(&mut stdout, &mut f).todo_context()?;
                f.flush().todo_context()?;
            }

            let status = child.wait()?;
            if status.success() {
                Ok(())
            } else {
                Err(anyhow!("`sudo ... tar` exited with {status}").into())
            }
        };
        save().with_context(|| format!("Failed to tar work directory for user `{username}`"))?;

        let purge_and_restore = || -> Result<()> {
            self.purge(env_name)?;
            self.create(env_name, init)?;
            println!("Restoring work directory from {work_tar:?}");
            self.init(
                env_name,
                &Init {
                    seeds: vec![work_tar.clone()],
                    script: self.program.script_path.join("dev-init.sh"),
                },
            )
            .with_context(|| format!("failed to restore work directory from {work_tar:?}"))
        };

        match purge_and_restore() {
            Ok(()) => {
                std::fs::remove_file(work_tar.as_host_raw()).todo_context()?;
                Ok(())
            }
            Err(e) => {
                println!("Encountered an error while resetting environment {env_name}.");
                println!("A copy of its work directory is here: {work_tar:?}");
                Err(e)
            }
        }
    }

    fn purge(&self, env_name: &EnvironmentName) -> Result<()> {
        if !self.list()?.contains(env_name) {
            return Ok(());
        }
        let username = self.username_from_environment(env_name);
        self.kill_username(&username)?;
        Command::new("sudo")
            .arg("--")
            .arg("deluser")
            .arg("--remove-home")
            .arg(&username)
            .status()
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(anyhow!("`sudo deluser` exited with {status}"))
                }
            })
            .with_context(|| format!("failed to delete user `{username}`"))
    }

    fn run(&self, env_name: &EnvironmentName, run_command: &RunnerCommand) -> Result<()> {
        let username = self.username_from_environment(env_name);

        let mut command = Command::new("sudo");
        command
            .env_clear()
            .env("SANDBOX", &env_name)
            .env("SHELL", &self.program.shell);
        if let Ok(display) = std::env::var("DISPLAY") {
            command.env("DISPLAY", display);
        }
        if let Ok(term) = std::env::var("TERM") {
            command.env("TERM", term);
        }

        command
            // This used to use `--chdir ~//w`, but that was introduced
            // relatively recently in sudo 1.9.3 (released 2020-09-21).
            //
            // The double-slash after `~` appeared to be necessary for sudo
            // (1.9.5p2). It seems dubious, though.
            .arg("--login")
            .args(["--user", &username])
            .arg("--preserve-env=SANDBOX,SHELL")
            .arg("--")
            .arg(&self.program.shell);
        match run_command {
            RunnerCommand::Interactive => {
                command.args(["-c", &format!("cd w && exec {}", self.program.shell)]);
            }
            RunnerCommand::Exec(exec) => {
                command.arg("-c");
                command.arg(format!(
                    "cd w && {}",
                    shlex::join(exec.iter().map(|a| a.as_str()))
                ));
            }
        }

        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(ExitStatusError::new(status, "sudo --user").into())
        }
    }
}
