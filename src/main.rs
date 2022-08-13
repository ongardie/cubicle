#![warn(clippy::explicit_into_iter_loop)]
#![warn(clippy::explicit_iter_loop)]
#![warn(clippy::if_then_some_else_none)]
#![warn(clippy::implicit_clone)]
#![warn(clippy::redundant_else)]
#![warn(clippy::single_match_else)]
#![warn(clippy::try_err)]
#![warn(clippy::unreadable_literal)]

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::{generate, shells::Shell};
use rand::seq::SliceRandom;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, BufRead, Write};
use std::iter;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod bytes;
use bytes::Bytes;

mod fs_util;
use fs_util::{copyfile_untrusted, du, rmtree, try_iterdir, DiskUsage, MaybeTempFile};

mod scoped_child;

#[cfg(target_os = "linux")]
mod bubblewrap;
#[cfg(target_os = "linux")]
use bubblewrap::Bubblewrap;

mod docker;
use docker::Docker;

struct PackageSpec {
    build_depends: PackageNameSet,
    depends: PackageNameSet,
    dir: PathBuf,
    origin: String,
    update: Option<String>,
    test: Option<String>,
}

type PackageSpecs = BTreeMap<PackageName, PackageSpec>;

struct Cubicle {
    shell: String,
    script_name: String,
    script_path: PathBuf,
    hostname: Option<String>,
    home: PathBuf,
    home_dirs: PathBuf,
    work_dirs: PathBuf,
    tmp_dir: PathBuf,
    timezone: String,
    user: String,
    runner: RunnerKind,
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

        let home_dirs = xdg_cache_home.join("cubicle").join("home");

        let work_dirs = xdg_data_home.join("cubicle").join("work");

        let tmp_dir = match std::env::var("TMPDIR") {
            Ok(path) => PathBuf::from(path),
            Err(_) => PathBuf::from("/tmp"),
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

        let program = Self {
            shell,
            script_name,
            script_path,
            hostname,
            home,
            home_dirs,
            work_dirs,
            tmp_dir,
            timezone,
            user,
            runner,
            package_cache,
            code_package_dir,
            user_package_dir,
            eff_word_list_dir,
        };

        Ok(program)
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
        for dir in try_iterdir(&self.user_package_dir)? {
            add(&self.user_package_dir.join(dir))?;
        }
        add(&self.code_package_dir)?;
        Ok(names)
    }

