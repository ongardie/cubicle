use clap::ValueEnum;
use serde::Serialize;
use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Display};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;
use tempfile::NamedTempFile;

use crate::somehow::{somehow as anyhow, warn, Context, Error, LowLevelResult, Result};

use super::fs_util::{
    create_tar_from_dir, file_size, summarize_dir, try_exists, try_iterdir, DirSummary, TarOptions,
};
use super::runner::{EnvironmentExists, Init, Runner, RunnerCommand};
use super::{
    rel_time, time_serialize, time_serialize_opt, Bytes, Cubicle, EnvironmentName, HostPath,
    RunnerKind,
};

mod manifest;
use manifest::{Dependency, Manifest};

/// Information about a package's source files.
pub struct PackageSpec {
    manifest: Manifest,
    dir: HostPath,
    origin: String,
    update: Option<String>,
    test: Option<String>,
}

/// Information about all available package sources.
///
/// Some package-related methods in [`Cubicle`] need this. Use
/// [`Cubicle::scan_packages`] to build one.
pub type PackageSpecs = BTreeMap<PackageName, PackageSpec>;

/// Used in [`Cubicle::update_packages`] to describe when packages should be
/// updated.
pub struct UpdatePackagesConditions {
    /// When should the named packages' transitive dependencies and
    /// build-dependencies be updated?
    pub dependencies: ShouldPackageUpdate,
    /// When should the given packages themselves be updated?
    pub named: ShouldPackageUpdate,
}

/// Describes when a package should be updated.
///
/// See [`Cubicle::update_packages`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShouldPackageUpdate {
    /// The package should be re-built.
    Always,

    /// The package should be re-built if:
    /// - It has not been successfully built for over
    ///   [`Config::auto_update`](crate::Config::auto_update) time,
    /// - Its source files have been updated since it was built, or
    /// - One of its transitive dependencies has been updated since it was
    ///   built.
    IfStale,

    /// The package should be built only if it's never successfully been built
    /// before.
    IfRequired,
}

#[derive(Clone, Copy)]
struct BuildDepends(bool);

fn transitive_depends(
    packages: &PackageNameSet,
    specs: &PackageSpecs,
    build_depends: BuildDepends,
) -> Result<PackageNameSet> {
    fn visit(
        specs: &PackageSpecs,
        build_depends: BuildDepends,
        visited: &mut BTreeSet<PackageName>,
        p: &PackageName,
        needed_by: Option<&PackageName>,
    ) -> Result<()> {
        if !visited.contains(p) {
            visited.insert(p.clone());
            let spec = specs.get(p).ok_or_else(|| match needed_by {
                Some(other) => {
                    anyhow!("could not find package definition for {p}, needed by {other}")
                }
                None => anyhow!("could not find package definition for {p}"),
            })?;
            for q in spec.manifest.root_depends().keys() {
                visit(
                    specs,
                    build_depends,
                    visited,
                    &PackageName::from_str(q).expect("todo"),
                    Some(p),
                )?;
            }
            if build_depends.0 {
                for q in spec.manifest.root_build_depends().keys() {
                    visit(
                        specs,
                        build_depends,
                        visited,
                        &PackageName::from_str(q).expect("todo"),
                        Some(p),
                    )?;
                }
            }
        }
        Ok(())
    }

    let mut visited = BTreeSet::new();
    for p in packages.iter() {
        visit(specs, build_depends, &mut visited, p, None)?;
    }
    Ok(visited)
}

impl Cubicle {
    pub(super) fn resolve_debian_packages(
        &self,
        packages: &PackageNameSet,
        specs: &PackageSpecs,
    ) -> Result<BTreeSet<String>> {
        let strict = match self.shared.config.runner {
            RunnerKind::Bubblewrap => true,
            RunnerKind::Docker => self.shared.config.docker.strict_debian_packages,
            RunnerKind::User => true,
        };
        if strict {
            strict_debian_packages(packages, specs)
        } else {
            all_debian_packages(specs)
        }
    }

