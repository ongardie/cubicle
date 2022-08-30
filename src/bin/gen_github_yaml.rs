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

//! This program generates `.github/workflows/main.yaml`, used by GitHub
//! Actions. This program is a development time utility that is part of the
//! Cubicle project.
//!
//! To re-run it:
//!
//! ```sh
//! cargo run --bin gen_github_yaml > .github/workflows/main.yaml
//! ```
//!
//! Then review the changes with `git diff`.
//!
//! Generating the YAML programmatically is tedious but has a few benefits:
//!
//! - GitHub workflow jobs may specify a matrix of say, operating systems and
//!   Rust versions. However, those remain as a single job. Downstream jobs may
//!   only depend on entire jobs. So when using the matrix functionality, the
//!   downstream Linux jobs must wait for upstream non-Linux jobs, for example.
//!   When generating the YAML, we can just use a for loop to generate as many
//!   similar jobs as we'd like.
//!
//! - While GitHub workflows support limited if conditions and for loops
//!   (matrixes), they have some quirks such as type conversions that can be
//!   annoying to debug. Generating the YAML lets us use normal programming
//!   language functionality locally and avoid some of the more complicated
//!   features of GitHub workflows.
//!
//! - YAML has many quirks and pitfalls. By generating the YAML, we can avoid
//!   learning these and avoid most syntactic errors. YAML is easier to read
//!   and review than to write.
//!
//! - Using (mostly) static types in Rust lets us avoid some silly mistakes. In
//!   YAML, it's easy to make typos when referring to things or to place a key
//!   at the wrong level in the hierarchy.
//!
//! The types in this file do not aim to model all the functionality of GitHub
//! workflows. They are tailored specifically to the needs of Cubicle.

use indoc::indoc;
use serde::{Serialize, Serializer};
use serde_json::json;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::io::Write;

fn s(s: &str) -> String {
    String::from(s)
}

/// Generates YAML `Value`s from what looks like JSON.
///
/// See the [`serde_json::json`] macro for details, which this uses internally.
macro_rules! yaml {
    ($($json:tt)+) => {
        yaml_value_from_json(json!($($json)+))
    }
}

fn yaml_value_from_json(value: serde_json::Value) -> serde_yaml::Value {
    use serde_json::Value as json;
    use serde_yaml::Value as yaml;
    match value {
        json::Null => yaml::Null,
        json::Bool(value) => yaml::Bool(value),
        json::Number(value) => yaml::Number(serde_yaml::Number::from(value.as_f64().unwrap())),
        json::String(value) => yaml::String(value),
        json::Array(value) => yaml::Sequence(value.into_iter().map(yaml_value_from_json).collect()),
        json::Object(value) => yaml::Mapping(serde_yaml::Mapping::from_iter(
            value
                .into_iter()
                .map(|(k, v)| (yaml::String(k), yaml_value_from_json(v))),
        )),
    }
}

/// Generates YAML `Mapping`s from what looks like a JSON object.
///
/// See the [`serde_json::json`] macro for details, which this uses internally.
macro_rules! ymap {
    {$($json:tt)+} => {
        if let Value::Mapping(m) = yaml!({$($json)+}) {
            m
        } else {
            panic!("not a Mapping")
        }
    }
}

/// Returns a map from string to string with a convenient syntax.
///
/// The syntax is `{ "key" => "value", ... }`.
macro_rules! dict {
    { $( $k:expr => $v:expr ),* $( , )? } => {
        Dict::from([
            $(
                ($k.to_string(), $v.to_string()),
            )*
        ])
    }
}

type Dict = BTreeMap<String, String>;

// GitHub Actions workflow syntax documentation is here:
// <https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions>
#[derive(Debug, Serialize)]
struct Workflow {
    name: String,
    on: Mapping,
    jobs: BTreeMap<JobKey, Job>,
}

mod jobkey {
    use super::Serialize;

    #[derive(Clone, Debug, Eq, PartialOrd, Ord, PartialEq, Serialize)]
    pub struct JobKey(String);

    impl JobKey {
        pub fn new(s: String) -> Self {
            assert!(
                s.len() < 100,
                "Invalid job key {s:?}: must be less than 100 characters"
            );
            assert!(
                s.chars()
                    .all(|c| c.is_ascii() && (c.is_alphanumeric() || c == '-' || c == '_')),
                "Invalid job key {s:?}: must be alphanumeric except '_' or '-'"
            );
            assert!(
                matches!(
                    s.chars().next(),
                    Some(c) if c.is_alphanumeric() || c == '_'),
                "Invalid job key {s:?}: must start with alphanumeric character or '_'"
            );
            Self(s)
        }
    }
}
use jobkey::JobKey;

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Job {
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    needs: Vec<JobKey>,
    runs_on: Os,
    steps: Vec<Step>,
}

