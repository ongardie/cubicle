#![warn(clippy::explicit_into_iter_loop)]
#![warn(clippy::explicit_iter_loop)]
#![warn(clippy::if_then_some_else_none)]
#![warn(clippy::implicit_clone)]
#![warn(clippy::redundant_else)]
#![warn(clippy::single_match_else)]
#![warn(clippy::try_err)]
#![warn(clippy::unreadable_literal)]

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use rand::seq::SliceRandom;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::io::{self, BufRead, Write};
use std::iter;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::rc::Rc;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;

mod cli;

mod runner;
use runner::{CheckedRunner, EnvFilesSummary, EnvironmentExists, Runner, RunnerCommand};

mod bytes;
use bytes::Bytes;

mod fs_util;
use fs_util::{create_tar_from_dir, file_size, summarize_dir, try_iterdir, DirSummary, TarOptions};

mod scoped_child;

#[cfg(target_os = "linux")]
mod bubblewrap;
#[cfg(target_os = "linux")]
use bubblewrap::Bubblewrap;

mod docker;
use docker::Docker;

mod user;
use user::User;

struct PackageSpec {
    build_depends: PackageNameSet,
    depends: PackageNameSet,
    dir: PathBuf,
    origin: String,
    update: Option<String>,
    test: Option<String>,
}

type PackageSpecs = BTreeMap<PackageName, PackageSpec>;

// This struct is split in two so that the runner may also keep a reference to
// `shared`.
struct Cubicle {
    shared: Rc<CubicleShared>,
    runner: CheckedRunner,
}

struct CubicleShared {
    shell: String,
    script_name: String,
    script_path: PathBuf,
    hostname: Option<String>,
    home: PathBuf,
    timezone: String,
    user: String,
    package_cache: PathBuf,
    code_package_dir: PathBuf,
    user_package_dir: PathBuf,
    eff_word_list_dir: PathBuf,
}

fn get_hostname() -> Option<String> {
    #[cfg(unix)]
    {
        let uname = rustix::process::uname();
        if let Ok(node_name) = uname.nodename().to_str() {
            if !node_name.is_empty() {
                return Some(node_name.to_owned());
            }
        }
    }
    None
}

impl Cubicle {
    fn new() -> Result<Self> {
        let hostname = get_hostname();
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

        let exe = std::env::current_exe()?;
        let script_name = match exe.file_name() {
            Some(path) => path.to_string_lossy().into_owned(),
            None => {
                return Err(anyhow!(
                    "could not get executable name from current_exe: {:?}",
                    exe
                ));
            }
        };
        let script_path = match exe.ancestors().nth(3) {
            Some(path) => path.to_owned(),
            None => {
                return Err(anyhow!(
                    "could not find project root. binary run from unexpected location: {:?}",
                    exe
                ));
            }
        };

        let timezone = match std::fs::read_to_string("/etc/timezone") {
            Ok(s) => s.trim().to_owned(),
            Err(e) => {
                println!(
                    "Warning: Falling back to UTC due to failure reading /etc/timezone: {}",
                    e
                );
                String::from("Etc/UTC")
            }
        };

        let package_cache = xdg_cache_home.join("cubicle").join("packages");
        let code_package_dir = script_path.join("packages");
        let user_package_dir = xdg_data_home.join("cubicle").join("packages");

        let eff_word_list_dir = xdg_cache_home.join("cubicle");

        let runner = get_runner(&script_path)?;

        let shared = Rc::new(CubicleShared {
            shell,
            script_name,
            script_path,
            hostname,
            home,
            timezone,
            user,
            package_cache,
            code_package_dir,
            user_package_dir,
            eff_word_list_dir,
        });

        let runner = CheckedRunner::new(match runner {
            #[cfg(target_os = "linux")]
            RunnerKind::Bubblewrap => Box::new(Bubblewrap::new(shared.clone())),
            RunnerKind::Docker => Box::new(Docker::new(shared.clone())),
            RunnerKind::User => Box::new(User::new(shared.clone())),
        });

        Ok(Cubicle { runner, shared })
    }

