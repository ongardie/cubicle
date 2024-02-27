//! Command-line parsing and processing for main Cubicle executable.
//!
//! Note: the documentation for [`Args`] and related types is used to generate
//! the usage for the command-line program and should be read from that
//! perspective.

use clap::{Parser, Subcommand};
use clap_complete::{generate, shells::Shell};
use std::collections::BTreeSet;
use std::fmt::{self, Debug, Display};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use wildmatch::WildMatch;

use cubicle::hidden::host_home_dir;
use cubicle::somehow::{somehow as anyhow, warn, Context, Error, Result};
use cubicle::{
    Cubicle, EnvironmentName, FullPackageName, ListFormat, ListPackagesFormat, Quiet,
    ShouldPackageUpdate, UpdatePackagesConditions,
};

/// Manage sandboxed development environments.
#[derive(Debug, Parser)]
pub struct Args {
    /// Path to configuration file.
    #[arg(
        short,
        long,
        default_value_t = default_config_path(),
        value_hint(clap::ValueHint::FilePath),
    )]
    config: PathWithVarExpansion,

    #[command(subcommand)]
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
    #[command(arg_required_else_help(true))]
    Completions { shell: Shell },

    /// Run a shell in an existing environment.
    #[command(arg_required_else_help(true))]
    Enter {
        /// Environment name.
        ///
        /// Wildcards are allowed: `?` matches a single character and `*`
        /// matches zero or more characters.
        name: EnvironmentPattern,
    },

    /// Run a command in an existing environment.
    #[command(arg_required_else_help(true))]
    Exec {
        /// Environment name.
        ///
        /// Wildcards are allowed: `?` matches a single character and `*`
        /// matches zero or more characters.
        name: EnvironmentPattern,
        /// Command and arguments to run.
        #[arg(last = true, required(true))]
        command: Vec<String>,
    },

    /// Show existing environments.
    List {
        /// Set output format.
        #[arg(long, value_enum, default_value_t)]
        format: ListFormat,
    },

    /// View and manage packages.
    #[command(subcommand)]
    Package(PackageCommands),

    /// Create a new environment.
    #[command(arg_required_else_help(true))]
    New {
        /// Run a shell in new environment.
        #[arg(long)]
        enter: bool,
        /// Comma-separated names of packages to inject into home directory.
        ///
        /// If omitted, uses the "default" package.
        ///
        /// Wildcards are allowed: `?` matches a single character and `*`
        /// matches zero or more characters.
        #[arg(long, value_delimiter = ',')]
        packages: Option<Vec<String>>,
        /// New environment name.
        name: EnvironmentName,
    },

    /// Delete environment(s) and their work directories.
    #[command(arg_required_else_help(true))]
    Purge {
        /// Environment name(s).
        ///
        /// Wildcards are allowed: `?` matches a single character and `*`
        /// matches zero or more characters.
        #[arg(required(true))]
        names: Vec<EnvironmentPattern>,
    },

    /// Recreate an environment (keeping only its work directory).
    #[command(arg_required_else_help(true))]
    Reset {
        /// Comma-separated names of packages to inject into home directory.
        ///
        /// If omitted, uses the packages from the `package.txt` file in the
        /// environment's work directory. This is automatically written when
        /// the environment is created or reset.
        ///
        /// Wildcards are allowed: `?` matches a single character and `*`
        /// matches zero or more characters.
        #[arg(long, value_delimiter = ',')]
        packages: Option<Vec<String>>,
        /// Environment name(s).
        ///
        /// Wildcards are allowed: `?` matches a single character and `*`
        /// matches zero or more characters.
        #[arg(required(true))]
        names: Vec<EnvironmentPattern>,
    },

    /// Create and enter a new temporary environment.
    Tmp {
        /// Comma-separated names of packages to inject into home directory.
        ///
        /// If omitted, uses the "default" package.
        ///
        /// Wildcards are allowed: `?` matches a single character and `*`
        /// matches zero or more characters.
        #[arg(long, value_delimiter = ',')]
        packages: Option<Vec<String>>,
    },
}

