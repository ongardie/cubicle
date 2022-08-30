//! Command-line parsing and processing for main Cubicle executable.
//!
//! Note: the documentation for [`Args`] and related types is used to generate
//! the usage for the command-line program and should be read from that
//! perspective.

use clap::{Parser, Subcommand};
use clap_complete::{generate, shells::Shell};
use std::collections::BTreeSet;
use std::fmt::Display;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use cubicle::hidden::host_home_dir;
use cubicle::somehow::{Context, Error, Result};
use cubicle::{
    Clean, Cubicle, EnvironmentName, ListFormat, ListPackagesFormat, PackageName, PackageNameSet,
    Quiet, ShouldPackageUpdate, UpdatePackagesConditions,
};

/// Manage sandboxed development environments.
#[derive(Debug, Parser)]
// clap shows only brief help messages (the top line of comments) with `-h` and
// longer messages with `--help`. This custom help message gives people some
// hope of learning that distinction. See
// <https://github.com/clap-rs/clap/issues/1015>.
#[clap(help_message("Print help information. Use --help for more details"))]
pub struct Args {
    /// Path to configuration file.
    #[clap(
        short,
        long,
        default_value_t = default_config_path(),
        value_hint(clap::ValueHint::FilePath),
    )]
    config: PathWithVarExpansion,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate tab-completions for your shell.
    ///
    /// Installation for Bash:
    ///
    ///   $ cub completions bash > ~/.local/share/bash-completion/completions/cub
    ///
    /// Installation for ZSH (depending on `$fpath`):
    ///
    ///   $ cub completions zsh > ~/.zfunc/_cub
    ///
    /// You may need to restart your shell or configure it.
    ///
    /// This installation works similarly as for rustup's completions. For
    /// detailed instructions, see:
    ///
    ///   $ rustup help completions
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

    /// View and manage packages.
    #[clap(subcommand)]
    Package(PackageCommands),

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

#[derive(Debug, Subcommand)]
enum PackageCommands {
    /// Show available packages.
    List {
        /// Set output format.
        #[clap(long, value_enum, default_value_t)]
        format: ListPackagesFormat,
    },

    /// (Re-)build one or more packages.
    #[clap(arg_required_else_help(true))]
    Update {
        /// Clear out existing build environment first.
        ///
        /// This flag only applies to the named PACKAGES, not their
        /// dependencies.
        #[clap(long)]
        clean: bool,
        /// Build dependencies only if required
        ///
        /// By default, this command will re-build dependencies if they are
        /// stale. With this flag, it will only build dependencies if they are
        /// strictly needed because have never been built successfully before.
        #[clap(long)]
        skip_deps: bool,
        /// Package name(s).
        packages: Vec<String>,
    },
}

/// Parses the command-line arguments given to this executable.
///
/// Exits the process upon errors or upon successfully handling certain flags
/// like `--help`.
pub fn parse() -> Args {
    Args::parse()
}

impl Args {
    /// Returns the path on the host's filesystem to the Cubicle configuration
    /// file (normally named `cubicle.toml`).
    pub fn config_path(&self) -> &Path {
        self.config.as_ref()
    }
}

/// This type wrapper stores a normal path but understands "$HOME".
///
/// In particular, it expands the variable "$HOME" when converting from a
/// string and displays the path with "$HOME" when possible.
///
/// The main reason for this is to get the user's home directory out of the
/// usage message and therefore out of the unit test snapshots.
///
/// # Historical Note
///
/// Previous to this, I (Diego) attempted to redact the home directory path
/// from the usage message in the unit test before snapshotting. This didn't
/// work because a difference in length of the path can cause the line to wrap
/// for some users and not others.
///
/// After that, I tried to only substitute in "$HOME" during Display but never
/// expand "$HOME". This didn't work either because clap always converts the
/// default value to a string, then parses that string.
#[derive(Debug)]
struct PathWithVarExpansion(PathBuf);

