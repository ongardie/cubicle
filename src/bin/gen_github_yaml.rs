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

use indoc::{formatdoc, indoc};
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
        json::Object(value) => yaml::Mapping(
            value
                .into_iter()
                .map(|(k, v)| (yaml::String(k), yaml_value_from_json(v)))
                .collect::<serde_yaml::Mapping>(),
        ),
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
    // Both Mac versions are slow to run Docker. Despite having to install
    // Colima, Mac OS 13 is more current and not much slower (relatively), so
    // prefer it.
    #[allow(unused)]
    Mac12,
    Mac13,
}

impl Os {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Ubuntu => "ubuntu-22.04",
            Self::Mac12 => "macos-12",
            Self::Mac13 => "macos-13",
        }
    }

    fn as_ident(&self) -> &'static str {
        match self {
            Self::Ubuntu => "ubuntu-22-04",
            Self::Mac12 => "macos-12",
            Self::Mac13 => "macos-13",
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
#[allow(unused)]
enum Action {
    Checkout,
    Cache,
    CacheRestore,
    CacheSave,
    DownloadArtifact,
    UploadArtifact,
}

impl Action {
    fn as_str(&self) -> &'static str {
        use Action::*;
        match self {
            Checkout => "actions/checkout@v4",
            Cache => "actions/cache@v4",
            CacheRestore => "actions/cache/restore@v4",
            CacheSave => "actions/cache/save@v4",
            DownloadArtifact => "actions/download-artifact@v4",
            UploadArtifact => "actions/upload-artifact@v4",
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
            Self::Stable => "stable",
            Self::Nightly => "nightly",
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
            Self::Bubblewrap => "bubblewrap",
            Self::Docker => "docker",
            Self::DockerBind => "docker-bind",
            Self::User => "user",
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
        let (key, job) = build_job(Os::Mac13, Rust::Stable, RunOnceChecks(false));
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
        system_test_job(Os::Mac13, Runner::Docker, vec![mac_stable_key]),
    ]);

    jobs
}

#[derive(Clone, Copy)]
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
        details: Run {
            run: formatdoc! {"
                rustup toolchain install {rust} --profile minimal --component clippy,rustfmt
                rustup default {rust}
            "},
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
        details: Run {
            run: s("cargo build"),
        },
        env: dict! {},
    });

    steps.push(Step {
        name: s("Run cargo test"),
        details: Run {
            run: s("cargo test"),
        },
        env: dict! { "RUST_BACKTRACE" => "1" },
    });

    // Some checks like `cargo fmt` only need to run once, preferably on the
    // stable toolchain.
    if run_once_checks.0 {
        steps.push(Step {
            name: s("Run cargo fmt"),
            details: Run {
                run: s("cargo fmt --all -- --check"),
            },
            env: dict! {},
        });

        steps.push(Step {
            name: s("Run clippy"),
            details: Run {
                run: s("cargo clippy -- -D warnings"),
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
            name: s("Run shellcheck"),
            details: Run {
                run: formatdoc! {r#"
                    curl -LO https://github.com/koalaman/shellcheck/releases/download/{version}/shellcheck-{version}.linux.x86_64.tar.xz
                    tar -xvf shellcheck-{version}.linux.x86_64.tar.xz
                    PATH="$PWD/shellcheck-{version}:$PATH" ./shellcheck.sh
                "#, version = "v0.9.0"},
            },
            env: dict! {},
        });

        steps.push(Step {
            name: s("Install cargo audit"),
            details: Run {
                run: s("cargo install cargo-audit"),
            },
            env: dict! {},
        });

        steps.push(Step {
            name: s("Run cargo audit"),
            details: Run {
                run: s("cargo audit"),
            },
            env: dict! {},
        });
    }

    steps.push(Step {
        name: s("Save build artifact"),
        details: Run {
            run: s(indoc! {"
                tar -C .. --create --file debug-dist.tar.zst \\
                    cubicle/packages/ \\
                    cubicle/src/bin/system_test/github/ \\
                    cubicle/target/debug/cub \\
                    cubicle/target/debug/system_test
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
                "path" => "debug-dist.tar.zst",
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
            steps.extend(install_docker(os));

            steps.push(Step {
                name: s("Docker hello world"),
                details: Run {
                    run: s("docker run --rm debian:12 echo 'Hello world'"),
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
            run: s("tar --directory .. --extract --verbose --file debug-dist.tar.zst"),
        },
        env: dict! {},
    });

    let config = format!("src/bin/system_test/github/{runner}.toml");
    steps.push(Step {
        name: s("Run cub list"),
        details: Run {
            run: format!("./target/debug/cub --config '{config}' list"),
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

fn install_docker(os: Os) -> Vec<Step> {
    // Docker isn't installed on the Mac runners due to licensing issues:
    // see <https://github.com/actions/runner-images/issues/2150>.
    match os {
        // Using Colima works for Mac OS 12 and 13, but it takes about 7
        // minutes to get through the Docker "Hello world".
        //
        // It doesn't seem like we can continue using the prior
        // docker-machine/boot2docker/Virtualbox approach:
        // - Virtualbox isn't installed and doesn't seem to work (with `brew
        //   install virtualbox`) on the Mac OS 13 runners.
        // - boot2docker is deprecated.
        // - docker-machine is deprecated.
        // - Even on Mac OS 12, there were odd gpg issues with debian:12 when
        //   running apt-update (that don't happen with Colima or elsewhere).
        Os::Mac12 | Os::Mac13 => {
            let install_docker = Step {
                name: s("Install Docker"),
                details: Run {
                    run: s("brew install docker"),
                },
                env: dict! {},
            };

            let start_colima = Step {
                name: s("Start Colima"),
                details: Run {
                    // Unfortunately, VZ seems to hang forever as of 2024-01-31.
                    //
                    // The Lima docs say that the VZ backend is faster and the
                    // example uses virtiofs. VZ is only available starting in Mac
                    // OS 13. See <https://lima-vm.io/docs/config/vmtype/>.
                    //
                    // The default disk size is 60 GiB, and the VZ backend appears
                    // to unpack and "expand" the QCOW2 image. The runner only has
                    // 14 GB and is slow to write to disk, so set the disk size
                    // smaller. The QCOW2 image expands to a 3.5 GiB raw disk, and
                    // the `--disk` flag takes an int, so the smallest reasonable
                    // size is probably 4.
                    //
                    // run: s("colima start --disk 4 --mount-type virtiofs --vm-type vz"),
                    run: s("colima start"), // using QEMU by default, not VZ
                },
                env: dict! {},
            };

            match os {
                Os::Mac12 => vec![install_docker, start_colima],

                // Colima isn't installed in the Mac OS 13 runners.
                Os::Mac13 => vec![
                    install_docker,
                    Step {
                        name: s("Install Colima"),
                        details: Run {
                            // Currently, installing colima installs both
                            // Python 3.11 and 3.12. These conflict with
                            // existing `/usr/local/bin/2to3-3.11` and
                            // `/usr/local/bin/2to3`. To work around this,
                            // install Python explicitly with the `--overwrite`
                            // flag.
                            run: s(indoc! {"
                                brew install --overwrite python@3.11
                                brew install --overwrite python@3.12
                                brew install colima
                            "}),
                        },
                        env: dict! {},
                    },
                    start_colima,
                ],

                Os::Ubuntu => unreachable!(),
            }
        }

        Os::Ubuntu => vec![/* works out of the box */],
    }
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