    fn add_packages(
        &self,
        packages: &mut BTreeMap<PackageName, PackageSpec>,
        dir: &Path,
        origin: &str,
    ) -> Result<()> {
        for name in try_iterdir(dir)? {
            let name = match name.to_str() {
                Some(name) => PackageName::from_str(name)?,
                None => {
                    return Err(anyhow!(
                        "package names must be valid UTF-8, found {name:#?} in {dir:#?}"
                    ))
                }
            };
            if packages.contains_key(&name) {
                continue;
            }
            let dir = dir.join(&name.0);
            let build_depends = read_package_list(&dir, "build-depends.txt")?.unwrap_or_default();
            let mut depends = read_package_list(&dir, "depends.txt")?.unwrap_or_default();
            depends.insert(PackageName::from_str("auto").unwrap());
            let test = dir
                .join("test.sh")
                .exists()
                .then_some(String::from("./test.sh"));
            let update = dir
                .join("update.sh")
                .exists()
                .then_some(String::from("./update.sh"));
            packages.insert(
                name,
                PackageSpec {
                    build_depends,
                    depends,
                    dir,
                    origin: origin.to_owned(),
                    test,
                    update,
                },
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct BuildDepends(bool);

fn transitive_depends(
    packages: &PackageNameSet,
    specs: &PackageSpecs,
    build_depends: BuildDepends,
) -> BTreeSet<PackageName> {
    fn visit(
        specs: &PackageSpecs,
        build_depends: BuildDepends,
        visited: &mut BTreeSet<PackageName>,
        p: &PackageName,
    ) {
        if !visited.contains(p) {
            visited.insert(p.clone());
            if let Some(spec) = specs.get(p) {
                for q in &spec.depends {
                    visit(specs, build_depends, visited, q);
                }
                if build_depends.0 {
                    for q in &spec.build_depends {
                        visit(specs, build_depends, visited, q);
                    }
                }
            }
        }
    }

    let mut visited = BTreeSet::new();
    for p in packages.iter() {
        visit(specs, build_depends, &mut visited, p);
    }
    visited
}

impl Cubicle {
    fn scan_package_names(&self) -> Result<PackageNameSet> {
        let mut names = PackageNameSet::new();
        let mut add = |dir: &Path| -> Result<()> {
            for name in try_iterdir(dir)? {
                if let Some(name) = name.to_str().and_then(|s| PackageName::from_str(s).ok()) {
                    names.insert(name);
                }
            }
            Ok(())
        };
        for dir in try_iterdir(&self.shared.user_package_dir)? {
            add(&self.shared.user_package_dir.join(dir))?;
        }
        add(&self.shared.code_package_dir)?;
        Ok(names)
    }

    fn scan_packages(&self) -> Result<PackageSpecs> {
        let mut specs = BTreeMap::new();

        for dir in try_iterdir(&self.shared.user_package_dir)? {
            let origin = dir.to_string_lossy();
            self.add_packages(
                &mut specs,
                &self.shared.user_package_dir.join(&dir),
                &origin,
            )?;
        }

        self.add_packages(&mut specs, &self.shared.code_package_dir, "built-in")?;

        let auto_deps = transitive_depends(
            &PackageNameSet::from([PackageName::from_str("auto").unwrap()]),
            &specs,
            BuildDepends(true),
        );
        for name in auto_deps {
            match specs.get_mut(&name) {
                Some(spec) => {
                    spec.depends.remove(&PackageName::from_str("auto").unwrap());
                }
                None => return Err(anyhow!("package auto transitively depends on {name} but definition of {name} not found")),
            }
        }

        Ok(specs)
    }

    fn update_packages(&self, packages: &PackageNameSet, specs: &PackageSpecs) -> Result<()> {
        let now = SystemTime::now();
        let mut todo: Vec<PackageName> =
            Vec::from_iter(transitive_depends(packages, specs, BuildDepends(true)));
        let mut done = BTreeSet::new();
        loop {
            let start_todos = todo.len();
            if start_todos == 0 {
                return Ok(());
            }
            let mut later = Vec::new();
            for name in todo {
                if let Some(spec) = specs.get(&name) {
                    if done.is_superset(&spec.depends) && done.is_superset(&spec.build_depends) {
                        self.update_stale_package(specs, &name, now)?;
                        done.insert(name);
                    } else {
                        later.push(name);
                    }
                }
            }
            if later.len() == start_todos {
                later.sort();
                return Err(anyhow!(
                    "Package dependencies are unsatisfiable for: {later:?}"
                ));
            }
            todo = later;
        }
    }

    fn last_built(&self, name: &PackageName) -> Option<SystemTime> {
        let path = self.shared.package_cache.join(format!("{name}.tar"));
        let metadata = std::fs::metadata(path).ok()?;
        metadata.modified().ok()
    }

    fn update_stale_package(
        &self,
        specs: &PackageSpecs,
        package_name: &PackageName,
        now: SystemTime,
    ) -> Result<()> {
        let spec = match specs.get(package_name) {
            Some(spec) => spec,
            None => return Err(anyhow!("Could not find package {package_name} definition")),
        };

        let needs_build = || -> Result<bool> {
            if spec.update.is_none() {
                return Ok(false);
            }
            let built = match self.last_built(package_name) {
                Some(built) => built,
                None => return Ok(true),
            };
            match now.duration_since(built) {
                Ok(d) if d > Duration::from_secs(60 * 60 * 12) => return Ok(true),
                Err(_) => return Ok(true),
                _ => {}
            }
            let DirSummary { last_modified, .. } = summarize_dir(&spec.dir)?;
            if last_modified > built {
                return Ok(true);
            }
            for p in spec.build_depends.union(&spec.depends) {
                if matches!(self.last_built(p), Some(b) if b > built) {
                    return Ok(true);
                }
            }
            Ok(false)
        };

        if needs_build()? {
            self.update_package(package_name, spec)?;
        }
        Ok(())
    }

    fn update_package(&self, package_name: &PackageName, spec: &PackageSpec) -> Result<()> {
        self.update_package_(package_name, spec)
            .with_context(|| format!("Failed to update package: {package_name}"))
    }

    fn update_package_(&self, package_name: &PackageName, spec: &PackageSpec) -> Result<()> {
        println!("Updating {package_name} package");
        let env_name = EnvironmentName::from_str(&format!("package-{package_name}")).unwrap();
        let package_cache = &self.shared.package_cache;

        {
            let tar_file = NamedTempFile::new()?;
            create_tar_from_dir(
                &spec.dir,
                tar_file.as_file(),
                &TarOptions {
                    prefix: Some(PathBuf::from(&env_name)),
                    ..TarOptions::default()
                },
            )
            .with_context(|| format!("Failed to tar package source for {package_name}"))?;

            let packages: PackageNameSet =
                spec.build_depends.union(&spec.depends).cloned().collect();

            match self.runner.exists(&env_name)? {
                EnvironmentExists::NoEnvironment => self.runner.create(&env_name)?,
                EnvironmentExists::PartiallyExists => {
                    return Err(anyhow!(
                        "Environment {env_name} in broken state (partially exists)"
                    ))
                }
                EnvironmentExists::FullyExists => {}
            }

            if let Err(e) = self.run(
                &env_name,
                &RunCommand::Init {
                    packages: &packages,
                    extra_seeds: &[tar_file.path()],
                },
            ) {
                let cached = package_cache.join(format!("{package_name}.tar"));
                if cached.exists() {
                    println!(
                        "WARNING: Failed to update package {package_name}. \
                        Keeping stale version. Error was: {e}"
                    );
                    return Ok(());
                }
                return Err(e);
            }

            // Note: the end of this block removes `tar_file` from the
            // filesystem.
        }

        std::fs::create_dir_all(&package_cache)?;
        let package_cache_dir =
            cap_std::fs::Dir::open_ambient_dir(&package_cache, cap_std::ambient_authority())?;

        match &spec.test {
            None => {
                let mut file = package_cache_dir.open_with(
                    &format!("{package_name}.tar"),
                    cap_std::fs::OpenOptions::new().create(true).write(true),
                )?;
                self.runner
                    .copy_out_from_home(&env_name, Path::new("provides.tar"), &mut file)
            }

            Some(test_script) => {
                println!("Testing {package_name} package");
                let test_name =
                    EnvironmentName::from_str(&format!("test-package-{package_name}")).unwrap();

                let tar_file = NamedTempFile::new()?;
                create_tar_from_dir(
                    &spec.dir,
                    tar_file.as_file(),
                    &TarOptions {
                        prefix: Some(PathBuf::from(&test_name)),
                        // `dev-init.sh` will run `update.sh` if it's present, but
                        // we don't want that
                        exclude: vec![PathBuf::from("update.sh")],
                    },
                )
                .with_context(|| format!("Failed to tar package source to test {package_name}"))?;

                let testing_tar = format!("{package_name}.testing.tar");
                {
                    let mut file = package_cache_dir.open_with(
                        &testing_tar,
                        cap_std::fs::OpenOptions::new().create(true).write(true),
                    )?;
                    self.runner.copy_out_from_home(
                        &env_name,
                        Path::new("provides.tar"),
                        &mut file,
                    )?;
                }

                self.purge_environment(&test_name, Quiet(true))?;
                self.runner.create(&test_name)?;
                let result = self
                    .run(
                        &test_name,
                        &RunCommand::Init {
                            packages: &spec.depends,
                            extra_seeds: &[tar_file.path(), &package_cache.join(&testing_tar)],
                        },
                    )
                    .and_then(|_| self.run(&test_name, &RunCommand::Exec(&[test_script.clone()])));
                if let Err(e) = result {
                    let cached = package_cache.join(format!("{package_name}.tar"));
                    if cached.exists() {
                        println!(
                            "WARNING: Updated package {package_name} failed tests. \
                            Keeping stale version. Error was: {e}"
                        );
                        return Ok(());
                    }
                    return Err(e);
                }
                self.purge_environment(&test_name, Quiet(true))?;
                std::fs::rename(
                    &package_cache.join(testing_tar),
                    &package_cache.join(format!("{package_name}.tar")),
                )?;
                Ok(())
            }
        }
    }

    fn enter_environment(&self, name: &EnvironmentName) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => Err(anyhow!("Environment {name} does not exist")),
            PartiallyExists => Err(anyhow!(
                "Environment {name} in broken state (try '{} reset')",
                self.shared.script_name
            )),
            FullyExists => self.run(name, &RunCommand::Interactive),
        }
    }

    fn exec_environment(&self, name: &EnvironmentName, command: &[String]) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => Err(anyhow!("Environment {name} does not exist")),
            PartiallyExists => Err(anyhow!(
                "Environment {name} in broken state (try '{} reset')",
                self.shared.script_name
            )),
            FullyExists => self.run(name, &RunCommand::Exec(command)),
        }
    }
}