#[derive(Debug, Serialize)]
struct Step {
    name: String,
    #[serde(flatten)]
    details: StepDetails,
    #[serde(skip_serializing_if = "Dict::is_empty")]
    env: Dict,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum StepDetails {
    Uses {
        uses: Action,
        #[serde(skip_serializing_if = "Dict::is_empty")]
        with: Dict,
    },
    Run {
        run: String,
    },
}

use StepDetails::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Os {
    Ubuntu,
    Mac,
}

impl Os {
    fn as_str(&self) -> &'static str {
        match self {
            Os::Ubuntu => "ubuntu-20.04",
            Os::Mac => "macos-12",
        }
    }

    fn as_ident(&self) -> &'static str {
        match self {
            Os::Ubuntu => "ubuntu-20-04",
            Os::Mac => "macos-12",
        }
    }
}

impl Serialize for Os {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Checkout,
    Cache,
    Cargo,
    DownloadArtifact,
    RustToolchain,
    UploadArtifact,
}

impl Action {
    fn as_str(&self) -> &'static str {
        use Action::*;
        match self {
            Checkout => "actions/checkout@v2",
            Cache => "actions/cache@v3",
            Cargo => "actions-rs/cargo@v1",
            DownloadArtifact => "actions/download-artifact@v3",
            RustToolchain => "actions-rs/toolchain@v1",
            UploadArtifact => "actions/upload-artifact@v3",
        }
    }
}

impl Serialize for Action {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Rust {
    Stable,
    Nightly,
}

impl Display for Rust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rust::Stable => "stable",
            Rust::Nightly => "nightly",
        }
        .fmt(f)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Runner {
    Bubblewrap,
    DockerBind,
    Docker,
    User,
}

impl Display for Runner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Runner::Bubblewrap => "bubblewrap",
            Runner::Docker => "docker",
            Runner::DockerBind => "docker-bind",
            Runner::User => "user",
        }
        .fmt(f)
    }
}

// The initial workflow YAML skeleton was based on
// https://github.com/actions-rs/example/blob/master/.github/workflows/quickstart.yml
// and
// https://github.com/ramosbugs/oauth2-rs/blob/main/.github/workflows/main.yml.
fn ci_workflow() -> Workflow {
    Workflow {
        name: s("CI"),
        on: ymap! {
            "push": {
                "branches": ["main"],
            },
            "pull_request": {},
            // Run daily to catch breakages in new Rust versions as well as new
            // cargo audit findings.
            "schedule": [
                {"cron": "0 16 * * *"},
            ],
            // Allows you to run this workflow manually from the Actions tab.
            "workflow_dispatch": {},
        },
        jobs: ci_jobs(),
    }
}

fn ci_jobs() -> BTreeMap<JobKey, Job> {
    let mut jobs = BTreeMap::new();

    let ubuntu_stable_key = {
        let (key, job) = build_job(Os::Ubuntu, Rust::Stable, RunOnceChecks(true));
        jobs.insert(key.clone(), job);
        key
    };

    jobs.extend([build_job(Os::Ubuntu, Rust::Nightly, RunOnceChecks(false))]);

    let mac_stable_key = {
        let (key, job) = build_job(Os::Mac, Rust::Stable, RunOnceChecks(false));
        jobs.insert(key.clone(), job);
        key
    };

    jobs.extend([
        system_test_job(
            Os::Ubuntu,
            Runner::Bubblewrap,
            vec![ubuntu_stable_key.clone()],
        ),
        system_test_job(Os::Ubuntu, Runner::Docker, vec![ubuntu_stable_key.clone()]),
        system_test_job(
            Os::Ubuntu,
            Runner::DockerBind,
            vec![ubuntu_stable_key.clone()],
        ),
        system_test_job(Os::Ubuntu, Runner::User, vec![ubuntu_stable_key]),
        system_test_job(Os::Mac, Runner::Docker, vec![mac_stable_key]),
    ]);

    jobs
}

struct RunOnceChecks(bool);