impl PathWithVarExpansion {
    /// Helper for Display. Split out for unit testing.
    fn sub_home_prefix(&self, home: &Path) -> String {
        if let Ok(rest) = self.0.strip_prefix(&home) {
            format!("$HOME{}{}", std::path::MAIN_SEPARATOR, rest.display())
        } else {
            format!("{}", self.0.display())
        }
    }

    /// Helper for `from_str`. Split out for unit testing.
    fn expand_home_prefix(path_str: &str, home: &Path) -> Self {
        let path = if path_str == "$HOME" {
            home.to_owned()
        } else if let Some(rest) =
            path_str.strip_prefix(&format!("$HOME{}", std::path::MAIN_SEPARATOR))
        {
            home.join(rest)
        } else {
            PathBuf::from(path_str)
        };
        Self(path)
    }
}

impl AsRef<Path> for PathWithVarExpansion {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Display for PathWithVarExpansion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.sub_home_prefix(host_home_dir()).fmt(f)
    }
}

impl FromStr for PathWithVarExpansion {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(Self::expand_home_prefix(s, host_home_dir()))
    }
}

fn default_config_path() -> PathWithVarExpansion {
    let xdg_config_home = if let Ok(path) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(path)
    } else {
        host_home_dir().join(".config")
    };
    PathWithVarExpansion(xdg_config_home.join("cubicle.toml"))
}

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

fn write_completions<W: io::Write>(shell: Shell, out: &mut W) -> Result<()> {
    use clap::CommandFactory;
    let cmd = &mut Args::command();

    // We can't list out environment names and package names statically.
    // Unfortunately, there seems to be no general way to tell `clap` about
    // these dynamic lists. For ZSH, we hack calls to this program into the
    // generated output. (Similar contributions would be welcome for Bash).
    if shell == Shell::Zsh {
        let mut buf: Vec<u8> = Vec::new();
        generate(shell, cmd, "cub", &mut buf);
        let buf = String::from_utf8(buf).context("error reading clap shell completion output")?;
        let mut counts = [0; 5];
        let mut write = || -> std::io::Result<()> {
            for line in buf.lines() {
                match line {
                    r#"':name -- Environment name:' \"# => {
                        counts[0] += 1;
                        writeln!(out, r#"':name -- Environment name:_cub_envs' \"#)?;
                    }
                    r#"'*::names -- Environment name(s):' \"# => {
                        counts[1] += 1;
                        writeln!(out, r#"'*::names -- Environment name(s):_cub_envs' \"#)?;
                    }
                    r#"'*::packages -- Package name(s):' \"# => {
                        counts[2] += 1;
                        writeln!(out, r#"'*::packages -- Package name(s):_cub_pkgs' \"#)?;
                    }
                    r#"'*--packages=[Comma-separated names of packages to inject into home directory]:PACKAGES: ' \"# =>
                    {
                        counts[3] += 1;
                        writeln!(
                            out,
                            r#"'*--packages=[Comma-separated names of packages to inject into home directory]:PACKAGES:_cub_pkgs_comma' \"#
                        )?;
                    }
                    r#"_cub "$@""# => {
                        counts[4] += 1;
                        writeln!(
                            out,
                            "{}",
                            r#"
_cub_envs() {
    _values -w 'environments' $(cub list --format=names)
}
_cub_pkgs() {
    _values -w 'packages' $(cub package list --format=names)
}
_cub_pkgs_comma() {
    _values -s , -w 'packages' $(cub package list --format=names)
}
"#
                        )?;
                        writeln!(out, "{}", line)?;
                    }
                    _ => {
                        writeln!(out, "{}", line)?;
                    }
                }
            }
            Ok(())
        };
        write().context("failed to write zsh completions")?;
        debug_assert_eq!(
            counts,
            [2, 2, 1, 3, 1],
            "zsh completions not patched as expected"
        );
    } else {
        generate(shell, cmd, "cub", out);
    }
    Ok(())
}

