#![warn(
    clippy::explicit_into_iter_loop,
    clippy::explicit_iter_loop,
    clippy::if_then_some_else_none,
    clippy::implicit_clone,
    clippy::redundant_else,
    clippy::try_err,
    clippy::unreadable_literal
)]

use clap::Parser;
use cubicle::config::Config;
use cubicle::somehow::{somehow as anyhow, Context, Result};
use cubicle::{
    Cubicle, EnvironmentName, FullPackageName, ListFormat, ListPackagesFormat, Quiet,
    ShouldPackageUpdate, UpdatePackagesConditions,
};
use insta::assert_snapshot;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Parser)]
struct Args {
    /// Path to configuration file.
    #[clap(short, long, required(true), value_hint(clap::ValueHint::FilePath))]
    config: PathBuf,
}

/// Read a file's contents and write it back to disk.
///
/// The purpose of this function is to update the file modified time. It's not
/// trivial to update that metadata in a cross-platform way, so this just
/// rewrites the file instead.
fn rewrite<P: AsRef<Path>>(path: P) -> Result<()> {
    let rewrite_ = |path: &Path| -> std::io::Result<()> {
        let contents = std::fs::read(path)?;
        std::fs::write(path, contents)?;
        Ok(())
    };
    let path = path.as_ref();
    rewrite_(path).with_context(|| format!("Failed to rewrite {path:?}"))
}

fn test_package_not_found_errors(cub: &Cubicle, test_env: &EnvironmentName) -> Result<()> {
    cub.purge_environment(test_env, Quiet(false))?;

    let not_exist = BTreeSet::from([FullPackageName::from_str("does-not-exist")?]);

    // cub new --packages=does-not-exist
    let err = cub
        .new_environment(test_env, Some(&not_exist))
        .expect_err("should not be able to use does-not-exist package in `cub new`");
    assert_snapshot!(
        err.debug_without_backtrace(),
        @r###"could not find package definition for "does-not-exist""###
    );

    let envs = cub.get_environment_names()?;
    assert!(
        !envs.contains(test_env),
        "{test_env} environment should not exist"
    );

    // cub tmp --packages=does-not-exist
    let err = cub
        .create_enter_tmp_environment(Some(&not_exist))
        .expect_err("should not be able to use does-not-exist package in `cub tmp`");
    assert_snapshot!(
        err.debug_without_backtrace(),
        @r###"could not find package definition for "does-not-exist""###
    );
    let new_envs = cub
        .get_environment_names()?
        .difference(&envs)
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        new_envs.is_empty(),
        "new tmp environment should not exist, found {new_envs:?}"
    );

    // cub reset --packages=does-not-exist
    cub.new_environment(test_env, Some(&BTreeSet::new()))?;
    cub.exec_environment(test_env, &[String::from("touch"), String::from("../foo")])?;
    let err = cub
        .reset_environment(test_env, Some(&not_exist))
        .expect_err("should not be able to use does-not-exist package in `cub reset`");
    assert_snapshot!(
        err.debug_without_backtrace(),
        @r###"could not find package definition for "does-not-exist""###
    );
    cub.exec_environment(test_env, &[String::from("cat"), String::from("../foo")])
        .context("file `../foo` should still exist")?;

    // cub package update does-not-exist
    let err = cub
        .update_packages(
            &not_exist,
            &cub.scan_packages()?,
            UpdatePackagesConditions {
                dependencies: ShouldPackageUpdate::Always,
                named: ShouldPackageUpdate::Always,
            },
        )
        .expect_err("should not be able to use does-not-exist package in `cub tmp`");
    assert_snapshot!(
        err.debug_without_backtrace(),
        @r###"could not find package definition for "does-not-exist""###
    );

    Ok(())
}

fn main() -> Result<()> {
    let exe = std::env::current_exe().todo_context()?;
    let project_root = match exe.ancestors().nth(3) {
        Some(path) => path.to_owned(),
        None => {
            return Err(anyhow!(
                "could not find project root. binary run from unexpected location: {:?}",
                exe
            ));
        }
    };

    let args = Args::parse();
    let config = Config::read_from_file(&args.config)?;
    let cub = Cubicle::new(config)?;

    let test_env = EnvironmentName::from_str("system_test")?;
    let configs_pkg = FullPackageName::from_str("configs")?;

    cub.list_environments(ListFormat::Default)?;

    test_package_not_found_errors(&cub, &test_env)?;

    cub.purge_environment(&test_env, Quiet(false))?;
    cub.new_environment(&test_env, Some(&BTreeSet::new()))?;
    cub.exec_environment(&test_env, &["ls", "-l", ".."].map(String::from))?;
    cub.reset_environment(&test_env, None)?;

    cub.purge_environment(&test_env, Quiet(false))?;
    cub.new_environment(&test_env, Some(&BTreeSet::from([configs_pkg])))?;
    cub.exec_environment(&test_env, &["ls", "-al", ".."].map(String::from))?;
    // This should cause the configs package to be rebuilt.
    rewrite(project_root.join("packages/configs/build.sh"))?;
    cub.reset_environment(&test_env, None)?;
    cub.exec_environment(&test_env, &["ls", "-al", ".."].map(String::from))?;

    cub.list_environments(ListFormat::Default)?;
    cub.purge_environment(&test_env, Quiet(false))?;

    cub.list_packages(ListPackagesFormat::Default)?;
    let packages = BTreeSet::from([FullPackageName::from_str("no-op")?]);
    cub.update_packages(
        &packages,
        &cub.scan_packages()?,
        UpdatePackagesConditions {
            dependencies: ShouldPackageUpdate::Always,
            named: ShouldPackageUpdate::Always,
        },
    )?;
    cub.list_packages(ListPackagesFormat::Default)?;

    Ok(())
}
