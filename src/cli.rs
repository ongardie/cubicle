use anyhow::Result;
use clap::{Parser, Subcommand};
use clap_complete::{generate, shells::Shell};
use std::io;

use super::{
    package_set_from_names, Clean, Cubicle, EnvironmentName, ListFormat, ListPackagesFormat, Quiet,
};

/// Manage sandboxed development environments.
#[derive(Debug, Parser)]
// clap shows only brief help messages (the top line of comments) with `-h` and
// longer messages with `--help`. This custom help message gives people some
// hope of learning that distinction. See
// <https://github.com/clap-rs/clap/issues/1015>.
#[clap(help_message("Print help information. Use --help for more details"))]
pub struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate tab-completions for your shell.
    ///
    /// Installation for Bash:
    ///
    ///     $ cub completions bash > ~/.local/share/bash-completion/completions/cub
    ///
    /// Installation for ZSH (depending on `$fpath`):
    ///
    ///     $ cub completions zsh > ~/.zfunc/_cub
    ///
    /// You may need to restart your shell or configure it.
    ///
    /// This installation works similarly as for rustup's completions. For
    /// detailed instructions, see:
    ///
    ///     $ rustup help completions
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

    /// Show available packages.
    Packages {
        /// Set output format.
        #[clap(long, value_enum, default_value_t)]
        format: ListPackagesFormat,
    },

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

pub fn parse() -> Args {
    Args::parse()
}

fn print_completions(shell: Shell) -> Result<()> {
    use clap::CommandFactory;
    let cmd = &mut Args::command();
    let out = &mut io::stdout();

    // We can't list out environment names and package names statically.
    // Unfortunately, there seems to be no general way to tell `clap` about
    // these dynamic lists. For ZSH, we hack calls to this program into the
    // generated output. (Similar contributions would be welcome for Bash).
    if shell == Shell::Zsh {
        let mut buf: Vec<u8> = Vec::new();
        generate(shell, cmd, "cub", &mut buf);
        let buf = String::from_utf8(buf)?;
        let mut counts = [0; 4];
        for line in buf.lines() {
            match line {
                r#"':name -- Environment name:' \"# => {
                    counts[0] += 1;
                    println!(r#"':name -- Environment name:_cub_envs' \"#)
                }
                r#"'*::names -- Environment name(s):' \"# => {
                    counts[1] += 1;
                    println!(r#"'*::names -- Environment name(s):_cub_envs' \"#)
                }
                r#"'*--packages=[Comma-separated names of packages to inject into home directory]:PACKAGES: ' \"# =>
                {
                    counts[2] += 1;
                    println!(
                        r#"'*--packages=[Comma-separated names of packages to inject into home directory]:PACKAGES:_cub_pkgs' \"#
                    )
                }
                r#"_cub "$@""# => {
                    counts[3] += 1;
                    println!(
                        "{}",
                        r#"
_cub_envs() {
    _values -w 'environments' $(cub list --format=names)
}
_cub_pkgs() {
    _values -s , -w 'packages' $(cub packages --format=names)
}
"#
                    );
                    println!("{}", line);
                }
                _ => println!("{}", line),
            }
        }
        debug_assert_eq!(counts, [2, 2, 3, 1], "completions not patched as expected",);
    } else {
        generate(shell, cmd, "cub", out);
    }
    Ok(())
}

pub(super) fn run(args: Args, program: &Cubicle) -> Result<()> {
    use Commands::*;
    match args.command {
        Completions { shell } => print_completions(shell),
        Enter { name } => program.enter_environment(&name),
        Exec { name, command } => program.exec_environment(&name, &command),
        List { format } => program.list_environments(format),
        New {
            name,
            enter,
            packages,
        } => {
            let packages = packages.map(package_set_from_names).transpose()?;
            program.new_environment(&name, packages)?;
            if enter {
                program.enter_environment(&name)?;
            }
            Ok(())
        }
        Packages { format } => program.list_packages(format),
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
                program.reset_environment(name, &packages, Clean(clean))?;
            }
            Ok(())
        }
        Tmp { packages } => {
            let packages = packages.map(package_set_from_names).transpose()?;
            program.create_enter_tmp_environment(packages)
        }
    }
}
