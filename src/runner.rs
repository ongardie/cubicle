use std::io;
use std::path::Path;

use super::fs_util::DirSummary;
use super::{EnvironmentName, HostPath};
use crate::somehow::Result;

/// Manages isolated operating system environments.
pub trait Runner {
    /// Returns a list of existing environments.
    ///
    /// The returned list includes environments that partially exist.
    fn list(&self) -> Result<Vec<EnvironmentName>>;

    /// Copies a single file from within the home directory in the environment
    /// into the given writer.
    ///
    /// The given path should be within the home directory in the environment
    /// and should not descend into the work directory. Runners may return an
    /// error otherwise.
    ///
    /// This will be able to read any such file accessible by the user in the
    /// environment.
    fn copy_out_from_home(
        &self,
        name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()>;

    /// Copies a single file from within the work directory in the environment
    /// into the given writer.
    ///
    /// The given path should be within the work directory in the environment.
    /// Runners may return an error otherwise.
    ///
    /// This will be able to read any such file accessible by the user in the
    /// environment.
    fn copy_out_from_work(
        &self,
        name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()>;

    /// Creates a new environment with the given name.
    ///
    /// Fails if an environment already (partially or fully) exists with that
    /// name.
    fn create(&self, name: &EnvironmentName) -> Result<()>;

    /// Returns whether the environment fully exists, partially exists (in a
    /// likely broken state), or does not exist at all.
    fn exists(&self, name: &EnvironmentName) -> Result<EnvironmentExists>;

    /// Calculates and returns information about the filesystem paths used for
    /// the environment.
    fn files_summary(&self, name: &EnvironmentName) -> Result<EnvFilesSummary>;

    /// Stops the environment, if running, and any processes running in it.
    ///
    /// Only returns once the environment has been stopped.
    ///
    /// Does not remove the environment's home or work directories.
    fn stop(&self, name: &EnvironmentName) -> Result<()>;

    /// Stops the environment, if running, and any processes running in it, and
    /// deletes its home directory except for its work directory.
    ///
    /// This tries to make partially existing environments fully exist (or
    /// returns an error saying why they can't).
    fn reset(&self, name: &EnvironmentName) -> Result<()>;

    /// Stops the environment, if running, and any processes running in it, and
    /// deletes the environment completely, including its home directory and
    /// work directory.
    ///
    /// This makes partially existing environments no longer exist.
    fn purge(&self, name: &EnvironmentName) -> Result<()>;

    /// Runs a command or interactive shell in the environment.
    ///
    /// The environment must fully exist already.
    fn run(&self, name: &EnvironmentName, command: &RunnerCommand) -> Result<()>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnvironmentExists {
    NoEnvironment,
    PartiallyExists,
    FullyExists,
}

pub struct EnvFilesSummary {
    pub home_dir_path: Option<HostPath>,
    pub home_dir: DirSummary,
    pub work_dir_path: Option<HostPath>,
    pub work_dir: DirSummary,
}

pub enum RunnerCommand<'a> {
    Interactive,
    Init {
        seeds: Vec<HostPath>,
        script: HostPath,
    },
    Exec(&'a [String]),
}

pub struct CheckedRunner(Box<dyn Runner>);

impl CheckedRunner {
    pub fn new(runner: Box<dyn Runner>) -> Self {
        Self(runner)
    }
}

impl Runner for CheckedRunner {
    fn list(&self) -> Result<Vec<EnvironmentName>> {
        self.0.list()
    }

    fn copy_out_from_home(
        &self,
        name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        assert_eq!(
            self.exists(name)?,
            EnvironmentExists::FullyExists,
            "Environment should fully exist before copy_out_from_home"
        );
        self.0.copy_out_from_home(name, path, w)
    }

    fn copy_out_from_work(
        &self,
        name: &EnvironmentName,
        path: &Path,
        w: &mut dyn io::Write,
    ) -> Result<()> {
        assert_eq!(
            self.exists(name)?,
            EnvironmentExists::FullyExists,
            "Environment should fully exist before copy_out_from_work"
        );
        self.0.copy_out_from_work(name, path, w)
    }

    fn create(&self, name: &EnvironmentName) -> Result<()> {
        assert_eq!(
            self.exists(name)?,
            EnvironmentExists::NoEnvironment,
            "Environment should not exist before create"
        );
        self.0.create(name)?;
        assert_eq!(
            self.exists(name)?,
            EnvironmentExists::FullyExists,
            "Environment should fully exist after create"
        );
        Ok(())
    }

    fn exists(&self, name: &EnvironmentName) -> Result<EnvironmentExists> {
        self.0.exists(name)
    }

    fn files_summary(&self, name: &EnvironmentName) -> Result<EnvFilesSummary> {
        assert_ne!(
            self.exists(name)?,
            EnvironmentExists::NoEnvironment,
            "Environment should partially or fully exist before files_summary"
        );
        self.0.files_summary(name)
    }

    fn stop(&self, name: &EnvironmentName) -> Result<()> {
        assert_ne!(
            self.exists(name)?,
            EnvironmentExists::NoEnvironment,
            "Environment should fully exist before stop"
        );
        self.0.stop(name)?;
        assert_ne!(
            self.exists(name)?,
            EnvironmentExists::NoEnvironment,
            "Environment should fully exist after stop"
        );
        Ok(())
    }

    fn reset(&self, name: &EnvironmentName) -> Result<()> {
        assert_ne!(
            self.exists(name)?,
            EnvironmentExists::NoEnvironment,
            "Environment should partially or fully exist before reset"
        );
        self.0.reset(name)?;
        assert_eq!(
            self.exists(name)?,
            EnvironmentExists::FullyExists,
            "Environment should fully exist after reset"
        );
        Ok(())
    }

    fn purge(&self, name: &EnvironmentName) -> Result<()> {
        self.0.purge(name)?;
        assert_eq!(
            self.exists(name)?,
            EnvironmentExists::NoEnvironment,
            "Environment should not exist after purge"
        );
        Ok(())
    }

    fn run(&self, name: &EnvironmentName, runner_command: &RunnerCommand) -> Result<()> {
        assert_eq!(
            self.exists(name)?,
            EnvironmentExists::FullyExists,
            "Environment should fully exist before run"
        );
        self.0.run(name, runner_command)?;
        assert_eq!(
            self.exists(name)?,
            EnvironmentExists::FullyExists,
            "Environment should fully exist after run"
        );
        Ok(())
    }
}