/// Execute the subcommand requested on the command line.
pub fn run(args: Args, program: &Cubicle) -> Result<()> {
    use Commands::*;
    match args.command {
        Completions { shell } => write_completions(shell, &mut io::stdout()),
        Enter { name } => program.enter_environment(&name),
        Exec { name, command } => program.exec_environment(&name, &command),
        List { format } => program.list_environments(format),
        New {
            name,
            enter,
            packages,
        } => {
            let packages = packages.map(package_set_from_names).transpose()?;
            program.new_environment(&name, packages.as_ref())?;
            if enter {
                program.enter_environment(&name)?;
            }
            Ok(())
        }
        Package(command) => run_package_command(command, program),
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
                program.reset_environment(name, packages.as_ref(), Clean(clean))?;
            }
            Ok(())
        }
        Tmp { packages } => {
            let packages = packages.map(package_set_from_names).transpose()?;
            program.create_enter_tmp_environment(packages.as_ref())
        }
    }
}

fn run_package_command(command: PackageCommands, program: &Cubicle) -> Result<()> {
    use PackageCommands::*;
    match command {
        List { format } => program.list_packages(format),

        Update {
            clean,
            skip_deps,
            packages,
        } => {
            use ShouldPackageUpdate::*;
            let packages = package_set_from_names(packages)?;
            if clean {
                for package in &packages {
                    program.purge_environment(
                        &EnvironmentName::for_builder_package(package),
                        Quiet(true),
                    )?
                }
            }
            let specs = program.scan_packages()?;
            program.update_packages(
                &packages,
                &specs,
                UpdatePackagesConditions {
                    dependencies: if skip_deps { IfRequired } else { IfStale },
                    named: Always,
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_display_snapshot, assert_snapshot};

    #[test]
    fn sub_home_prefix() {
        let p = PathWithVarExpansion(PathBuf::from("/home/foo/bar"));
        assert_eq!("$HOME/bar", p.sub_home_prefix(Path::new("/home/foo")));
        assert_eq!("$HOME/bar", p.sub_home_prefix(Path::new("/home/foo/")));
        assert_eq!("/home/foo/bar", p.sub_home_prefix(Path::new("/home/fo")));
    }

    #[test]
    fn expand_home_prefix() {
        assert_eq!(
            "/home/foo/bar",
            PathWithVarExpansion::expand_home_prefix("$HOME/bar", Path::new("/home/foo"))
                .to_string()
        );
        assert_eq!(
            "/home/foo/bar",
            PathWithVarExpansion::expand_home_prefix("$HOME/bar", Path::new("/home/foo/"))
                .to_string()
        );
        assert_eq!(
            "/home/foo",
            PathWithVarExpansion::expand_home_prefix("$HOME", Path::new("/home/foo")).to_string()
        );
        assert_eq!(
            "$HOMER",
            PathWithVarExpansion::expand_home_prefix("$HOMER", Path::new("/home/foo")).to_string()
        );
        assert_eq!(
            "/abc/$HOME",
            PathWithVarExpansion::expand_home_prefix("/abc/$HOME", Path::new("/home/foo"))
                .to_string()
        );
        assert_eq!(
            "/abc/def",
            PathWithVarExpansion::expand_home_prefix("/abc/def", Path::new("/home/foo"))
                .to_string()
        );
    }

    #[test]
    fn usage() {
        for cmd in [
            "",
            "completions",
            "enter",
            "exec",
            "list",
            "new",
            "package",
            "package list",
            "package update",
            "purge",
            "reset",
            "tmp",
        ] {
            let split_cmd = shlex::split(&format!("cub {cmd} --help")).unwrap();
            let err = Args::try_parse_from(split_cmd).unwrap_err();
            let name = format!("usage_{}", if cmd.is_empty() { "cub" } else { cmd });
            assert_display_snapshot!(name, err);
        }
    }

    #[test]
    fn write_completions() {
        for shell in [Shell::Bash, Shell::Zsh] {
            let mut buf: Vec<u8> = Vec::new();
            super::write_completions(shell, &mut buf).unwrap();
            let buf = String::from_utf8(buf).unwrap();
            assert_snapshot!(format!("write_completions_{shell}"), buf);
        }
    }
}