/// View and manage packages.
#[derive(Debug, Subcommand)]
enum PackageCommands {
    /// Show available packages.
    List {
        /// Set output format.
        #[arg(long, value_enum, default_value_t)]
        format: ListPackagesFormat,
    },

    /// (Re-)build one or more packages.
    #[command(arg_required_else_help(true))]
    Update {
        /// Clear out existing build environment first.
        ///
        /// This flag only applies to the named PACKAGES, not their
        /// dependencies.
        #[arg(long)]
        clean: bool,
        /// Build dependencies only if required.
        ///
        /// By default, this command will re-build dependencies if they are
        /// stale. With this flag, it will only build dependencies if they are
        /// strictly needed because have never been built successfully before.
        #[arg(long)]
        skip_deps: bool,
        /// Package name(s).
        ///
        /// Wildcards are allowed: `?` matches a single character and `*`
        /// matches zero or more characters.
        #[arg(required(true))]
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
#[derive(Clone, Debug)]
struct PathWithVarExpansion(PathBuf);

impl PathWithVarExpansion {
    /// Helper for Display. Split out for unit testing.
    fn sub_home_prefix(&self, home: &Path) -> String {
        if let Ok(rest) = self.0.strip_prefix(home) {
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
        Display::fmt(&self.sub_home_prefix(host_home_dir()), f)
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

fn package_set_from_patterns(
    patterns: &[String],
    names: BTreeSet<FullPackageName>,
) -> Result<BTreeSet<FullPackageName>> {
    let names: Vec<(String, FullPackageName)> = names
        .into_iter()
        .map(|name| (name.unquoted(), name))
        .collect();
    let mut matched = BTreeSet::new();
    for pattern_str in patterns {
        let pattern_str = pattern_str.trim();
        if pattern_str.is_empty() {
            continue;
        }
        let pattern = GlobPattern::new(pattern_str.to_owned());
        if pattern.is_pattern() {
            let mut matched_some = false;
            for (unquoted, name) in &names {
                if pattern.matches(unquoted) {
                    matched.insert(name.clone());
                    matched_some = true;
                }
            }
            if !matched_some {
                warn(anyhow!(
                    "pattern {pattern_str:?} did not match any package names"
                ));
            }
        } else {
            matched.insert(FullPackageName::from_str(pattern_str)?);
        }
    }
    Ok(matched)
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
                    r#"if [ "$funcstack[1]" = "_cub" ]; then"# => {
                        counts[4] += 1;
                        #[allow(clippy::write_literal)]
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
        Enter { name } => {
            program.enter_environment(&name.matching_environment(program.get_environment_names()?)?)
        }
        Exec { name, command } => program.exec_environment(
            &name.matching_environment(program.get_environment_names()?)?,
            &command,
        ),
        List { format } => program.list_environments(format),
        New {
            name,
            enter,
            packages,
        } => {
            let packages = packages
                .map(|packages| package_set_from_patterns(&packages, program.get_package_names()?))
                .transpose()?;
            program.new_environment(&name, packages)?;
            if enter {
                program.enter_environment(&name)?;
            }
            Ok(())
        }
        Package(command) => run_package_command(command, program),
        Purge { names } => {
            for name in matching_environments(&names, program.get_environment_names()?)? {
                program.purge_environment(&name, Quiet(false))?;
            }
            Ok(())
        }
        // TODO: rename
        Reset { names, packages } => {
            let packages = packages
                .map(|packages| package_set_from_patterns(&packages, program.get_package_names()?))
                .transpose()?;
            for name in matching_environments(&names, program.get_environment_names()?)? {
                program.reset_environment(&name, packages.clone())?;
            }
            Ok(())
        }
        Tmp { packages } => {
            let packages = packages
                .map(|packages| package_set_from_patterns(&packages, program.get_package_names()?))
                .transpose()?;
            program.create_enter_tmp_environment(packages)
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
            let packages = package_set_from_patterns(&packages, program.get_package_names()?)?;
            if clean {
                for package in &packages {
                    program.purge_environment(
                        &EnvironmentName::for_builder_package(package),
                        Quiet(true),
                    )?;
                }
            }
            let specs = program.scan_packages()?;
            program.update_packages(
                &packages,
                &specs,
                &UpdatePackagesConditions {
                    dependencies: if skip_deps { IfRequired } else { IfStale },
                    named: Always,
                },
            )
        }
    }
}

#[derive(Clone, Debug)]
struct GlobPattern {
    str: String,
    pattern: Option<WildMatch>,
}

impl GlobPattern {
    fn new(str: String) -> Self {
        let pattern = str.contains(['?', '*']).then(|| WildMatch::new(&str));
        Self { str, pattern }
    }

    fn is_pattern(&self) -> bool {
        self.pattern.is_some()
    }

    fn matches(&self, s: &str) -> bool {
        match &self.pattern {
            Some(pattern) => pattern.matches(s),
            None => self.str == s,
        }
    }
}

#[derive(Clone, Debug)]
struct EnvironmentPattern(GlobPattern);

impl Display for EnvironmentPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.0.str, f)
    }
}

impl FromStr for EnvironmentPattern {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(GlobPattern::new(s.to_owned())))
    }
}

