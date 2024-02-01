mod cli;

use cubicle::{config::Config, Cubicle, Result};

fn main() -> Result<()> {
    let args = cli::parse();
    let config = Config::read_from_file(args.config_path())?;
    let program = Cubicle::new(config)?;
    cli::run(args, &program)
}
