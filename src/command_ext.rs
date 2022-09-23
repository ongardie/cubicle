#![allow(clippy::disallowed_types)]
use std::ffi::{OsStr, OsString};
use std::process::{Child, Command as StdCommand};
pub use std::process::{ChildStderr, ChildStdin, ChildStdout, ExitStatus, Output, Stdio};

use crate::somehow::{Context, Result};

#[must_use]
pub struct ScopedChild {
    inner: Option<Child>,
    name: OsString,
}

impl ScopedChild {
    fn new(inner: Child, name: &OsStr) -> Self {
        Self {
            inner: Some(inner),
            name: name.to_owned(),
        }
    }

    pub fn stdin(&mut self) -> &mut Option<ChildStdin> {
        &mut self.inner.as_mut().unwrap().stdin
    }
    pub fn stdout(&mut self) -> &mut Option<ChildStdout> {
        &mut self.inner.as_mut().unwrap().stdout
    }
    #[allow(dead_code)]
    pub fn stderr(&mut self) -> &mut Option<ChildStderr> {
        &mut self.inner.as_mut().unwrap().stderr
    }

    pub fn wait(&mut self) -> Result<ExitStatus> {
        self.inner
            .as_mut()
            .unwrap()
            .wait()
            .with_context(|| format!("error waiting on child process {:?}", self.name))
    }

    pub fn wait_with_output(mut self) -> Result<Output> {
        self.inner
            .take()
            .unwrap()
            .wait_with_output()
            .with_context(|| format!("error waiting on child process {:?}", self.name))
    }
}

impl Drop for ScopedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.inner.take() {
            if let Ok(None) = child.try_wait() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[derive(Debug)]
pub struct Command {
    inner: StdCommand,
    set_stdin: bool,
    set_stdout: bool,
    set_stderr: bool,
}

impl Command {
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            inner: StdCommand::new(program),
            set_stdin: false,
            set_stdout: false,
            set_stderr: false,
        }
    }

    pub fn scoped_spawn(&mut self) -> Result<ScopedChild> {
        let child = self.inner.spawn().with_context(|| {
            format!(
                "failed to spawn {:?} process ($PATH is {:?})",
                self.inner.get_program(),
                match std::env::var_os("PATH") {
                    Some(path) => path,
                    None => OsString::from("not set"),
                }
            )
        })?;
        Ok(ScopedChild::new(child, self.inner.get_program()))
    }

    pub fn output(&mut self) -> Result<Output> {
        self.inner.stdin(Stdio::null());
        if !self.set_stdout {
            self.inner.stdout(Stdio::piped());
        }
        if !self.set_stderr {
            self.inner.stderr(Stdio::piped());
        }
        let child = self.scoped_spawn()?;
        child.wait_with_output()
    }

    pub fn status(&mut self) -> Result<ExitStatus> {
        let mut child = self.scoped_spawn()?;
        child.wait()
    }

    pub fn stdin<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.set_stdin = true;
        self.inner.stdin(cfg);
        self
    }

    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.set_stdout = true;
        self.inner.stdout(cfg);
        self
    }

    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.set_stderr = true;
        self.inner.stderr(cfg);
        self
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.inner.arg(arg);
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.inner.args(args);
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.inner.env_clear();
        self
    }

    pub fn env<K, V>(&mut self, key: K, val: V) -> &mut Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.inner.env(key, val);
        self
    }
}