impl EnvironmentPattern {
    fn matches(&self, name: &EnvironmentName) -> bool {
        self.0.matches(name.as_ref())
    }

    fn matching_environment(&self, names: BTreeSet<EnvironmentName>) -> Result<EnvironmentName> {
        let mut names = names.into_iter().filter(|name| self.matches(name));
        match (names.next(), names.next()) {
            (None, _) => {
                if self.0.is_pattern() {
                    Err(anyhow!("pattern {self} matched no environment names"))
                } else {
                    Err(anyhow!("environment {self} not found"))
                }
            }
            (Some(name), None) => Ok(name),
            (Some(_), Some(_)) => Err(anyhow!("pattern {self} matched more than 1 environment")),
        }
    }
}

fn matching_environments(
    patterns: &[EnvironmentPattern],
    names: BTreeSet<EnvironmentName>,
) -> Result<Vec<EnvironmentName>> {
    let mut unmatched = names;
    let mut matched = Vec::new();
    for pattern in patterns {
        let start = matched.len();
        matched.extend(drain_filter(&mut unmatched, |name| pattern.matches(name)));
        if matched.len() == start && !matched.iter().any(|name| pattern.matches(name)) {
            if pattern.0.is_pattern() {
                warn(anyhow!(
                    "pattern {pattern} did not match any environment names"
                ));
            } else {
                return Err(anyhow!("environment {pattern} not found"));
            }
        }
    }
    Ok(matched)
}

