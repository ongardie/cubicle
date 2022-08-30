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

mod cli;

use cubicle::{config::Config, Cubicle, Result};

fn main() -> Result<()> {
    let args = cli::parse();
    let config = Config::read_from_file(args.config_path())?;
    let program = Cubicle::new(config)?;
    cli::run(args, &program)
}
