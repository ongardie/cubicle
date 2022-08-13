use super::scoped_child::{ScopedChild, ScopedSpawn};
use super::{Cubicle, EnvironmentName, ExitStatusError, Runner, RunnerCommand, RunnerRunArgs};
use anyhow::Result;
use std::ffi::OsString;
use std::path::Path;
use std::process::{ChildStdout, Command, Stdio};

pub struct Bubblewrap<'a> {
    pub(super) program: &'a Cubicle,
}

fn get_fd_for_child<F>(file: &F) -> Result<String>
where
    F: rustix::fd::AsFd + std::os::unix::io::AsRawFd,
{
    // This is pretty ugly, but it's how bwrap likes it.
    let mut flags = rustix::fs::fcntl_getfd(file)?;
    flags.remove(rustix::fs::FdFlags::CLOEXEC);
    rustix::fs::fcntl_setfd(&file, flags)?;
    Ok(file.as_raw_fd().to_string())
}

fn ro_bind_try<P: AsRef<Path>>(path: P) -> [OsString; 3] {
    return [
        OsString::from("--ro-bind-try"),
        path.as_ref().as_os_str().to_owned(),
        path.as_ref().as_os_str().to_owned(),
    ];
}

impl<'a> Runner for Bubblewrap<'a> {
    fn kill(&self, _name: &EnvironmentName) -> Result<()> {
        // nothing to do
        Ok(())
    }

    fn run(
        &self,
        name: &EnvironmentName,
        RunnerRunArgs {
            command: run_command,
            host_home,
            host_work,
        }: &RunnerRunArgs,
    ) -> Result<()> {
        struct Seed {
            _child: ScopedChild, // this is here so its destructor will reap it later
            stdout: ChildStdout,
        }
        let seed = if let RunnerCommand::Init { seeds, .. } = run_command {
            println!("Packing seed tarball");
            let mut child = Command::new("pv")
                .args(["-i", "0.1"])
                .stdout(Stdio::piped())
                .args(seeds)
                .scoped_spawn()?;
            let stdout = child.stdout.take().unwrap();
            Some(Seed {
                _child: child,
                stdout,
            })
        } else {
            None
        };

        let seccomp = std::fs::File::open(self.program.script_path.join("seccomp.bpf"))?;

        let mut command = Command::new("bwrap");

        command.env_clear();
        command.env(
            "PATH",
            match self.program.home.to_str() {
                Some(home) => format!("{home}/bin:/bin:/sbin"),
                None => String::from("/bin:/sbin"),
            },
        );
        command.env("SANDBOX", name);
        command.env("TMPDIR", self.program.home.join("tmp"));
        for key in ["DISPLAY", "HOME", "SHELL", "TERM", "USER"] {
            if let Ok(value) = std::env::var(key) {
                command.env(key, value);
            }
        }

        command.arg("--die-with-parent");
        command.arg("--unshare-cgroup");
        command.arg("--unshare-ipc");
        command.arg("--unshare-pid");
        command.arg("--unshare-uts");

        command.arg("--hostname");
        match &self.program.hostname {
            Some(hostname) => command.arg(format!("{name}.{hostname}")),
            None => command.arg(name),
        };

        command.args(["--symlink", "/usr/bin", "/bin"]);
        command.args(["--dev", "/dev"]);

        if let RunnerCommand::Init { script, .. } = run_command {
            command
                .arg("--ro-bind-try")
                .arg(script)
                .arg("/cubicle-init.sh");
        }

        if let Some(Seed { stdout, .. }) = &seed {
            command
                .arg("--file")
                .arg(get_fd_for_child(stdout)?)
                .arg("/dev/shm/seed.tar");
        }
        command.args(ro_bind_try("/etc"));
        command.arg("--bind").arg(host_home).arg(&self.program.home);
        command
            .arg("--bind")
            .arg(host_work)
            .arg(self.program.home.join(name));
        command.args(["--symlink", "/usr/lib", "/lib"]);
        command.args(["--symlink", "/usr/lib64", "/lib64"]);
        command.args(ro_bind_try("/opt"));
        command.args(["--proc", "/proc"]);
        command.args(["--symlink", "/usr/sbin", "/sbin"]);
        command.args(["--tmpfs", "/tmp"]);
        command.args(ro_bind_try("/usr"));
        command.args(ro_bind_try("/var/lib/apt/lists"));
        command.args(ro_bind_try("/var/lib/dpkg"));
        command.arg("--seccomp").arg(get_fd_for_child(&seccomp)?);
        command.arg("--chdir").arg(self.program.home.join(name));
        command.arg("--");
        command.arg(&self.program.shell);
        command.arg("-l");

        match run_command {
            RunnerCommand::Interactive => {}
            RunnerCommand::Init { .. } => {
                command.args(["-c", "/cubicle-init.sh"]);
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
            Err(ExitStatusError::new(status, "bwrap").into())
        }
    }
}