// `BTreeSet::drain_filter` isn't stable yet. See <https://github.com/rust-lang/rust/issues/70530>.
fn drain_filter<T, F>(set: &mut BTreeSet<T>, mut pred: F) -> Vec<T>
where
    T: Clone + Ord,
    F: FnMut(&T) -> bool,
{
    let mut matches = Vec::new();
    for x in set.iter() {
        if pred(x) {
            matches.push(x.clone());
        }
    }
    for x in &matches {
        set.remove(x);
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use expect_test::{expect, expect_file};

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
    fn package_set_from_patterns() {
        let names = || {
            BTreeSet::from([
                FullPackageName::from_str("foo").unwrap(),
                FullPackageName::from_str("foobar").unwrap(),
            ])
        };

        expect![[r#"
            Ok(
                {},
            )
        "#]]
        .assert_debug_eq(&super::package_set_from_patterns(&[], names()));

        expect![[r#"
            Ok(
                {
                    FullPackageName(
                        Root,
                        PackageName(
                            "foo",
                        ),
                    ),
                },
            )
        "#]]
        .assert_debug_eq(&super::package_set_from_patterns(
            &[String::from("foo")],
            names(),
        ));

        expect![[r#"
            Ok(
                {
                    FullPackageName(
                        Root,
                        PackageName(
                            "foo",
                        ),
                    ),
                    FullPackageName(
                        Root,
                        PackageName(
                            "foobar",
                        ),
                    ),
                },
            )
        "#]]
        .assert_debug_eq(&super::package_set_from_patterns(
            &[String::from("foo*"), String::from("bar*")],
            names(),
        ));

        expect![[r#"
            Ok(
                {
                    FullPackageName(
                        Root,
                        PackageName(
                            "baz",
                        ),
                    ),
                    FullPackageName(
                        Root,
                        PackageName(
                            "foo",
                        ),
                    ),
                    FullPackageName(
                        Root,
                        PackageName(
                            "foobar",
                        ),
                    ),
                },
            )
        "#]]
        .assert_debug_eq(&super::package_set_from_patterns(
            &[String::from("foo*"), String::from("baz")],
            names(),
        ));
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
            let err = Args::command()
                .term_width(100)
                .try_get_matches_from(split_cmd)
                .unwrap_err();
            let name = format!(
                "snapshots/cub__cli__tests__usage_{}.snap",
                if cmd.is_empty() { "cub" } else { cmd }
            );
            expect_file![name].assert_eq(&err.to_string());
        }
    }

    #[test]
    fn write_completions() {
        for shell in [Shell::Bash, Shell::Zsh] {
            let mut buf: Vec<u8> = Vec::new();
            super::write_completions(shell, &mut buf).unwrap();
            let buf = String::from_utf8(buf).unwrap();
            expect_file![format!(
                "snapshots/cub__cli__tests__write_completions_{shell}.snap"
            )]
            .assert_eq(&buf);
        }
    }

    #[test]
    fn matching_environment() {
        let names = || {
            BTreeSet::<EnvironmentName>::from([
                EnvironmentName::from_str("foo").unwrap(),
                EnvironmentName::from_str("foobar").unwrap(),
            ])
        };
        assert_eq!(
            EnvironmentPattern::from_str("x")
                .unwrap()
                .matching_environment(names())
                .unwrap_err()
                .debug_without_backtrace(),
            "environment \"x\" not found"
        );
        assert_eq!(
            EnvironmentPattern::from_str("bar*")
                .unwrap()
                .matching_environment(names())
                .unwrap_err()
                .debug_without_backtrace(),
            "pattern \"bar*\" matched no environment names"
        );
        assert_eq!(
            EnvironmentPattern::from_str("fo?")
                .unwrap()
                .matching_environment(names())
                .unwrap()
                .to_string(),
            "\"foo\""
        );
        assert_eq!(
            EnvironmentPattern::from_str("foo*")
                .unwrap()
                .matching_environment(names())
                .unwrap_err()
                .debug_without_backtrace(),
            "pattern \"foo*\" matched more than 1 environment"
        );
    }

    #[test]
    fn matching_environments() {
        let names = || {
            BTreeSet::<EnvironmentName>::from([
                EnvironmentName::from_str("foo").unwrap(),
                EnvironmentName::from_str("foobar").unwrap(),
            ])
        };

        expect![[r#"
            Ok(
                [],
            )
        "#]]
        .assert_debug_eq(&super::matching_environments(&[], names()));

        expect![[r#"
            Ok(
                [
                    EnvironmentName(
                        "foo",
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(&super::matching_environments(
            &[EnvironmentPattern::from_str("foo").unwrap()],
            names(),
        ));

        expect![[r#"
            Err(
                "environment \"bar\" not found",
            )
        "#]]
        .assert_debug_eq(&super::matching_environments(
            &[EnvironmentPattern::from_str("bar").unwrap()],
            names(),
        ));

        expect![[r#"
            Ok(
                [
                    EnvironmentName(
                        "foo",
                    ),
                    EnvironmentName(
                        "foobar",
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(&super::matching_environments(
            &[
                EnvironmentPattern::from_str("foo*").unwrap(),
                EnvironmentPattern::from_str("bar*").unwrap(),
                EnvironmentPattern::from_str("*").unwrap(),
            ],
            names(),
        ));

        assert_eq!(
            super::matching_environments(
                &[
                    EnvironmentPattern::from_str("foo*").unwrap(),
                    EnvironmentPattern::from_str("baz").unwrap(),
                ],
                names()
            )
            .unwrap_err()
            .debug_without_backtrace(),
            "environment \"baz\" not found"
        );
    }
}
