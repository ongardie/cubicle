mod cli;

use cubicle::{Cubicle, Result, config::Config};

fn main() -> Result<()> {
    let args = cli::parse();
    let config = Config::read_from_file(args.config_path())?;
    let program = Cubicle::new(config)?;
    cli::run(args, &program)
}
