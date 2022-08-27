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
// END OF HEADER

//! This test ensures that each of the files that form crate roots of cargo
//! targets (see <https://doc.rust-lang.org/cargo/guide/project-layout.html>)
//! start with the header at the top of this file.
//!
//! The purpose of this is to ensure the entire project is checked by the same
//! Clippy lints. Currently (2022), Clippy configs must be defined in every
//! crate root or every invocation to `cargo clippy`.
//!
//! There is a vague plan to allow Clippy to be configured globally by Cargo
//! but not much progress yet. See
//! <https://github.com/rust-lang/rust-clippy/issues/1313> and
//! <https://github.com/rust-lang/cargo/issues/5034>.
//!
//! The problems with using command-line arguments is that it's easy to forget
//! them, and editors and such need to be reconfigured.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

use cubicle::somehow::Result;

fn targets(manifest_dir: &Path) -> Result<Vec<PathBuf>> {
    #[derive(Debug, Deserialize)]
    struct Metadata {
        packages: Vec<Package>,
    }

    #[derive(Debug, Deserialize)]
    struct Package {
        targets: Vec<Target>,
    }

    #[derive(Debug, Deserialize)]
    struct Target {
        src_path: PathBuf,
    }

    // This can alternatively be done with the cargo package, but that
    // package takes a while to build.
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--no-deps")
        .output()?;
    assert!(output.status.success(), "cargo metadata failed");

    let metadata: Metadata = serde_json::from_slice(&output.stdout)?;
    let mut roots = Vec::new();
    for package in metadata.packages {
        for target in package.targets {
            roots.push(target.src_path);
        }
    }

    // check some known paths to make sure they're included
    for path in [
        "src/lib.rs",
        "src/main.rs",
        "src/bin/system_test/main.rs",
        file!(),
    ] {
        let path = manifest_dir.join(path);
        assert!(&roots.contains(&path));
    }

    Ok(roots)
}

#[test]
fn roots() -> Result<()> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let source = std::fs::read_to_string(manifest_dir.join(file!()))?;
    let (header, _) = source
        .split_once("// END OF HEADER")
        .expect("should have END OF HEADER comment");

    for target in targets(&manifest_dir)? {
        let source = std::fs::read_to_string(&target)?;
        let ok = source.starts_with(header);
        assert!(ok, "{target:?} should start with exact header:\n{header}");
    }

    Ok(())
}
