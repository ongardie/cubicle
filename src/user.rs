use anyhow::{anyhow, Result};
use std::fmt;
use std::io::{self, BufRead, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use super::fs_util::{summarize_dir, DirSummary};
use super::runner::{EnvFilesSummary, EnvironmentExists, Runner, RunnerCommand};
use super::scoped_child::ScopedSpawn;
use super::{CubicleShared, EnvironmentName, ExitStatusError};

pub struct User {
    pub(super) program: Rc<CubicleShared>,
    username_prefix: &'static str,
    work_tars: PathBuf,
}

struct Username(String);

impl fmt::Display for Username {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for Username {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl User {
    pub(super) fn new(program: Rc<CubicleShared>) -> Self {
        let xdg_data_home = match std::env::var("XDG_DATA_HOME") {
            Ok(path) => PathBuf::from(path),
            Err(_) => program.home.join(".local").join("share"),
        };
        let work_tars = xdg_data_home.join("cubicle").join("work");

        Self {
            program,
            username_prefix: "cub-",
            work_tars,
        }
    }

    fn username_from_environment(&self, env: &EnvironmentName) -> Username {
        Username(format!("{}{}", self.username_prefix, env))
    }

    fn user_exists(&self, username: &Username) -> Result<bool> {
        let status = Command::new("sudo")
            .args(["--user", username])
            .arg("--")
            .arg("true")
            .env_clear()
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(status) if status.success() => Ok(true),
            _ => Ok(false),
        }
    }

    fn create_user(&self, username: &Username) -> Result<()> {
        let status = Command::new("sudo")
            .arg("--")
            .arg("adduser")
            .arg("--disabled-password")
            .args([
                "--gecos",
                &format!("Cubicle environment for user {}", self.program.user),
            ])
            .args(["--shell", &self.program.shell])
            .arg(username.deref())
            .status()?;
        if !status.success() {
            return Err(anyhow!(
                "Failed to create user {}: \
                sudo useradd exited with status {:?}",
                username,
                status.code(),
            ));
        }

        let status = Command::new("sudo")
            .arg("--login")
            .args(["--user", username])
            .arg("--")
            .arg("mkdir")
            .arg("w")
            .env_clear()
            .status()?;
        if !status.success() {
            return Err(anyhow!(
                "Failed to create user {} work directory ~/w/: \
                sudo mkdir exited with status {:?}",
                username,
                status.code(),
            ));
        }

        Ok(())
    }

    fn kill_username(&self, username: &Username) -> Result<()> {
        // TODO: give processes a chance to handle SIGTERM first
        let _ = Command::new("sudo")
            .arg("--")
            .arg("pkill")
            .args(["--signal", "KILL"])
            .args(["--uid", username.deref()])
            .status()?;
        Ok(())
    }

    fn copy_in_seeds(&self, username: &Username, seeds: &[&Path]) -> Result<()> {
        if seeds.is_empty() {
            return Ok(());
        }

        println!("Copying seed tarball");
        let mut source = Command::new("pv")
            .args(["-i", "0.1"])
            .args(seeds)
            .stdout(Stdio::piped())
            .scoped_spawn()?;
        let mut source_stdout = source.stdout.take().unwrap();

        let mut dest = Command::new("sudo")
            .args(["--chdir", "~"])
            .args(["--user", username])
            .arg("--")
            .arg("tar")
            .arg("--extract")
            .arg("--ignore-zero")
            .env_clear()
            .stdin(Stdio::piped())
            .scoped_spawn()?;

        {
            let mut dest_stdin = dest.stdin.take().unwrap();
            io::copy(&mut source_stdout, &mut dest_stdin)?;
            dest_stdin.flush()?;
        }

        let status = dest.wait()?;
        if !status.success() {
            return Err(anyhow!(
                "Failed to copy seed tarball into user {}: \
                sudo tar exited with status {:?}",
                username,
                status.code(),
            ));
        }

        let status = source.wait()?;
        if !status.success() {
            return Err(anyhow!(
                "Failed to read seed tarballs for user {}: \
                pv exited with status {:?}",
                username,
                status.code(),
            ));
        }

        Ok(())
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
        let mut child = Command::new("sudo")
            .args(["--chdir", "~"])
            .args(["--user", &username])
            .arg("--")
            .arg("cat")
            .arg(path)
            .env_clear()
            .stdout(Stdio::piped())
            .scoped_spawn()?;
        let mut stdout = child.stdout.take().unwrap();
        io::copy(&mut stdout, w)?;
        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow!(
                "Failed to copy file {:?} from user {}: \
                sudo cat exited with status {:?}",
                path,
                username,
                status.code(),
            ));
        }
        Ok(())
    }

    fn copy_out_from_work(
        &self,
        env_name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        self.copy_out_from_home(env_name, &Path::new("w").join(path), w)
    }

    fn create(&self, env_name: &EnvironmentName) -> Result<()> {
        let username = self.username_from_environment(env_name);
        self.create_user(&username)?;
        Ok(())
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
        let file = std::fs::File::open("/etc/passwd")?;
        let reader = io::BufReader::new(file);
        let mut names = Vec::new();
        for line in reader.lines() {
            let line = line?;
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
        let home = {
            let file = std::fs::File::open("/etc/passwd")?;
            let reader = io::BufReader::new(file);
            let mut home = None;
            for line in reader.lines() {
                let line = line?;
                let mut fields = line.split(':');
                if fields.next() != Some(&username) {
                    continue;
                }
                home = fields.nth(4).map(PathBuf::from);
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

    fn reset(&self, env_name: &EnvironmentName) -> Result<()> {
        let username = self.username_from_environment(env_name);
        self.kill_username(&username)?;

        std::fs::create_dir_all(&self.work_tars)?;
        let work_tar = self.work_tars.join(format!(
            "{}-{}.tar",
            env_name,
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        ));

        println!("Saving work directory to {work_tar:?}");
        let mut child = Command::new("sudo")
            .args(["--chdir", "~"])
            .args(["--user", &username])
            .arg("--")
            .arg("tar")
            .arg("--create")
            .arg("w")
            .env_clear()
            .stdout(Stdio::piped())
            .scoped_spawn()?;
        let mut stdout = child.stdout.take().unwrap();

        {
            let mut f = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&work_tar)?;
            io::copy(&mut stdout, &mut f)?;
            f.flush()?;
        }
        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow!(
                "Failed to tar work directory for environment {}: \
                sudo tar exited with status {:?}",
                env_name,
                status.code(),
            ));
        }

        let purge_and_restore = || -> Result<()> {
            self.purge(env_name)?;
            self.create_user(&username)?;
            println!("Restoring work directory from {work_tar:?}");
            self.run(
                env_name,
                &RunnerCommand::Init {
                    seeds: vec![work_tar.clone()],
                    script: self.program.script_path.join("dev-init.sh"),
                },
            )
        };

        match purge_and_restore() {
            Ok(()) => {
                std::fs::remove_file(work_tar)?;
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
        let status = Command::new("sudo")
            .arg("--")
            .arg("deluser")
            .arg("--remove-home")
            .arg(username.deref())
            .status()?;
        if !status.success() {
            return Err(anyhow!(
                "Failed to delete user {}: \
                sudo deluser exited with status {:?}",
                username,
                status.code(),
            ));
        }
        Ok(())
    }

    fn run(&self, env_name: &EnvironmentName, run_command: &RunnerCommand) -> Result<()> {
        let username = self.username_from_environment(env_name);

        if let RunnerCommand::Init { seeds, script } = run_command {
            let script_tar = tempfile::NamedTempFile::new()?;
            let mut builder = tar::Builder::new(script_tar.as_file());
            let mut script_file = std::fs::File::open(script)?;
            builder.append_file(".cubicle-init-script", &mut script_file)?;
            builder.into_inner().and_then(|mut f| f.flush())?;

            let mut seeds: Vec<&Path> = seeds.iter().map(|p| p.as_path()).collect();
            seeds.push(script_tar.path());
            self.copy_in_seeds(&username, &seeds)?;
        }

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
            // The double-slash after `~` appears to be necessary for sudo
            // (1.9.5p2). It seems dubious, though.
            .args(["--chdir", "~//w"])
            .arg("--login")
            .args(["--user", &username])
            .arg("--preserve-env=SANDBOX,SHELL")
            .arg("--")
            .arg(&self.program.shell);
        match run_command {
            RunnerCommand::Interactive => {}
            RunnerCommand::Init { .. } => {
                command.args(["-c", "../.cubicle-init-script"]);
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
            Err(ExitStatusError::new(status, "sudo --user").into())
        }
    }
}
