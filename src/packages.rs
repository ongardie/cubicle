use clap::ValueEnum;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, BufRead, Write};
use std::iter;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime};
use tempfile::NamedTempFile;

use crate::somehow::{somehow as anyhow, Context, Error, Result};

use super::fs_util::{
    create_tar_from_dir, file_size, summarize_dir, try_exists, try_iterdir, DirSummary, TarOptions,
};
use super::{
    rel_time, time_serialize, time_serialize_opt, Bytes, Cubicle, EnvironmentExists,
    EnvironmentName, HostPath, Quiet, RunCommand, Runner,
};

pub struct PackageSpec {
    // TODO: these shouldn't need to be public
    pub build_depends: PackageNameSet,
    pub depends: PackageNameSet,
    dir: HostPath,
    origin: String,
    update: Option<String>,
    test: Option<String>,
}

type PackageSpecs = BTreeMap<PackageName, PackageSpec>;

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
    fn add_packages(
        &self,
        packages: &mut BTreeMap<PackageName, PackageSpec>,
        dir: &HostPath,
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

            let test = try_exists(&dir.join("test.sh"))
                .todo_context()?
                .then_some(String::from("./test.sh"));
            let update = try_exists(&dir.join("update.sh"))
                .todo_context()?
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

    fn scan_package_names(&self) -> Result<PackageNameSet> {
        let mut names = PackageNameSet::new();
        let mut add = |dir: &HostPath| -> Result<()> {
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

    pub(super) fn scan_packages(&self) -> Result<PackageSpecs> {
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

    pub(super) fn update_packages(
        &self,
        packages: &PackageNameSet,
        specs: &PackageSpecs,
    ) -> Result<()> {
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
        let metadata = std::fs::metadata(path.as_host_raw()).ok()?;
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

    pub(super) fn update_package(
        &self,
        package_name: &PackageName,
        spec: &PackageSpec,
    ) -> Result<()> {
        self.update_package_(package_name, spec)
            .with_context(|| format!("Failed to update package: {package_name}"))
    }

    fn update_package_(&self, package_name: &PackageName, spec: &PackageSpec) -> Result<()> {
        println!("Updating {package_name} package");
        let env_name = EnvironmentName::from_str(&format!("package-{package_name}")).unwrap();
        let package_cache = &self.shared.package_cache;

        {
            let tar_file = NamedTempFile::new().todo_context()?;
            create_tar_from_dir(
                &spec.dir,
                tar_file.as_file(),
                &TarOptions {
                    prefix: Some(PathBuf::from("w")),
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
                    extra_seeds: &[&HostPath::try_from(tar_file.path().to_owned())?],
                },
            ) {
                let cached = package_cache.join(format!("{package_name}.tar"));
                if try_exists(&cached).todo_context()? {
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

        std::fs::create_dir_all(&package_cache.as_host_raw()).todo_context()?;
        let package_cache_dir = cap_std::fs::Dir::open_ambient_dir(
            &package_cache.as_host_raw(),
            cap_std::ambient_authority(),
        )
        .todo_context()?;

        match &spec.test {
            None => {
                let mut file = package_cache_dir
                    .open_with(
                        &format!("{package_name}.tar"),
                        cap_std::fs::OpenOptions::new().create(true).write(true),
                    )
                    .todo_context()?;
                self.runner
                    .copy_out_from_home(&env_name, Path::new("provides.tar"), &mut file)
            }

            Some(test_script) => {
                println!("Testing {package_name} package");
                let test_name =
                    EnvironmentName::from_str(&format!("test-package-{package_name}")).unwrap();

                let tar_file = NamedTempFile::new().todo_context()?;
                create_tar_from_dir(
                    &spec.dir,
                    tar_file.as_file(),
                    &TarOptions {
                        prefix: Some(PathBuf::from("w")),
                        // `dev-init.sh` will run `update.sh` if it's present, but
                        // we don't want that
                        exclude: vec![PathBuf::from("update.sh")],
                    },
                )
                .with_context(|| format!("Failed to tar package source to test {package_name}"))?;

                let testing_tar = format!("{package_name}.testing.tar");
                {
                    let mut file = package_cache_dir
                        .open_with(
                            &testing_tar,
                            cap_std::fs::OpenOptions::new().create(true).write(true),
                        )
                        .todo_context()?;
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
                            extra_seeds: &[
                                &HostPath::try_from(tar_file.path().to_owned())?,
                                &package_cache.join(&testing_tar),
                            ],
                        },
                    )
                    .and_then(|_| self.run(&test_name, &RunCommand::Exec(&[test_script.clone()])));
                if let Err(e) = result {
                    let cached = package_cache.join(format!("{package_name}.tar"));
                    if try_exists(&cached).todo_context()? {
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
                    &package_cache.join(testing_tar).as_host_raw(),
                    &package_cache
                        .join(format!("{package_name}.tar"))
                        .as_host_raw(),
                )
                .todo_context()?;
                Ok(())
            }
        }
    }

    /// Corresponds to `cub packages`.
    pub fn list_packages(&self, format: ListPackagesFormat) -> Result<()> {
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
                    match std::fs::metadata(
                        &self
                            .shared
                            .package_cache
                            .join(format!("{name}.tar"))
                            .as_host_raw(),
                    ) {
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
                        dir: spec.dir.as_host_raw().to_owned(),
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
                println!(
                    "{}",
                    serde_json::to_string_pretty(&packages)
                        .context("failed to serialize JSON while listing packages")?
                );
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

    pub(super) fn read_package_list_from_env(
        &self,
        name: &EnvironmentName,
    ) -> Result<Option<PackageNameSet>> {
        let mut buf = Vec::new();
        self.runner
            .copy_out_from_work(name, Path::new("packages.txt"), &mut buf)?;
        let reader = io::BufReader::new(buf.as_slice());
        let names = reader
            .lines()
            .map(|name| match name {
                Ok(name) => PackageName::from_str(&name),
                Err(e) => Err(e).todo_context(),
            })
            .collect::<Result<PackageNameSet>>()
            .todo_context()?;
        Ok(Some(names))
    }

    pub(super) fn packages_to_seeds(&self, packages: &PackageNameSet) -> Result<Vec<HostPath>> {
        let mut seeds = Vec::with_capacity(packages.len());
        let specs = self.scan_packages()?;
        let deps = transitive_depends(packages, &specs, BuildDepends(false));
        for package in deps {
            let provides = self.shared.package_cache.join(format!("{package}.tar"));
            if try_exists(&provides).todo_context()? {
                seeds.push(provides);
            }
        }
        Ok(seeds)
    }
}

/// The name of a potential Cubicle package.
///
/// Other than '-' and '_' and some non-ASCII characters, values of this type
/// may not contain whitespace or special characters.
#[derive(Debug, Clone, Eq, Ord, PartialOrd, PartialEq, Serialize)]
pub struct PackageName(String);

impl FromStr for PackageName {
    type Err = Error;
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

/// An ordered set of package names.
pub type PackageNameSet = BTreeSet<PackageName>;

/// Allowed formats for [`Cubicle::list_packages`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ListPackagesFormat {
    /// Human-formatted table.
    #[default]
    Default,
    /// Detailed JSON output for machine consumption.
    Json,
    /// Newline-delimited list of package names only.
    Names,
}

fn read_package_list(dir: &HostPath, path: &str) -> Result<Option<PackageNameSet>> {
    let dir = cap_std::fs::Dir::open_ambient_dir(dir.as_host_raw(), cap_std::ambient_authority())
        .todo_context()?;
    let file = match dir.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).todo_context(),
    };
    let reader = io::BufReader::new(file);
    let names = reader
        .lines()
        .map(|name| match name {
            Ok(name) => PackageName::from_str(&name),
            Err(e) => Err(e).todo_context(),
        })
        .collect::<Result<PackageNameSet>>()
        .todo_context()?;
    Ok(Some(names))
}

pub fn write_package_list_tar(packages: &PackageNameSet) -> Result<tempfile::NamedTempFile> {
    let file = tempfile::NamedTempFile::new().todo_context()?;
    let metadata = file.as_file().metadata().todo_context()?;
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
        writeln!(buf, "{package}").todo_context()?;
    }
    header.set_size(buf.len() as u64);
    builder
        .append_data(
            &mut header,
            Path::new("w").join("packages.txt"),
            buf.as_slice(),
        )
        .todo_context()?;
    builder
        .into_inner()
        .and_then(|mut f| f.flush())
        .todo_context()?;
    Ok(file)
}
