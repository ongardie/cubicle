#![warn(
    clippy::explicit_into_iter_loop,
    clippy::explicit_iter_loop,
    clippy::if_then_some_else_none,
    clippy::implicit_clone,
    clippy::redundant_else,
    clippy::single_match_else,
    clippy::try_err,
    clippy::unreadable_literal
)]

use clap::Parser;
use cubicle::config::Config;
use cubicle::somehow::{somehow as anyhow, Context, Result};
use cubicle::{Clean, Cubicle, EnvironmentName, ListFormat, PackageName, PackageNameSet, Quiet};
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
    let rewrite_ = |path: &Path| -> Result<()> {
        let contents = std::fs::read(path)?;
        std::fs::write(path, contents)?;
        Ok(())
    };
    let path = path.as_ref();
    rewrite_(path).with_context(|| format!("Failed to rewrite {path:?}"))
}

fn main() -> Result<()> {
    let exe = std::env::current_exe()?;
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

    let test_env = EnvironmentName::from_str("test")?;
    let configs_pkg = PackageName::from_str("configs")?;

    cub.list_environments(ListFormat::Default)?;

    cub.purge_environment(&test_env, Quiet(false))?;
    cub.new_environment(&test_env, Some(PackageNameSet::new()))?;
    cub.exec_environment(&test_env, &["ls", "-l", ".."].map(String::from))?;
    cub.reset_environment(&test_env, None, Clean(false))?;

    cub.purge_environment(&test_env, Quiet(false))?;
    cub.new_environment(&test_env, Some(PackageNameSet::from([configs_pkg])))?;
    cub.exec_environment(&test_env, &["ls", "-al", ".."].map(String::from))?;
    // This should cause the configs package to be rebuilt.
    rewrite(project_root.join("packages/configs/update.sh"))?;
    cub.reset_environment(&test_env, None, Clean(false))?;
    cub.exec_environment(&test_env, &["ls", "-al", ".."].map(String::from))?;

    cub.list_environments(ListFormat::Default)?;
    cub.purge_environment(&test_env, Quiet(false))?;

    Ok(())
}
