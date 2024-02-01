use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::command_ext::Command;
use super::fs_util::{summarize_dir, DirSummary};
use super::runner::{EnvFilesSummary, EnvironmentExists, Init, Runner, RunnerCommand, Target};
use super::{apt, CubicleShared, EnvironmentName, ExitStatusError, HostPath};
use crate::encoding::{percent_decode, percent_encode, FilenameEncoder};
use crate::somehow::{somehow as anyhow, Context, LowLevelResult, Result};

pub struct User {
    pub(super) program: Rc<CubicleShared>,
    username_prefix: &'static str,
    work_tars: HostPath,
}

mod newtypes {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;

    /// Usernames generated using a hash function.
    ///
    /// A username can't necessarily fit all the information we'd like to
    /// encode, as usernames are required to be short on some operating
    /// systems. On Linux they're usually limited to 31 characters; see
    /// <https://systemd.io/USER_NAMES/>.
    ///
    /// Using a fixed-length hash function may also have another benefit. For
    /// software installations that hardcode their absolute paths, we can
    /// probably `sed/$PACKAGE_BUILDER_HASH/$TARGET_ENVIRONMENT_HASH/` on the
    /// package tarball with some success.
    #[derive(Debug)]
    pub struct Username(String);

    impl Username {
        pub fn new(prefix: &str, name: &str) -> Self {
            let len = prefix.len() + 24;
            let mut buf = String::with_capacity(len);
            buf.push_str(prefix);
            for byte in Sha256::new()
                // Add in a couple of things so that the hash is unlikely to be
                // found anywhere else by accident.
                .chain_update("NzSWIOeAbGN1BHJtG7Kt")
                .chain_update(prefix)
                .chain_update(name.as_bytes())
                .finalize()
                .iter()
                .take(12)
            {
                write!(buf, "{:02x}", byte).unwrap();
            }
            debug_assert!(buf.len() == len);
            Self(buf)
        }

        pub fn as_str(&self) -> &str {
            &self.0
        }
    }

