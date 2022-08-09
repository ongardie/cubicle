use clap::{Parser, Subcommand};

/// Manage sandboxed development environments.
#[derive(Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run a shell in an existing environment.
    Enter {
        /// Environment name.
        name: String,
    },
}

fn main() {
    let args = Args::parse();
    todo!("{:#?}", args);
}