    fn add_packages(
        &self,
        packages: &mut PackageSpecs,
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

            let mut manifest = match Manifest::read(&dir, "package.toml").with_context(|| {
                format!(
                    "could not read manifest for package {name}: {:?}",
                    dir.join("package.toml").as_host_raw()
                )
            })? {
                Some(manifest) => manifest,
                None => {
                    warn(anyhow!(
                        "no manifest found for package {name}: missing {:?}",
                        dir.join("package.toml").as_host_raw()
                    ));
                    continue;
                }
            };

            manifest
                .depends
                .get_mut(PackageNamespace::root())
                .unwrap()
                .insert(String::from("auto"), Dependency {});

            let test = try_exists(&dir.join("test.sh"))
                .todo_context()?
                .then_some(String::from("./test.sh"));
            let update = try_exists(&dir.join("update.sh"))
                .todo_context()?
                .then_some(String::from("./update.sh"));
            packages.insert(
                name,
                PackageSpec {
                    manifest,
                    dir,
                    origin: origin.to_owned(),
                    test,
                    update,
                },
            );
        }
        Ok(())
    }

    /// Returns a list of available packages.
    pub fn get_package_names(&self) -> Result<PackageNameSet> {
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

    /// Returns information about available package sources.
    pub fn scan_packages(&self) -> Result<PackageSpecs> {
        let mut specs = PackageSpecs::new();

        for dir in try_iterdir(&self.shared.user_package_dir)? {
            let origin = dir.to_string_lossy();
            self.add_packages(
                &mut specs,
                &self.shared.user_package_dir.join(&dir),
                &origin,
            )?;
        }

        self.add_packages(&mut specs, &self.shared.code_package_dir, "built-in")?;

        let auto = PackageName::from_str("auto").unwrap();
        let auto_deps =
            transitive_depends(&PackageNameSet::from([auto]), &specs, BuildDepends(true))?;
        for name in auto_deps {
            let spec = specs.get_mut(&name).unwrap();
            spec.manifest
                .depends
                .get_mut(PackageNamespace::root())
                .unwrap()
                .remove("auto");
        }

        Ok(specs)
    }

    /// Rebuilds some of the given packages and their transitive dependencies,
    /// as requested.
    pub fn update_packages(
        &self,
        packages: &PackageNameSet,
        specs: &PackageSpecs,
        conditions: UpdatePackagesConditions,
    ) -> Result<()> {
        let now = SystemTime::now();
        let mut todo: Vec<PackageName> =
            Vec::from_iter(transitive_depends(packages, specs, BuildDepends(true))?);
        let mut done = BTreeSet::new();
        loop {
            let start_todos = todo.len();
            if start_todos == 0 {
                return Ok(());
            }
            let mut later = Vec::new();
            for name in todo {
                if let Some(spec) = specs.get(&name) {
                    if spec
                        .manifest
                        .root_depends()
                        .keys()
                        .all(|dep| done.contains(dep.as_str()))
                        && spec
                            .manifest
                            .root_build_depends()
                            .keys()
                            .all(|dep| done.contains(dep.as_str()))
                    {
                        let needs_build = {
                            if spec.update.is_none() {
                                false
                            } else {
                                let when = if packages.contains(&name) {
                                    conditions.named
                                } else {
                                    conditions.dependencies
                                };
                                match when {
                                    ShouldPackageUpdate::Always => true,
                                    ShouldPackageUpdate::IfStale => {
                                        self.package_is_stale(&name, spec, now)?
                                    }
                                    ShouldPackageUpdate::IfRequired => {
                                        self.last_built(&name).is_none()
                                    }
                                }
                            }
                        };
                        if needs_build {
                            self.update_package(&name, spec, specs)?;
                        }
                        done.insert(name);
                    } else {
                        later.push(name);
                    }
                } else {
                    return Err(anyhow!("could not find package definition for `{name}`"));
                }
            }
            if later.len() == start_todos {
                later.sort();
                return Err(anyhow!(
                    "package dependencies are unsatisfiable for: {later:?}"
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

    fn package_is_stale(
        &self,
        package_name: &PackageName,
        spec: &PackageSpec,
        now: SystemTime,
    ) -> Result<bool> {
        let built = match self.last_built(package_name) {
            Some(built) => built,
            None => return Ok(true),
        };
        if let Some(threshold) = self.shared.config.auto_update {
            match now.duration_since(built) {
                Ok(d) if d > threshold => return Ok(true),
                Err(_) => return Ok(true),
                _ => {}
            }
        }
        let DirSummary { last_modified, .. } = summarize_dir(&spec.dir)?;
        if last_modified > built {
            return Ok(true);
        }
        for p in spec
            .manifest
            .root_build_depends()
            .keys()
            .chain(spec.manifest.root_depends().keys())
        {
            let p = PackageName::from_str(p).expect("todo");
            if matches!(self.last_built(&p), Some(b) if b > built) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn package_build_failed(&self, package_name: &PackageName) -> Result<bool> {
        let failed_marker = &self
            .shared
            .package_cache
            .join(format!("{package_name}.failed"));
        try_exists(failed_marker)
            .with_context(|| format!("error while checking if {failed_marker:?} exists"))
    }

    fn update_package(
        &self,
        package_name: &PackageName,
        spec: &PackageSpec,
        specs: &PackageSpecs,
    ) -> Result<()> {
        let package_cache = &self.shared.package_cache;
        let failed_marker = package_cache.join(format!("{package_name}.failed"));

        match self
            .update_package_(package_name, spec, specs)
            .with_context(|| format!("failed to update package: {package_name}"))
        {
            Ok(_) => {
                if let Err(e) = std::fs::remove_file(failed_marker.as_host_raw()) {
                    if e.kind() != io::ErrorKind::NotFound {
                        return Err(e).context(format!(
                            "failed to remove file {failed_marker:?} after \
                            successfully updating package {package_name:?}"
                        ));
                    }
                }
                Ok(())
            }
            Err(update_error) => {
                std::fs::create_dir_all(package_cache.as_host_raw())
                    .with_context(|| format!("failed to create directory {package_cache:?}"))?;
                if let Err(e2) = std::fs::File::create(failed_marker.as_host_raw())
                    .with_context(|| format!("failed to create file {failed_marker:?}"))
                {
                    warn(e2);
                }
                let cached = package_cache.join(format!("{package_name}.tar"));
                let use_stale = match try_exists(&cached)
                    .with_context(|| format!("error while checking if {cached:?} exists"))
                {
                    Ok(exists) => exists,
                    Err(e2) => {
                        warn(e2);
                        false
                    }
                };
                if use_stale {
                    warn(update_error.context(format!("using stale version of {package_name}")));
                    Ok(())
                } else {
                    Err(update_error)
                }
            }
        }
    }

    fn update_package_(
        &self,
        package_name: &PackageName,
        spec: &PackageSpec,
        specs: &PackageSpecs,
    ) -> LowLevelResult<()> {
        println!("Updating {package_name} package");
        let env_name = EnvironmentName::for_builder_package(package_name);
        self.build_package(package_name, &env_name, spec, specs)
            .with_context(|| format!("error building package {package_name}"))?;

        let package_cache = &self.shared.package_cache;
        std::fs::create_dir_all(&package_cache.as_host_raw())
            .with_context(|| format!("failed to create directory {package_cache:?}"))?;
        let package_cache_dir = cap_std::fs::Dir::open_ambient_dir(
            &package_cache.as_host_raw(),
            cap_std::ambient_authority(),
        )
        .with_context(|| format!("failed to open directory {package_cache:?}"))?;

        let testing_tar_name = format!("{package_name}.testing.tar");
        let testing_tar_abs = package_cache.join(&testing_tar_name);
        {
            let mut file = package_cache_dir
                .open_with(
                    &testing_tar_name,
                    cap_std::fs::OpenOptions::new().create(true).write(true),
                )
                .with_context(|| {
                    format!(
                        "failed to create file for package build output: {:?}",
                        testing_tar_abs,
                    )
                })?;
            self.runner
                .copy_out_from_home(&env_name, Path::new("provides.tar"), &mut file)
                .with_context(|| {
                    format!(
                        "failed to copy package build output from `~/provides.tar` on {env_name} to {:?}",
                        testing_tar_abs,
                    )
                })?;
        }

        if let Some(test_script) = &spec.test {
            self.test_package(package_name, testing_tar_abs, test_script, spec, specs)
                .with_context(|| format!("error testing package {package_name}"))?;
        }

        let package_tar = format!("{package_name}.tar");
        package_cache_dir
            .rename(&testing_tar_name, &package_cache_dir, &package_tar)
            .with_context(|| {
                format!(
                    "failed to rename {testing_tar_name:?} to {package_tar:?} in {package_cache:?}"
                )
            })?;
        Ok(())
    }

    fn build_package(
        &self,
        package_name: &PackageName,
        env_name: &EnvironmentName,
        spec: &PackageSpec,
        specs: &PackageSpecs,
    ) -> Result<()> {
        let packages: PackageNameSet = spec
            .manifest
            .root_build_depends()
            .keys()
            .chain(spec.manifest.root_depends().keys())
            .map(|name| PackageName::from_str(name).expect("todo"))
            .collect();

        let mut debian_packages = self.resolve_debian_packages(&packages, specs)?;
        if let Some(debian) = spec.manifest.depends.get("debian") {
            debian_packages.extend(debian.keys().cloned());
        }
        if let Some(debian) = spec.manifest.build_depends.get("debian") {
            debian_packages.extend(debian.keys().cloned());
        }

        let mut seeds = self.packages_to_seeds(&packages)?;

        let tar_file = NamedTempFile::new().todo_context()?;
        create_tar_from_dir(
            &spec.dir,
            tar_file.as_file(),
            &TarOptions {
                prefix: Some(PathBuf::from("w")),
                ..TarOptions::default()
            },
        )
        .with_context(|| format!("failed to tar package source for {package_name}"))?;
        seeds.push(HostPath::try_from(tar_file.path().to_owned()).unwrap());

        let init = Init {
            debian_packages,
            seeds,
            script: self.shared.script_path.join("dev-init.sh"),
        };

        use EnvironmentExists::*;
        match self.runner.exists(env_name)? {
            FullyExists | PartiallyExists => self.runner.reset(env_name, &init),
            NoEnvironment => self.runner.create(env_name, &init),
        }
    }

    fn test_package(
        &self,
        package_name: &PackageName,
        testing_tar: HostPath,
        test_script: &str,
        spec: &PackageSpec,
        specs: &PackageSpecs,
    ) -> Result<()> {
        println!("Testing {package_name} package");
        let test_name =
            EnvironmentName::from_str(&format!("test-package-{}", package_name.as_str())).unwrap();

        self.runner.purge(&test_name)?;

        let packages = spec
            .manifest
            .root_depends()
            .keys()
            .map(|name| PackageName::from_str(name).expect("todo"))
            .collect();
        let mut seeds = self.packages_to_seeds(&packages)?;
        seeds.push(testing_tar);

        let mut debian_packages = self.resolve_debian_packages(&packages, specs)?;
        if let Some(debian) = spec.manifest.depends.get("debian") {
            debian_packages.extend(debian.keys().cloned());
        }

        {
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
            .with_context(|| format!("failed to tar package source to test {package_name}"))?;
            seeds.push(HostPath::try_from(tar_file.path().to_owned()).unwrap());

            self.runner.create(
                &test_name,
                &Init {
                    debian_packages,
                    seeds,
                    script: self.shared.script_path.join("dev-init.sh"),
                },
            )?;
        }

        self.runner
            .run(&test_name, &RunnerCommand::Exec(&[test_script.to_owned()]))?;
        self.runner.purge(&test_name)
    }

    /// Returns details of available packages.
    pub fn get_packages(&self) -> Result<BTreeMap<PackageName, PackageDetails>> {
        self.scan_packages()?
            .into_iter()
            .map(|(name, spec)| -> Result<(PackageName, PackageDetails)> {
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
                let last_build_failed = self.package_build_failed(&name)?;
                Ok((
                    name,
                    PackageDetails {
                        build_depends: spec
                            .manifest
                            .build_depends
                            .into_iter()
                            .map(|(namespace, packages)| {
                                (namespace.0, packages.into_keys().collect())
                            })
                            .collect(),
                        built,
                        depends: spec
                            .manifest
                            .depends
                            .into_iter()
                            .map(|(namespace, packages)| {
                                (namespace.0, packages.into_keys().collect())
                            })
                            .collect(),
                        dir: spec.dir.as_host_raw().to_owned(),
                        edited,
                        last_build_failed,
                        origin: spec.origin,
                        size,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()
    }

    /// Corresponds to `cub package list`.
    pub fn list_packages(&self, format: ListPackagesFormat) -> Result<()> {
        use ListPackagesFormat::*;
        match format {
            Names => {
                for name in self.get_package_names()? {
                    println!("{}", name);
                }
            }

            Json => {
                let packages = self.get_packages()?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&packages)
                        .context("failed to serialize JSON while listing packages")?
                );
            }

            Default => {
                let packages = self.get_packages()?;
                let nw = packages
                    .keys()
                    .map(|name| name.as_str().len())
                    .max()
                    .unwrap_or(10);
                let now = SystemTime::now();
                println!(
                    "{:<nw$}  {:<8}  {:>10}  {:>13}  {:>13}  {:>8}",
                    "name", "origin", "size", "built", "edited", "status"
                );
                println!(
                    "{0:-<nw$}  {0:-<8}  {0:-<10}  {0:-<13}  {0:-<13}  {0:-<8}",
                    ""
                );
                for (name, package) in packages {
                    println!(
                        "{:<nw$}  {:<8}  {:>10}  {:>13}  {:>13}  {:>8}",
                        name.as_str(),
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
                        if package.last_build_failed {
                            "failed"
                        } else {
                            "ok"
                        },
                    );
                }
            }
        }
        Ok(())
    }

    pub(super) fn read_package_list_from_env(
        &self,
        name: &EnvironmentName,
    ) -> Result<PackageNameSet> {
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
        Ok(names)
    }

    pub(super) fn packages_to_seeds(&self, packages: &PackageNameSet) -> Result<Vec<HostPath>> {
        let mut seeds = Vec::with_capacity(packages.len());
        let specs = self.scan_packages()?;
        let deps = transitive_depends(packages, &specs, BuildDepends(false))?;
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
#[derive(Clone, Debug, Eq, Ord, PartialOrd, PartialEq, Serialize)]
pub struct PackageName(String);

impl PackageName {
    /// Returns a string slice representing the package name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for PackageName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

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

impl Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

/// An ordered set of package names.
pub type PackageNameSet = BTreeSet<PackageName>;

#[derive(Debug, Clone, Eq, Ord, PartialOrd, PartialEq, Serialize)]
pub struct PackageNamespace(String);

impl PackageNamespace {
    fn root() -> &'static str {
        "cubicle"
    }

    fn root_owned() -> PackageNamespace {
        Self(Self::root().to_owned())
    }
}

impl Borrow<str> for PackageNamespace {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl FromStr for PackageNamespace {
    type Err = Error;
    fn from_str(mut s: &str) -> Result<Self> {
        s = s.trim();
        if s.is_empty() {
            return Err(anyhow!("package namespace cannot be empty"));
        }
        if s.contains(|c: char| {
            (c.is_ascii() && !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_'))
                || c.is_control()
                || c.is_whitespace()
        }) {
            return Err(anyhow!(
                "package namespace cannot contain special characters"
            ));
        }
        Ok(Self(s.to_owned()))
    }
}

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
        writeln!(buf, "{}", package.as_str()).todo_context()?;
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

fn strict_debian_packages(
    packages: &PackageNameSet,
    specs: &PackageSpecs,
) -> Result<BTreeSet<String>> {
    fn visit(
        specs: &PackageSpecs,
        visited: &mut BTreeSet<PackageName>,
        debian_packages: &mut BTreeSet<String>,
        p: &PackageName,
    ) {
        if !visited.contains(p) {
            visited.insert(p.clone());
            let spec = specs.get(p).expect("todo");
            if let Some(debian) = spec.manifest.depends.get("debian") {
                debian_packages.extend(debian.keys().cloned());
            }
            for q in spec.manifest.root_depends().keys() {
                visit(
                    specs,
                    visited,
                    debian_packages,
                    &PackageName::from_str(q).expect("todo"),
                );
            }
        }
    }

    let mut visited = BTreeSet::new();
    let mut debian_packages = BTreeSet::new();
    for p in packages.iter() {
        visit(specs, &mut visited, &mut debian_packages, p);
    }
    Ok(debian_packages)
}

fn all_debian_packages(specs: &PackageSpecs) -> Result<BTreeSet<String>> {
    let mut debian_packages = BTreeSet::new();
    for spec in specs.values() {
        if let Some(debian) = spec.manifest.depends.get("debian") {
            debian_packages.extend(debian.keys().cloned());
        }
        if let Some(debian) = spec.manifest.build_depends.get("debian") {
            debian_packages.extend(debian.keys().cloned());
        }
    }
    Ok(debian_packages)
}

/// Description of a package as returned by [`Cubicle::get_packages`].
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct PackageDetails {
    /// Map from package namespaces to package names for packages this package
    /// needs at build-time.
    pub build_depends: BTreeMap<String, Vec<String>>,
    #[serde(serialize_with = "time_serialize_opt")]
    /// The last time the package was successfully built, if available.
    pub built: Option<SystemTime>,
    /// Map from package namespaces to package names for packages this package
    /// needs at build-time and run-time.
    pub depends: BTreeMap<String, Vec<String>>,
    #[serde(serialize_with = "time_serialize")]
    /// The last time the package sources were changed (or `UNIX_EPOCH` if
    /// unavailable).
    pub edited: SystemTime,
    /// The path on the host to the package sources.
    pub dir: PathBuf,
    /// If true, the last completed build attempt failed. If false, either the
    /// last completed build succeeded or no build has yet completed to success
    /// or failure.
    pub last_build_failed: bool,
    /// Where the package sources came from. For package sources shipped with
    /// Cubicle, this is `"built-in"`. For local packages, it is the name of
    /// the parent directory above the package source.
    pub origin: String,
    /// The size of the last successful package build output, if available.
    pub size: Option<u64>,
}