    impl std::fmt::Display for Username {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            std::fmt::Debug::fmt(&self.0, f)
        }
    }
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
        Username::new(self.username_prefix, env.as_str())
    }

    fn user_exists(&self, username: &Username) -> Result<bool> {
        self.user_exists_(username)
            .with_context(|| format!("failed to check if user {username} exists"))
    }

    fn user_exists_(&self, username: &Username) -> LowLevelResult<bool> {
        let status = Command::new("sudo")
            .args(["--user", username.as_str()])
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

    fn create_user(&self, env_name: &EnvironmentName, username: &Username) -> Result<()> {
        self.create_user_(env_name, username)
            .with_context(|| format!("failed to create user {username} for environment {env_name}"))
    }

    fn create_user_(&self, env_name: &EnvironmentName, username: &Username) -> LowLevelResult<()> {
        Command::new("sudo")
            .arg("--")
            .arg("adduser")
            .arg("--disabled-password")
            .args([
                "--gecos",
                &percent_encode(env_name.as_str(), |_i, c| {
                    c.is_ascii_control() || matches!(c, ',' | ':')
                }),
            ])
            .args(["--shell", &self.program.shell])
            .arg(username.as_str())
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
            .args(["--user", username.as_str()])
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
            .with_context(|| format!("failed to create work directory for {username}"))?;

        Ok(())
    }

    fn kill_username(&self, username: &Username) -> Result<()> {
        // TODO: give processes a chance to handle SIGTERM first
        Command::new("sudo")
            .arg("--")
            .arg("pkill")
            .args(["--signal", "KILL"])
            .args(["--uid", username.as_str()])
            .status()
            .and_then(|status| match status.code() {
                Some(0) | Some(1) => Ok(()),
                _ => Err(anyhow!("`sudo pkill` exited with {status}")),
            })
            .with_context(|| format!("failed to kill processes for user {username}"))
    }

    fn copy_in_seeds(&self, username: &Username, seeds: &[&HostPath]) -> Result<()> {
        self.copy_in_seeds_(username, seeds)
            .with_context(|| format!("failed to copy seed tarball into user {username} home dir"))
    }

    fn copy_in_seeds_(&self, username: &Username, seeds: &[&HostPath]) -> LowLevelResult<()> {
        if seeds.is_empty() {
            return Ok(());
        }

        println!("Copying/extracting seed tarball");
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
            .args(["--user", username.as_str()])
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
            .with_context(|| format!("failed to copy file {path:?} from user {username}"))
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
            .args(["--user", username.as_str()])
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

    fn init(
        &self,
        env_name: &EnvironmentName,
        Init {
            debian_packages,
            env_vars,
            seeds,
        }: &Init,
    ) -> Result<()> {
        apt::check_satisfied(
            &debian_packages
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>(),
        );

        let username = self.username_from_environment(env_name);
        let script_tar = tempfile::NamedTempFile::new().todo_context()?;
        let mut builder = tar::Builder::new(script_tar.as_file());

        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o700);
        header.set_size(self.program.env_init_script.len() as u64);
        builder
            .append_data(
                &mut header,
                ".cubicle-init-script",
                self.program.env_init_script,
            )
            .todo_context()?;
        builder
            .into_inner()
            .and_then(|mut f| f.flush())
            .todo_context()?;

        let mut seeds: Vec<&HostPath> = seeds.iter().collect();
        let script_tar_path = HostPath::try_from(script_tar.path().to_owned())?;
        seeds.push(&script_tar_path);
        self.copy_in_seeds(&username, &seeds)?;
        self.run_(
            env_name,
            &RunnerCommand::Exec {
                command: &["../.cubicle-init-script".to_owned()],
                env_vars,
            },
        )
    }

    fn run_(&self, env_name: &EnvironmentName, run_command: &RunnerCommand) -> Result<()> {
        let username = self.username_from_environment(env_name);

        let mut command = Command::new("sudo");

        command
            // This used to use `--chdir ~//w`, but that was introduced
            // relatively recently in sudo 1.9.3 (released 2020-09-21).
            //
            // The double-slash after `~` appeared to be necessary for sudo
            // (1.9.5p2). It seems dubious, though.
            .arg("--login")
            .args(["--user", username.as_str()]);

        command.env_clear();
        command
            .env("CUBICLE", env_name.as_str())
            .arg("--preserve-env=CUBICLE");
        command
            .env("SHELL", &self.program.shell)
            .arg("--preserve-env=SHELL");
        for var in ["DISPLAY", "LANG", "TERM"] {
            if let Ok(value) = std::env::var(var) {
                command.env(var, value).arg(format!("--preserve-env={var}"));
            }
        }
        match run_command {
            RunnerCommand::Interactive => {}
            RunnerCommand::Exec { env_vars, .. } => {
                for (var, value) in *env_vars {
                    command.env(var, value).arg(format!("--preserve-env={var}"));
                }
            }
        }

        command.arg("--").arg(&self.program.shell);

        match run_command {
            RunnerCommand::Interactive => {
                command.args(["-c", &format!("cd w && exec {}", self.program.shell)]);
            }
            RunnerCommand::Exec { command: exec, .. } => {
                command.arg("-c");
                command.arg(format!(
                    "cd w && {}",
                    shlex::try_join(exec.iter().map(|a| a.as_str())).expect("TODO")
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
        self.create_user(env_name, &username)?;
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
        let mut envs = Vec::new();
        for account in Passwd::open()? {
            let Account {
                username, gecos, ..
            } = account?;
            if !username.starts_with(self.username_prefix) {
                continue;
            }
            let name = match gecos.split_once(',') {
                Some((a, _)) => a,
                None => &gecos,
            };
            if let Ok(env) = percent_decode(name).and_then(EnvironmentName::from_string) {
                if self.username_from_environment(&env).as_str() == username {
                    envs.push(env);
                }
            }
        }
        Ok(envs)
    }

    fn files_summary(&self, env_name: &EnvironmentName) -> Result<EnvFilesSummary> {
        let username = self.username_from_environment(env_name);

        let mut home: Option<HostPath> = None;

        for account in Passwd::open()? {
            let account = account?;
            if account.username == username.as_str() {
                home = Some(account.home);
                break;
            }
        }

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

        std::fs::create_dir_all(self.work_tars.as_host_raw()).todo_context()?;
        let work_tar = self.work_tars.join(
            FilenameEncoder::new()
                .push(env_name.as_str())
                .push("-")
                .push(&format!(
                    "{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                ))
                .push(".tar")
                .encode(),
        );

        let save = || -> LowLevelResult<()> {
            println!("Saving work directory to {work_tar}");
            let mut child = Command::new("sudo")
                // See notes about `--chdir` elsewhere.
                .arg("--login")
                .args(["--user", username.as_str()])
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
                    .open(work_tar.as_host_raw())
                    .with_context(|| format!("failed to open {work_tar} for writing"))?;
                io::copy(&mut stdout, &mut f).context("failed to copy data")?;
                f.flush().context("failed to flush data")?;
            }

            let status = child.wait()?;
            if status.success() {
                Ok(())
            } else {
                Err(anyhow!("`sudo ... tar` exited with {status}").into())
            }
        };
        save().with_context(|| {
            format!("failed to save work directory for user {username} to {work_tar}")
        })?;

        let purge_and_restore = || -> Result<()> {
            self.purge(env_name)?;
            self.create(env_name, init)?;
            println!("Restoring work directory from {work_tar}");
            self.init(
                env_name,
                &Init {
                    debian_packages: Vec::new(),
                    env_vars: Vec::new(),
                    seeds: vec![work_tar.clone()],
                },
            )
            .with_context(|| {
                format!("failed to restore work directory from {work_tar} for user {username}")
            })
        };

        match purge_and_restore() {
            Ok(()) => {
                std::fs::remove_file(work_tar.as_host_raw()).todo_context()?;
                Ok(())
            }
            Err(e) => {
                println!("Encountered an error while resetting environment {env_name}.");
                println!("A copy of its work directory is here: {work_tar}");
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
            .arg(username.as_str())
            .status()
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(anyhow!("`sudo deluser` exited with {status}"))
                }
            })
            .with_context(|| format!("failed to delete user {username}"))
    }

    fn run(&self, env_name: &EnvironmentName, run_command: &RunnerCommand) -> Result<()> {
        self.run_(env_name, run_command)
    }

    fn supports_any(&self, targets: &[Target]) -> Result<bool> {
        Ok(targets.iter().any(|Target { arch, os }| {
            (match arch {
                None => true,
                Some(arch) => arch == std::env::consts::ARCH,
            }) && (match os {
                None => true,
                Some(os) => os == std::env::consts::OS,
            })
        }))
    }
}

/// An iterator over `/etc/passwd` accounts.
struct Passwd {
    lines: std::iter::Enumerate<std::io::Lines<io::BufReader<std::fs::File>>>,
}

impl Passwd {
    fn open() -> Result<Self> {
        let file = std::fs::File::open("/etc/passwd").context("failed to open \"/etc/passwd\"")?;
        let reader = io::BufReader::new(file);
        Ok(Self {
            lines: reader.lines().enumerate(),
        })
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct Account {
    username: String,
    uid: u32,
    gid: u32,
    gecos: String,
    home: HostPath,
    shell: String,
}

impl Iterator for Passwd {
    type Item = Result<Account>;
    fn next(&mut self) -> Option<Result<Account>> {
        self.lines.next().map(|(i, line)| {
            line.enough_context()
                .and_then(|line: String| -> Result<Account> {
                    let mut fields = line.split(':');
                    if let (
                        Some(username),
                        Some(_password),
                        Some(uid),
                        Some(gid),
                        Some(gecos),
                        Some(home),
                    ) = (
                        fields.next(),
                        fields.next(),
                        fields.next(),
                        fields.next(),
                        fields.next(),
                        fields.next(),
                    ) {
                        Ok(Account {
                            username: username.to_owned(),
                            uid: uid.parse::<u32>().context("error parsing uid")?,
                            gid: gid.parse::<u32>().context("error parsing gid")?,
                            gecos: gecos.to_owned(),
                            home: HostPath::try_from(home.to_owned())
                                .context("error parsing home path")?,
                            shell: fields.next().unwrap_or("/bin/sh").to_owned(),
                        })
                    } else {
                        Err(anyhow!("not enough fields"))
                    }
                })
                .with_context(|| format!("failed to parse line {i} of \"/etc/passwd\""))
        })
    }
}