fn time_serialize<S>(time: &SystemTime, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let time = time.duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
    ser.serialize_f64(time)
}

fn time_serialize_opt<S>(time: &Option<SystemTime>, ser: S) -> Result<S::Ok, S::Error>
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
    format!("{duration:.0} days")
}

fn nonzero_time(t: SystemTime) -> Option<SystemTime> {
    if t == UNIX_EPOCH {
        None
    } else {
        Some(t)
    }
}

impl Cubicle {
    fn list_environments(&self, format: ListFormat) -> Result<()> {
        let names = {
            let mut names = self.runner.list()?;
            names.sort_unstable();
            names
        };

        if format == ListFormat::Names {
            // fast path for shell completions
            for name in names {
                println!("{}", name);
            }
            return Ok(());
        }

        #[derive(Debug, Serialize)]
        struct Env {
            home_dir: Option<PathBuf>,
            home_dir_du_error: bool,
            home_dir_size: u64,
            #[serde(serialize_with = "time_serialize_opt")]
            home_dir_mtime: Option<SystemTime>,
            work_dir: Option<PathBuf>,
            work_dir_du_error: bool,
            work_dir_size: u64,
            #[serde(serialize_with = "time_serialize_opt")]
            work_dir_mtime: Option<SystemTime>,
        }

        let envs = names.iter().map(|name| {
            let summary = match self.runner.files_summary(name) {
                Ok(summary) => summary,
                Err(e) => {
                    println!("Warning: Failed to summarize disk usage for {name}: {e}");
                    EnvFilesSummary {
                        home_dir_path: None,
                        home_dir: DirSummary::new_with_errors(),
                        work_dir_path: None,
                        work_dir: DirSummary::new_with_errors(),
                    }
                }
            };
            (
                name,
                Env {
                    home_dir: summary.home_dir_path,
                    home_dir_du_error: summary.home_dir.errors,
                    home_dir_size: summary.home_dir.total_size,
                    home_dir_mtime: nonzero_time(summary.home_dir.last_modified),
                    work_dir: summary.work_dir_path,
                    work_dir_du_error: summary.work_dir.errors,
                    work_dir_size: summary.work_dir.total_size,
                    work_dir_mtime: nonzero_time(summary.work_dir.last_modified),
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
                    "", "home directory", "work directory",
                );
                println!(
                    "{:<nw$} | {:>10} {:>13} | {:>10} {:>13}",
                    "name", "size", "modified", "size", "modified",
                );
                println!("{0:-<nw$} + {0:-<10} {0:-<13} + {0:-<10} {0:-<13}", "",);

                // `Bytes` doesn't implement width/alignment, so it needs an
                // extra `to_string()`.
                #[allow(clippy::to_string_in_format_args)]
                for (name, env) in envs {
                    println!(
                        "{:<nw$} | {:>9}{} {:>13} | {:>9}{} {:>13}",
                        name,
                        Bytes(env.home_dir_size).to_string(),
                        if env.home_dir_du_error { '+' } else { ' ' },
                        match env.home_dir_mtime {
                            Some(mtime) => rel_time(now.duration_since(mtime).ok()),
                            None => String::from("N/A"),
                        },
                        Bytes(env.work_dir_size).to_string(),
                        if env.work_dir_du_error { '+' } else { ' ' },
                        match env.work_dir_mtime {
                            Some(mtime) => rel_time(now.duration_since(mtime).ok()),
                            None => String::from("N/A"),
                        },
                    );
                }
            }
        }

        Ok(())
    }

    fn list_packages(&self, format: ListPackagesFormat) -> Result<()> {
        type Format = ListPackagesFormat;

        if format == Format::Names {
            // fast path for shell completions
            for name in self.scan_package_names()? {
                println!("{}", name);
            }
            return Ok(());
        }

        #[derive(Debug, Serialize)]
        struct Package {
            build_depends: Vec<String>,
            #[serde(serialize_with = "time_serialize_opt")]
            built: Option<SystemTime>,
            depends: Vec<String>,
            #[serde(serialize_with = "time_serialize")]
            edited: SystemTime,
            dir: PathBuf,
            origin: String,
            size: Option<u64>,
        }

        let specs = self.scan_packages()?;
        let packages = specs
            .into_iter()
            .map(|(name, spec)| -> Result<(PackageName, Package)> {
                let (built, size) = {
                    match std::fs::metadata(&self.shared.package_cache.join(format!("{name}.tar")))
                    {
                        Ok(metadata) => (metadata.modified().ok(), file_size(&metadata)),
                        Err(_) => (None, None),
                    }
                };
                let edited = summarize_dir(&spec.dir)?.last_modified;
                Ok((
                    name,
                    Package {
                        build_depends: spec
                            .build_depends
                            .iter()
                            .map(|name| name.0.clone())
                            .collect(),
                        built,
                        depends: spec.depends.iter().map(|name| name.0.clone()).collect(),
                        dir: spec.dir,
                        edited,
                        origin: spec.origin,
                        size,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;

        match format {
            Format::Names => unreachable!("handled above"),

            Format::Json => {
                println!("{}", serde_json::to_string_pretty(&packages)?);
            }

            Format::Default => {
                let nw = packages
                    .keys()
                    .map(|name| name.0.len())
                    .chain(iter::once(10))
                    .max()
                    .unwrap();
                let now = SystemTime::now();
                println!(
                    "{:<nw$}  {:<8}  {:>10}  {:>13}  {:>13}",
                    "name", "origin", "size", "built", "edited",
                );
                println!("{0:-<nw$}  {0:-<8}  {0:-<10}  {0:-<13}  {0:-<13}", "");
                for (name, package) in packages {
                    println!(
                        "{:<nw$}  {:<8}  {:>10}  {:>13}  {:>13}",
                        name,
                        package.origin,
                        match package.size {
                            Some(size) => Bytes(size).to_string(),
                            None => String::from("N/A"),
                        },
                        match package.built {
                            Some(built) => rel_time(now.duration_since(built).ok()),
                            None => String::from("N/A"),
                        },
                        rel_time(now.duration_since(package.edited).ok()),
                    );
                }
            }
        }
        Ok(())
    }

    fn read_package_list_from_env(&self, name: &EnvironmentName) -> Result<Option<PackageNameSet>> {
        let mut buf = Vec::new();
        self.runner
            .copy_out_from_work(name, Path::new("packages.txt"), &mut buf)?;
        let reader = io::BufReader::new(buf.as_slice());
        let names = reader.lines().collect::<Result<Vec<String>, _>>()?;
        Ok(Some(package_set_from_names(names)?))
    }
}

fn read_package_list(dir: &Path, path: &str) -> Result<Option<PackageNameSet>> {
    let dir = cap_std::fs::Dir::open_ambient_dir(dir, cap_std::ambient_authority())?;
    let file = match dir.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let reader = io::BufReader::new(file);
    let names = reader.lines().collect::<Result<Vec<String>, _>>()?;
    Ok(Some(package_set_from_names(names)?))
}

fn write_package_list_tar(
    name: &EnvironmentName,
    packages: &PackageNameSet,
) -> Result<tempfile::NamedTempFile> {
    let file = tempfile::NamedTempFile::new()?;
    let metadata = file.as_file().metadata()?;
    let mut builder = tar::Builder::new(file.as_file());
    let mut header = tar::Header::new_gnu();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        header.set_mtime(metadata.mtime() as u64);
        header.set_uid(metadata.uid() as u64);
        header.set_gid(metadata.gid() as u64);
        header.set_mode(metadata.mode());
    }

    let mut buf = Vec::new();
    for package in packages.iter() {
        writeln!(buf, "{package}")?;
    }
    header.set_size(buf.len() as u64);
    builder.append_data(
        &mut header,
        Path::new(name).join("packages.txt"),
        buf.as_slice(),
    )?;
    builder.into_inner().and_then(|mut f| f.flush())?;
    Ok(file)
}

impl Cubicle {
    fn new_environment(
        &self,
        name: &EnvironmentName,
        packages: Option<PackageNameSet>,
    ) -> Result<()> {
        use EnvironmentExists::*;
        match self.runner.exists(name)? {
            NoEnvironment => {}
            PartiallyExists => {
                return Err(anyhow!(
                    "Environment {name} in broken state (try '{} reset')",
                    self.shared.script_name
                ))
            }
            FullyExists => {
                return Err(anyhow!(
                    "Environment {name} already exists (did you mean '{} reset'?)",
                    self.shared.script_name
                ))
            }
        }

        self.runner.create(name)?;

        let packages = match packages {
            Some(p) => p,
            None => PackageNameSet::from([PackageName::from_str("default").unwrap()]),
        };
        self.update_packages(&packages, &self.scan_packages()?)?;
        let packages_txt = write_package_list_tar(name, &packages)?;
        self.run(
            name,
            &RunCommand::Init {
                packages: &packages,
                extra_seeds: &[packages_txt.path()],
            },
        )
        .with_context(|| format!("Failed to initialize new environment {name}"))
    }

    fn random_name<F>(&self, filter: F) -> Result<String>
    where
        F: Fn(&str) -> Result<bool>,
    {
        fn from_file<F>(file: std::fs::File, filter: F) -> Result<String>
        where
            F: Fn(&str) -> Result<bool>,
        {
            let mut rng = rand::thread_rng();
            let reader = io::BufReader::new(file);
            let lines = reader.lines().collect::<Result<Vec<String>, _>>()?;
            for _ in 0..200 {
                if let Some(line) = lines.choose(&mut rng) {
                    for word in line.split_ascii_whitespace() {
                        if word.chars().all(char::is_numeric) {
                            // probably diceware numbers
                            continue;
                        }
                        if filter(word)? {
                            return Ok(word.to_owned());
                        }
                    }
                }
            }
            Err(anyhow!("found no suitable word"))
        }

        // 1. Prefer the EFF short word list. See https://www.eff.org/dice for
        // more info.
        let eff = || -> Result<String> {
            let eff_word_list = self
                .shared
                .eff_word_list_dir
                .join("eff_short_wordlist_1.txt");
            let file = match std::fs::File::open(&eff_word_list) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    println!("Downloading EFF short wordlist");
                    let url = "https://www.eff.org/files/2016/09/08/eff_short_wordlist_1.txt";
                    let body = reqwest::blocking::get(url)?.text()?;
                    std::fs::write(&eff_word_list, body)?;
                    std::fs::File::open(&eff_word_list)?
                }
                Err(e) => return Err(e.into()),
            };
            from_file(file, |w| Ok(w.len() < 10 && filter(w)?))
        };

        // 2. /usr/share/dict/words
        let dict = || -> Result<String> {
            let file = std::fs::File::open("/usr/share/dict/words")?;
            from_file(file, |w| Ok(w.len() < 6 && filter(w)?))
        };

        match eff() {
            Ok(word) => return Ok(word),
            Err(e) => {
                println!("Warning: failed to extract word from EFF list: {e}");
            }
        }
        match dict() {
            Ok(word) => return Ok(word),
            Err(e) => {
                println!("Warning: failed to extract word from /usr/share/dict/words: {e}");
            }
        }

        // 3. Random 6 letters
        let mut rng = rand::thread_rng();
        let alphabet = [
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q',
            'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
        ];
        for _ in 0..20 {
            let word = std::iter::repeat_with(|| alphabet.choose(&mut rng).unwrap())
                .take(6)
                .collect::<String>();
            if filter(&word)? {
                return Ok(word);
            }
        }

        // 4. Random 32 letters
        let word = std::iter::repeat_with(|| alphabet.choose(&mut rng).unwrap())
            .take(32)
            .collect::<String>();
        if filter(&word)? {
            return Ok(word);
        }

        // 5. Give up.
        Err(anyhow!(
            "Failed to generate suitable random word with any strategy"
        ))
    }

    fn create_enter_tmp_environment(&self, packages: Option<PackageNameSet>) -> Result<()> {
        let name = {
            let name = self
                .random_name(|name| {
                    if name.starts_with("cub") {
                        // that'd be confusing
                        return Ok(false);
                    }
                    match EnvironmentName::from_str(&format!("tmp-{name}")) {
                        Ok(env) => {
                            let exists = self.runner.exists(&env)?;
                            Ok(exists == EnvironmentExists::NoEnvironment)
                        }
                        Err(_) => Ok(false),
                    }
                })
                .context("Failed to generate random environment name")?;
            EnvironmentName::from_str(&format!("tmp-{name}")).unwrap()
        };
        self.new_environment(&name, packages)?;
        self.run(&name, &RunCommand::Interactive)
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Quiet(bool);

impl Cubicle {
    fn purge_environment(&self, name: &EnvironmentName, quiet: Quiet) -> Result<()> {
        if !quiet.0 && self.runner.exists(name)? == EnvironmentExists::NoEnvironment {
            println!("Warning: environment {name} does not exist (nothing to purge)");
        }
        // Call purge regardless in case it disagrees with `exists` and finds
        // something useful to do.
        self.runner.purge(name)?;
        assert_eq!(
            self.runner.exists(name)?,
            EnvironmentExists::NoEnvironment,
            "Environment should not exist after purge"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Clean(bool);

impl Cubicle {
    fn reset_environment(
        &self,
        name: &EnvironmentName,
        packages: &Option<PackageNameSet>,
        clean: Clean,
    ) -> Result<()> {
        if self.runner.exists(name)? == EnvironmentExists::NoEnvironment {
            return Err(anyhow!(
                "Environment {name} does not exist (did you mean '{} new'?)",
                self.shared.script_name,
            ));
        }

        if clean.0 {
            return self.runner.reset(name);
        }

        let (mut changed, mut packages) = match packages {
            Some(packages) => (true, packages.clone()),
            None => match self.read_package_list_from_env(name)? {
                None => (
                    true,
                    PackageNameSet::from([PackageName::from_str("default").unwrap()]),
                ),
                Some(packages) => (false, packages),
            },
        };

        self.runner.reset(name)?;

        match name.extract_builder_package_name() {
            None => {
                self.update_packages(&packages, &self.scan_packages()?)?;
            }
            Some(package_name) => {
                let specs = self.scan_packages()?;
                let spec = match specs.get(&package_name) {
                    Some(spec) => spec,
                    None => {
                        return Err(anyhow!("Could not find package source for {package_name}"))
                    }
                };
                let start_len = packages.len();
                packages.extend(spec.build_depends.iter().cloned());
                packages.extend(spec.depends.iter().cloned());
                changed = changed || packages.len() != start_len;
                self.update_packages(&packages, &specs)?;
                self.update_package(&package_name, spec)?;
            }
        }

        let mut extra_seeds = Vec::new();
        let packages_txt;
        if changed {
            packages_txt = write_package_list_tar(name, &packages)?;
            extra_seeds.push(packages_txt.path());
        }

        self.run(
            name,
            &RunCommand::Init {
                packages: &packages,
                extra_seeds: &extra_seeds,
            },
        )
    }

    fn packages_to_seeds(&self, packages: &PackageNameSet) -> Result<Vec<PathBuf>> {
        let mut seeds = Vec::with_capacity(packages.len());
        let specs = self.scan_packages()?;
        let deps = transitive_depends(packages, &specs, BuildDepends(false));
        for package in deps {
            let provides = self.shared.package_cache.join(format!("{package}.tar"));
            if provides.exists() {
                seeds.push(provides);
            }
        }
        Ok(seeds)
    }

    fn run(&self, name: &EnvironmentName, command: &RunCommand) -> Result<()> {
        let runner_command = match command {
            RunCommand::Interactive => RunnerCommand::Interactive,
            RunCommand::Init {
                packages,
                extra_seeds,
            } => {
                let mut seeds = self.packages_to_seeds(packages)?;
                for seed in extra_seeds.iter() {
                    seeds.push((*seed).to_owned());
                }
                RunnerCommand::Init {
                    seeds,
                    script: self.shared.script_path.join("dev-init.sh"),
                }
            }
            RunCommand::Exec(cmd) => RunnerCommand::Exec(cmd),
        };

        self.runner.run(name, &runner_command)
    }
}

enum RunCommand<'a> {
    Interactive,
    Init {
        packages: &'a PackageNameSet,
        extra_seeds: &'a [&'a Path],
    },
    Exec(&'a [String]),
}

#[derive(Debug)]
struct ExitStatusError {
    status: ExitStatus,
    context: String,
}

impl ExitStatusError {
    fn new(status: ExitStatus, context: &str) -> Self {
        assert!(matches!(status.code(), Some(c) if c != 0));
        Self {
            status,
            context: context.to_owned(),
        }
    }
}

impl std::error::Error for ExitStatusError {}

impl fmt::Display for ExitStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Non-zero exit status ({}) from {}",
            self.status.code().unwrap(),
            self.context
        )
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EnvironmentName(String);

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

        Ok(Self(s.to_owned()))
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

impl std::convert::AsRef<OsStr> for EnvironmentName {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

impl EnvironmentName {
    fn extract_builder_package_name(&self) -> Option<PackageName> {
        self.0
            .strip_prefix("package-")
            .and_then(|s| PackageName::from_str(s).ok())
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialOrd, PartialEq, Serialize)]
struct PackageName(String);

impl FromStr for PackageName {
    type Err = anyhow::Error;
    fn from_str(mut s: &str) -> Result<Self> {
        s = s.trim();
        if s.is_empty() {
            return Err(anyhow!("package name cannot be empty"));
        }
        if s.contains(|c: char| {
            (c.is_ascii() && !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_'))
                || c.is_control()
                || c.is_whitespace()
        }) {
            return Err(anyhow!("package name cannot contain special characters"));
        }
        Ok(Self(s.to_owned()))
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

type PackageNameSet = BTreeSet<PackageName>;

fn package_set_from_names(names: Vec<String>) -> Result<PackageNameSet> {
    let mut set: PackageNameSet = BTreeSet::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let name = PackageName::from_str(name)?;
        set.insert(name);
    }
    Ok(set)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ValueEnum)]
enum ListFormat {
    #[default]
    Default,
    Json,
    Names,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ValueEnum)]
enum ListPackagesFormat {
    #[default]
    Default,
    Json,
    Names,
}

#[derive(Clone, Copy)]
enum RunnerKind {
    #[cfg(target_os = "linux")]
    Bubblewrap,
    Docker,
    User,
}

fn get_runner(script_path: &Path) -> Result<RunnerKind> {
    let runner_path = script_path.join(".RUNNER");
    let runners = "'bubblewrap' (on Linux only) or 'docker' or 'user'";
    match std::fs::read_to_string(&runner_path)
        .with_context(|| format!("Could not read {:?}. Expected {}.", runner_path, runners))?
        .trim()
    {
        #[cfg(target_os = "linux")]
        "bubblewrap" => Ok(RunnerKind::Bubblewrap),
        #[cfg(not(target_os = "linux"))]
        "bubblewrap" => Err(anyhow!("Bubblewrap is only supported on Linux")),
        "docker" => Ok(RunnerKind::Docker),
        "user" => Ok(RunnerKind::User),
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
    let args = cli::parse();
    cli::run(args, &program)
}