    fn scan_packages(&self) -> Result<PackageSpecs> {
        let mut specs = BTreeMap::new();

        for dir in try_iterdir(&self.user_package_dir)? {
            let origin = dir.to_string_lossy();
            self.add_packages(&mut specs, &self.user_package_dir.join(&dir), &origin)?;
        }

        self.add_packages(&mut specs, &self.code_package_dir, "built-in")?;

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
        let mut todo: Vec<PackageName> = transitive_depends(packages, specs, BuildDepends(true))
            .into_iter()
            .collect();
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
        let path = self.package_cache.join(format!("{name}.tar"));
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
            let DiskUsage { mtime, .. } = du(&spec.dir)?;
            if mtime > built {
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
        println!("Updating {package_name} package");
        let env_name = EnvironmentName::from_str(&format!("package-{package_name}")).unwrap();

        let work_dir = self.work_dirs.join(&env_name);
        if !work_dir.exists() {
            std::fs::create_dir_all(work_dir)?;
        }

        {
            let tar_path = MaybeTempFile(self.tmp_dir.join(format!("cubicle-{package_name}.tar")));
            let status = Command::new("tar")
                .arg("-c")
                .arg("--directory")
                .arg(&spec.dir)
                .arg(".")
                .args(["--transform", &format!(r#"s/^\./{env_name}/"#)])
                .arg("-f")
                .arg(tar_path.deref())
                .status()?;
            if !status.success() {
                return Err(anyhow!(
                    "failed to tar package source for {package_name}: \
                    tar exited with status {:#?}",
                    status.code(),
                ));
            }

            let packages: PackageNameSet =
                spec.build_depends.union(&spec.depends).cloned().collect();

            if let Err(e) = self.run(
                &env_name,
                &RunCommand::Init {
                    packages: &packages,
                    extra_seeds: &[tar_path.deref()],
                },
            ) {
                let cached = self.package_cache.join(format!("{package_name}.tar"));
                if cached.exists() {
                    println!(
                        "WARNING: Failed to update package {package_name}. \
                        Keeping stale version. Error was: {e}"
                    );
                    return Ok(());
                }
                return Err(e);
            }

            // Note: the end of this block removes `tar_path` from the
            // filesystem.
        }

        std::fs::create_dir_all(&self.package_cache)?;
        match &spec.test {
            None => {
                // We want to access `provides.tar` from the package build container.
                // However, that could potentially be a (malicious) symlink that points
                // to some sensitive file elsewhere on the host.
                copyfile_untrusted(
                    &self.home_dirs.join(env_name),
                    Path::new("provides.tar"),
                    &self.package_cache,
                    Path::new(&format!("{package_name}.tar")),
                )
            }

            Some(test_script) => {
                println!("Testing {package_name} package");
                let test_name =
                    EnvironmentName::from_str(&format!("test-package-{package_name}")).unwrap();

                let tar_path =
                    MaybeTempFile(self.tmp_dir.join(format!("cubicle-{package_name}.tar")));
                let status = Command::new("tar")
                    .arg("-c")
                    .arg("--anchored")
                    .arg("--directory")
                    .arg(&spec.dir)
                    // dev-init.sh will run `update.sh` if it's present, but we
                    // don't want that
                    .args(["--exclude", "./update.sh"])
                    .arg(".")
                    .args(["--transform", &format!(r#"s/^\./{test_name}/"#)])
                    .arg("-f")
                    .arg(tar_path.deref())
                    .status()?;
                if !status.success() {
                    return Err(anyhow!(
                        "failed to tar package source to test {package_name}: \
                        tar exited with status {:#?}",
                        status.code(),
                    ));
                }

                // See copyfile comment above.
                let testing_tar = format!("{package_name}.testing.tar");
                copyfile_untrusted(
                    &self.home_dirs.join(env_name),
                    Path::new("provides.tar"),
                    &self.package_cache,
                    Path::new(&testing_tar),
                )?;

                self.purge_environment(&test_name, Quiet(true))?;
                let work_dir = self.work_dirs.join(&test_name);
                std::fs::create_dir_all(work_dir)?;
                let result = self
                    .run(
                        &test_name,
                        &RunCommand::Init {
                            packages: &spec.depends,
                            extra_seeds: &[&tar_path, &self.package_cache.join(&testing_tar)],
                        },
                    )
                    .and_then(|_| self.run(&test_name, &RunCommand::Exec(&[test_script.clone()])));
                if let Err(e) = result {
                    let cached = self.package_cache.join(format!("{package_name}.tar"));
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
                    &self.package_cache.join(testing_tar),
                    &self.package_cache.join(format!("{package_name}.tar")),
                )?;
                Ok(())
            }
        }
    }

    fn enter_environment(&self, name: &EnvironmentName) -> Result<()> {
        if !self.work_dirs.join(name).exists() {
            return Err(anyhow!("Environment {} does not exist", name));
        }
        self.run(name, &RunCommand::Interactive)
    }

    fn exec_environment(&self, name: &EnvironmentName, command: &[String]) -> Result<()> {
        if !self.work_dirs.join(name).exists() {
            return Err(anyhow!("Environment {} does not exist", name));
        }
        self.run(name, &RunCommand::Exec(command))
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
            home_dir_size: Option<u64>,
            #[serde(serialize_with = "time_serialize_opt")]
            home_dir_mtime: Option<SystemTime>,
            work_dir: Option<PathBuf>,
            work_dir_du_error: bool,
            work_dir_size: Option<u64>,
            #[serde(serialize_with = "time_serialize_opt")]
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
                    home_dir: home_dir.exists().then_some(home_dir),
                    home_dir_du_error,
                    home_dir_size,
                    home_dir_mtime,
                    work_dir: work_dir.exists().then_some(work_dir),
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
                    "", "home directory", "work directory",
                );
                println!(
                    "{:<nw$} | {:>10} {:>13} | {:>10} {:>13}",
                    "name", "size", "modified", "size", "modified",
                );
                println!("{0:-<nw$} + {0:-<10} {0:-<13} + {0:-<10} {0:-<13}", "",);
                for (name, env) in envs {
                    println!(
                        "{:<nw$} | {:>10} {:>13} | {:>10} {:>13}",
                        name,
                        match env.home_dir_size {
                            Some(size) => {
                                let mut size = Bytes(size).to_string();
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
                                let mut size = Bytes(size).to_string();
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
                let DiskUsage {
                    error,
                    size,
                    mtime: built,
                } = du(&self.package_cache.join(format!("{name}.tar")))?;
                let DiskUsage { mtime: edited, .. } = du(&spec.dir)?;
                Ok((
                    name,
                    Package {
                        build_depends: spec
                            .build_depends
                            .iter()
                            .map(|name| name.0.clone())
                            .collect(),
                        built: if error { None } else { Some(built) },
                        depends: spec.depends.iter().map(|name| name.0.clone()).collect(),
                        dir: spec.dir,
                        edited,
                        origin: spec.origin,
                        size: if error { None } else { Some(size) },
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

fn write_package_list(dir: &Path, path: &str, packages: &PackageNameSet) -> Result<()> {
    let dir = cap_std::fs::Dir::open_ambient_dir(dir, cap_std::ambient_authority())?;
    let mut file = dir.open_with(
        path,
        cap_std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true),
    )?;
    for package in packages.iter() {
        writeln!(file, "{package}")?;
    }
    writeln!(file)?;
    file.flush()?;
    Ok(())
}

impl Cubicle {
    fn new_environment(
        &self,
        name: &EnvironmentName,
        packages: Option<PackageNameSet>,
    ) -> Result<()> {
        let work_dir = self.work_dirs.join(name);
        if work_dir.exists() || self.home_dirs.join(name).exists() {
            return Err(anyhow!(
                "environment {name} exists (did you mean '{} reset'?)",
                self.script_name,
            ));
        }

        let packages = match packages {
            Some(p) => p,
            None => PackageNameSet::from([PackageName::from_str("default").unwrap()]),
        };

        self.update_packages(&packages, &self.scan_packages()?)?;
        std::fs::create_dir_all(&work_dir)?;
        write_package_list(&work_dir, "packages.txt", &packages)?;
        self.run(
            name,
            &RunCommand::Init {
                packages: &packages,
                extra_seeds: &[],
            },
        )
    }

    fn random_name<F>(&self, filter: F) -> Option<String>
    where
        F: Fn(&str) -> bool,
    {
        fn from_file<F>(file: std::fs::File, filter: F) -> Result<String>
        where
            F: Fn(&str) -> bool,
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
                        if filter(word) {
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
            let eff_word_list = self.eff_word_list_dir.join("eff_short_wordlist_1.txt");
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
            from_file(file, |w| w.len() < 10 && filter(w))
        };

        // 2. /usr/share/dict/words
        let dict = || -> Result<String> {
            let file = std::fs::File::open("/usr/share/dict/words")?;
            from_file(file, |w| w.len() < 6 && filter(w))
        };

        match eff() {
            Ok(word) => return Some(word),
            Err(e) => {
                println!("Warning: failed to extract word from EFF list: {e}");
            }
        }
        match dict() {
            Ok(word) => return Some(word),
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
            if filter(&word) {
                return Some(word);
            }
        }

        // 4. Random 32 letters
        let word = std::iter::repeat_with(|| alphabet.choose(&mut rng).unwrap())
            .take(32)
            .collect::<String>();
        if filter(&word) {
            return Some(word);
        }

        // 5. Give up.
        None
    }

    fn create_enter_tmp_environment(&self, packages: Option<PackageNameSet>) -> Result<()> {
        let name = match self.random_name(|name| {
            if name.starts_with("cub") {
                // that'd be confusing
                return false;
            }
            let name = format!("tmp-{name}");
            EnvironmentName::from_str(&name).is_ok()
                && !self.work_dirs.join(&name).exists()
                && !self.home_dirs.join(&name).exists()
        }) {
            Some(name) => EnvironmentName::from_str(&format!("tmp-{name}")).unwrap(),
            None => return Err(anyhow!("failed to generate random environment name")),
        };

        let packages = match packages {
            Some(p) => p,
            None => PackageNameSet::from([PackageName::from_str("default").unwrap()]),
        };
        self.update_packages(&packages, &self.scan_packages()?)?;

        let work_dir = self.work_dirs.join(&name);
        std::fs::create_dir_all(&work_dir)?;
        write_package_list(&work_dir, "packages.txt", &packages)?;
        self.run(
            &name,
            &RunCommand::Init {
                packages: &packages,
                extra_seeds: &[],
            },
        )?;
        self.run(&name, &RunCommand::Interactive)
    }
}

#[derive(Clone, Copy, PartialEq)]
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
        let work_dir = self.work_dirs.join(name);
        if !work_dir.exists() {
            return Err(anyhow!(
                "environment {name} does not exist (did you mean '{} reset'?)",
                self.script_name,
            ));
        }
        let host_home = self.home_dirs.join(name);
        if host_home.exists() {
            self.with_runner(|runner| runner.kill(name))?;
            rmtree(&host_home)?;
        }
        if clean.0 {
            return Ok(());
        }

        let (mut changed, mut packages) = match packages {
            Some(packages) => (true, packages.clone()),
            None => match read_package_list(&work_dir, "packages.txt")? {
                None => (
                    true,
                    PackageNameSet::from([PackageName::from_str("default").unwrap()]),
                ),
                Some(packages) => (false, packages),
            },
        };
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
        if changed {
            write_package_list(&work_dir, "packages.txt", &packages)?;
        }

        self.run(
            name,
            &RunCommand::Init {
                packages: &packages,
                extra_seeds: &[],
            },
        )
    }

    fn with_runner<F, T>(&self, func: F) -> Result<T>
    where
        F: Fn(&dyn Runner) -> Result<T>,
    {
        match self.runner {
            #[cfg(target_os = "linux")]
            RunnerKind::Bubblewrap => func(&Bubblewrap { program: self }),
            RunnerKind::Docker => func(&Docker { program: self }),
        }
    }

    fn packages_to_seeds(&self, packages: &PackageNameSet) -> Result<Vec<PathBuf>> {
        let mut seeds = Vec::with_capacity(packages.len());
        let specs = self.scan_packages()?;
        let deps = transitive_depends(packages, &specs, BuildDepends(false));
        for package in deps {
            let provides = self.package_cache.join(format!("{package}.tar"));
            if provides.exists() {
                seeds.push(provides);
            }
        }
        Ok(seeds)
    }

    fn run(&self, name: &EnvironmentName, command: &RunCommand) -> Result<()> {
        let host_home = self.home_dirs.join(name);
        let host_work = self.work_dirs.join(name);

        std::fs::create_dir_all(&host_home)?;

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
                    script: self.script_path.join("dev-init.sh"),
                }
            }
            RunCommand::Exec(cmd) => RunnerCommand::Exec(cmd),
        };

        self.with_runner(|runner| {
            runner.run(
                name,
                &RunnerRunArgs {
                    command: &runner_command,
                    host_home: &host_home,
                    host_work: &host_work,
                },
            )
        })
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

enum RunnerCommand<'a> {
    Interactive,
    Init {
        seeds: Vec<PathBuf>,
        script: PathBuf,
    },
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
    command: &'a RunnerCommand<'a>,
    host_home: &'a Path,
    host_work: &'a Path,
}

/// Manage sandboxed development environments.
#[derive(Debug, Parser)]
// clap shows only brief help messages (the top line of comments) with `-h` and
// longer messages with `--help`. This custom help message gives people some
// hope of learning that distinction. See
// <https://github.com/clap-rs/clap/issues/1015>.
#[clap(help_message("Print help information. Use --help for more details"))]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate tab-completions for your shell.
    ///
    /// Installation for Bash:
    ///
    ///     $ cub completions bash > ~/.local/share/bash-completion/completions/cub
    ///
    /// Installation for ZSH (depending on `$fpath`):
    ///
    ///     $ cub completions zsh > ~/.zfunc/_cub
    ///
    /// You may need to restart your shell or configure it.
    ///
    /// This installation works similarly as for rustup's completions. For
    /// detailed instructions, see:
    ///
    ///     $ rustup help completions
    #[clap(arg_required_else_help(true))]
    Completions {
        #[clap(value_parser)]
        shell: Shell,
    },

    /// Run a shell in an existing environment.
    #[clap(arg_required_else_help(true))]
    Enter {
        /// Environment name.
        name: EnvironmentName,
    },

    /// Run a command in an existing environment.
    #[clap(arg_required_else_help(true))]
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

    /// Show available packages.
    Packages {
        /// Set output format.
        #[clap(long, value_enum, default_value_t)]
        format: ListPackagesFormat,
    },

    /// Create a new environment.
    #[clap(arg_required_else_help(true))]
    New {
        /// Run a shell in new environment.
        #[clap(long)]
        enter: bool,
        /// Comma-separated names of packages to inject into home directory.
        #[clap(long, use_value_delimiter(true))]
        packages: Option<Vec<String>>,
        /// New environment name.
        name: EnvironmentName,
    },

    /// Delete environment(s) and their work directories.
    #[clap(arg_required_else_help(true))]
    Purge {
        /// Environment name(s).
        #[clap(required(true))]
        names: Vec<EnvironmentName>,
    },

    /// Recreate an environment (keeping its work directory).
    #[clap(arg_required_else_help(true))]
    Reset {
        /// Remove home directory and do not recreate it.
        #[clap(long)]
        clean: bool,
        /// Comma-separated names of packages to inject into home directory.
        #[clap(long, use_value_delimiter(true))]
        packages: Option<Vec<String>>,
        /// Environment name(s).
        #[clap(required(true))]
        names: Vec<EnvironmentName>,
    },

    /// Create and enter a new temporary environment.
    Tmp {
        /// Comma-separated names of packages to inject into home directory.
        #[clap(long, use_value_delimiter(true))]
        packages: Option<Vec<String>>,
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

enum RunnerKind {
    #[cfg(target_os = "linux")]
    Bubblewrap,
    Docker,
}

fn get_runner(script_path: &Path) -> Result<RunnerKind> {
    let runner_path = script_path.join(".RUNNER");
    let runners = "'bubblewrap' (on Linux only) or 'docker'";
    match std::fs::read_to_string(&runner_path)
        .with_context(|| format!("Could not read {:?}. Expected {}.", runner_path, runners))?
        .trim()
    {
        #[cfg(target_os = "linux")]
        "bubblewrap" => Ok(RunnerKind::Bubblewrap),
        #[cfg(not(target_os = "linux"))]
        "bubblewrap" => Err(anyhow!("Bubblewrap is only supported on Linux")),
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
    use Commands::*;
    let program = Cubicle::new()?;
    let args = Args::parse();
    match args.command {
        Completions { shell } => {
            use clap::CommandFactory;
            let cmd = &mut Args::command();
            let out = &mut io::stdout();

            // We can't list out environment names and package names
            // statically. Unfortunately, there seems to be no general way to
            // tell `clap` about these dynamic lists. For ZSH, we hack calls to
            // this program into the generated output. (Similar contributions
            // would be welcome for Bash).
            if shell == Shell::Zsh {
                let mut buf: Vec<u8> = Vec::new();
                generate(shell, cmd, "cub", &mut buf);
                let buf = String::from_utf8(buf)?;
                let mut counts = [0; 4];
                for line in buf.lines() {
                    match line {
                        r#"':name -- Environment name:' \"# => {
                            counts[0] += 1;
                            println!(r#"':name -- Environment name:_cub_envs' \"#)
                        }
                        r#"'*::names -- Environment name(s):' \"# => {
                            counts[1] += 1;
                            println!(r#"'*::names -- Environment name(s):_cub_envs' \"#)
                        }
                        r#"'*--packages=[Comma-separated names of packages to inject into home directory]:PACKAGES: ' \"# =>
                        {
                            counts[2] += 1;
                            println!(
                                r#"'*--packages=[Comma-separated names of packages to inject into home directory]:PACKAGES:_cub_pkgs' \"#
                            )
                        }
                        r#"_cub "$@""# => {
                            counts[3] += 1;
                            println!(
                                "{}",
                                r#"
_cub_envs() {
    _values -w 'environments' $(cub list --format=names)
}
_cub_pkgs() {
    _values -s , -w 'packages' $(cub packages --format=names)
}
"#
                            );
                            println!("{}", line);
                        }
                        _ => println!("{}", line),
                    }
                }
                debug_assert_eq!(counts, [2, 2, 3, 1], "completions not patched as expected",);
            } else {
                generate(shell, cmd, "cub", out);
            }
            Ok(())
        }

        Enter { name } => program.enter_environment(&name),
        Exec { name, command } => program.exec_environment(&name, &command),
        List { format } => program.list_environments(format),
        New {
            name,
            enter,
            packages,
        } => {
            let packages = packages.map(package_set_from_names).transpose()?;
            program.new_environment(&name, packages)?;
            if enter {
                program.enter_environment(&name)?;
            }
            Ok(())
        }
        Packages { format } => program.list_packages(format),
        Purge { names } => {
            for name in names {
                program.purge_environment(&name, Quiet(false))?;
            }
            Ok(())
        }
        // TODO: rename
        Reset {
            names,
            clean,
            packages,
        } => {
            let packages = packages.map(package_set_from_names).transpose()?;
            for name in &names {
                program.reset_environment(name, &packages, Clean(clean))?;
            }
            Ok(())
        }

        Tmp { packages } => {
            let packages = packages.map(package_set_from_names).transpose()?;
            program.create_enter_tmp_environment(packages)
        }
    }
}