fn build_job(os: Os, rust: Rust, run_once_checks: RunOnceChecks) -> (JobKey, Job) {
    let mut steps = vec![];

    steps.push(Step {
        name: s("Check out sources"),
        details: Uses {
            uses: Action::Checkout,
            with: dict! {},
        },
        env: dict! {},
    });

    steps.push(Step {
        name: format!("Install Rust {rust} toolchain"),
        details: Uses {
            uses: Action::RustToolchain,
            with: dict! {
                "profile" => "minimal",
                "toolchain" => rust,
                "override" => true,
                "components" => "rustfmt, clippy",
            },
        },
        env: dict! {},
    });

    steps.push(Step {
        // See https://github.com/actions/cache/blob/main/examples.md#rust---cargo
        name: s("Use Rust/Cargo cache"),
        details: Uses {
            uses: Action::Cache,
            with: dict! {
                "path" => indoc! {"
                    ~/.cargo/registry
                    ~/.cargo/git/
                    target/
                "},
                "key" => format!("cargo-{os}-{rust}-${{{{ hashFiles('Cargo.lock') }}}}"),
                "restore-keys" => format!("cargo-{os}-{rust}-"),
            },
        },
        env: dict! {},
    });

    steps.push(Step {
        name: s("Run cargo build"),
        details: Uses {
            uses: Action::Cargo,
            with: dict! { "command" => "build" },
        },
        env: dict! {},
    });

    steps.push(Step {
        name: s("Run cargo test"),
        details: Uses {
            uses: Action::Cargo,
            with: dict! { "command" => "test" },
        },
        env: dict! {},
    });

    // Some checks like `cargo fmt` only need to run once, preferably on the
    // stable toolchain.
    if run_once_checks.0 {
        steps.push(Step {
            name: s("Run cargo fmt"),
            details: Uses {
                uses: Action::Cargo,
                with: dict! {
                    "command" => "fmt",
                    "args" => "--all -- --check",
                },
            },
            env: dict! {},
        });

        steps.push(Step {
            name: s("Run clippy"),
            details: Uses {
                uses: Action::Cargo,
                with: dict! {
                    "command" => "clippy",
                    "args" => "-- -D warnings",
                },
            },
            env: dict! {},
        });

        steps.push(Step {
            name: s("Check GitHub YAML"),
            details: Run {
                run: s(indoc! {"
                    cargo run --bin gen_github_yaml > .github/workflows/main.gen.yaml
                    diff .github/workflows/main.yaml .github/workflows/main.gen.yaml
                "}),
            },
            env: dict! {},
        });

        steps.push(Step {
            name: s("Install cargo audit"),
            details: Uses {
                uses: Action::Cargo,
                with: dict! {
                    "command" => "install",
                    "args" => "cargo-audit",
                },
            },
            env: dict! {},
        });

        steps.push(Step {
            name: s("Run cargo audit"),
            details: Uses {
                uses: Action::Cargo,
                with: dict! { "command" => "audit" },
            },
            env: dict! {},
        });
    }

    // The uploader currently uses gzip internally and will aggressively and
    // slowly gzip anything it doesn't think is already compressed. It has an
    // exception for a very small number of file extensions, including `.gz`.
    // This uses `gzip -1` first, which saves a second or two. We can switch to
    // Zstd (which would save another couple of seconds) once this PR is
    // merged: <https://github.com/actions/toolkit/pull/1118>.
    steps.push(Step {
        name: s("Save build artifact"),
        details: Run {
            run: s(indoc! {"
                tar -C .. --create \\
                    cubicle/dev-init.sh \\
                    cubicle/packages/ \\
                    cubicle/src/bin/system_test/github/ \\
                    cubicle/target/debug/cubicle \\
                    cubicle/target/debug/system_test | \\
                gzip -1 > debug-dist.tar.gz
            "}),
        },
        env: dict! {},
    });

    steps.push(Step {
        name: s("Upload build artifact"),
        details: Uses {
            uses: Action::UploadArtifact,
            with: dict! {
                "name" => format!("debug-dist-{os}-{rust}"),
                "path" => "debug-dist.tar.gz",
                "if-no-files-found" => "error",
            },
        },
        env: dict! {},
    });

    let key = JobKey::new(format!("build-{}-{}", os.as_ident(), rust));

    let job = Job {
        name: format!("Build & check ({os}, Rust {rust})"),
        needs: vec![],
        runs_on: os,
        steps,
    };

    (key, job)
}

fn system_test_job(os: Os, runner: Runner, needs: Vec<JobKey>) -> (JobKey, Job) {
    let rust = Rust::Stable;
    let mut steps = Vec::new();

    match runner {
        Runner::Bubblewrap => {
            assert!(os == Os::Ubuntu);
            steps.push(Step {
                name: s("Install Bubblewrap and minor dependencies"),
                details: Run {
                    run: s("sudo apt-get install -y bubblewrap pv"),
                },
                env: dict! {},
            });
        }

        Runner::Docker | Runner::DockerBind => {
            if os == Os::Mac {
                steps.extend(docker_mac_install_steps());
            }

            steps.push(Step {
                name: s("Docker hello world"),
                details: Run {
                    run: s("docker run --rm debian:11 echo 'Hello world'"),
                },
                env: dict! {},
            });
        }

        Runner::User => {
            assert!(os == Os::Ubuntu); // for now
            steps.push(Step {
                name: s("Install minor dependencies"),
                details: Run {
                    run: s("sudo apt-get install -y pv"),
                },
                env: dict! {},
            });
        }
    }

    steps.push(Step {
        name: s("Download build artifact"),
        details: Uses {
            uses: Action::DownloadArtifact,
            with: dict! { "name" => format!("debug-dist-{os}-{rust}") },
        },
        env: dict! {},
    });

    steps.push(Step {
        name: s("Unpack build artifact"),
        details: Run {
            run: s("tar --directory .. --extract --verbose --file debug-dist.tar.gz"),
        },
        env: dict! {},
    });

    let config = format!("src/bin/system_test/github/{runner}.toml");
    steps.push(Step {
        name: s("Run cub list"),
        details: Run {
            run: format!("./target/debug/cubicle --config '{config}' list"),
        },
        env: dict! {"RUST_BACKTRACE" => "1"},
    });

    let config = format!("src/bin/system_test/github/{runner}.toml");
    steps.push(Step {
        name: s("Run system test"),
        details: Run {
            run: format!("./target/debug/system_test --config '{config}'"),
        },
        env: dict! {
            "INSTA_WORKSPACE_ROOT" => ".",
            "RUST_BACKTRACE" => "1",
        },
    });

    let key = JobKey::new(format!("system_test-{}-{}", os.as_ident(), runner));

    let job = Job {
        name: format!("System tests ({os}, {runner})"),
        needs,
        runs_on: os,
        steps,
    };

    (key, job)
}

// Docker isn't installed on the Mac runners due to licensing issues:
// see <https://github.com/actions/runner-images/issues/2150>.
fn docker_mac_install_steps() -> Vec<Step> {
    vec![
        Step {
            name: s("Install Docker"),
            details: Run {
                run: s("brew install docker docker-machine"),
            },
            env: dict! {},
        },
        Step {
            name: s("Create VirtualBox VM for Docker"),
            // Normally docker-machine would check for the latest boot2docker
            // URL, but it gives the error: `Error with pre-create check:
            // "failure getting a version tag from the Github API response (are
            // you getting rate limited by Github?)"`. Its README says to pass
            // `--github-api-token`, but that doesn't work due to
            // <https://github.com/docker/machine/issues/2765> and
            // <https://github.com/docker/machine/issues/2296>. This just sets
            // a static URL. Because boot2docker is no longer maintained,
            // there's no risk of a newer ISO being released.

            // Then, set a different IPv4 range to work around this error:
            //
            // ```
            // Error creating machine: Error in driver during machine creation:
            //     Error setting up host only network on machine start:
            //     /usr/local/bin/VBoxManage hostonlyif ipconfig vboxnet0 --ip
            //     192.168.99.1 --netmask 255.255.255.0 failed:
            // VBoxManage: error: Code E_ACCESSDENIED (0x80070005) - Access denied
            //     (extended info not available)
            // VBoxManage: error: Context: "EnableStaticIPConfig(Bstr(pszIp).raw(),
            //     Bstr(pszNetmask).raw())" at line 242 of file VBoxManageHostonly.cpp
            // ```
            //
            // See <https://github.com/nektos/act/issues/858>.
            details: Run {
                run: s(indoc! {r#"
                    docker-machine create \
                        --driver virtualbox \
                        --virtualbox-boot2docker-url 'https://github.com/boot2docker/boot2docker/releases/download/v19.03.12/boot2docker.iso' \
                        --virtualbox-hostonly-cidr '192.168.56.1/24' \
                        default && \
                    eval "$(docker-machine env default)" && \
                    env | grep DOCKER >> $GITHUB_ENV
                "#}),
            },
            env: dict! {},
        },
    ]
}

fn write<W: Write>(mut w: W, workflow: &Workflow) -> anyhow::Result<()> {
    writeln!(
        w,
        "# This file is automatically generated from {:?}.",
        file!()
    )?;
    writeln!(w, "# Do not modify it directly.")?;
    writeln!(w)?;
    serde_yaml::to_writer(w, workflow)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let ci = ci_workflow();
    write(&std::io::stdout(), &ci)
}
