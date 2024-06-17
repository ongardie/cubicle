use regex::{Regex, RegexBuilder};
use std::str::FromStr;
use std::sync::OnceLock;

use crate::command_ext::Command;
use crate::somehow::{somehow as anyhow, warn, Context, Result};

#[derive(Debug)]
pub struct Summary {
    pub upgraded: usize,
    pub newly_installed: usize,
    pub removed: usize,
    #[allow(unused)]
    pub not_upgraded: usize,
}

impl Summary {
    pub fn was_satisfied(&self) -> bool {
        self.upgraded == 0 && self.newly_installed == 0 && self.removed == 0
    }
}

pub fn simulate_satisfy(deps: &[&str]) -> Result<Summary> {
    if deps.is_empty() {
        return Ok(Summary {
            upgraded: 0,
            newly_installed: 0,
            removed: 0,
            not_upgraded: 0,
        });
    }
    let output = Command::new("apt-get")
        .arg("satisfy")
        .arg("--dry-run")
        .arg("--no-install-recommends")
        .arg("--")
        .args(deps)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "`apt-get satisfy --dry-run ...` exited with {}.

    Stdout:
    {}

    Stderr:
    {}
    ",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    };

    let stdout = String::from_utf8(output.stdout)
        .context("failed to read `apt-get satisfy --dry-run ...` output")?;

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(||
        RegexBuilder::new(
            r#"^([0-9]+) upgraded, ([0-9]+) newly installed, ([0-9]+) to remove and ([0-9]+) not upgraded.$"#
        )
        .multi_line(true)
        .build()
        .unwrap());

    match re.captures(&stdout) {
        Some(caps) => {
            let count = |i| usize::from_str(caps.get(i).unwrap().as_str()).unwrap();
            Ok(Summary {
                upgraded: count(1),
                newly_installed: count(2),
                removed: count(3),
                not_upgraded: count(4),
            })
        }
        None => Err(anyhow!(
            "unexpected output from `apt-get satisfy --dry-run ...`: {stdout:?}"
        )),
    }
}

pub fn check_satisfied(deps: &[&str]) {
    match simulate_satisfy(deps) {
        Ok(summary) => {
            if !summary.was_satisfied() {
                warn(anyhow!("apt dependencies unsatisfied: {deps:?}"));
            }
        }
        Err(e) => {
            warn(e.context(format!("apt dependencies unsatisfiable: {deps:?}")));
        }
    }
}
