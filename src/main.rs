use colored::*;
use structopt::StructOpt;
use toml_sort::{Config, Opt, ProcessedConfig, Res};

fn main() -> Res<()> {
    let opt = Opt::from_args();

    let config = Config::read_from_file().unwrap_or_else(|| {
        println!(
            "{}",
            "No 'toml-sort.toml' in this directory and its parents, using default config.\n"
                .yellow()
        );
        Config::default()
    });

    let config: ProcessedConfig = config.into();

    if opt.files.is_empty() {
        let _ = Opt::clap().print_help();
        println!();
        std::process::exit(1);
    }

    for file in opt.files {
        config.process_file(file, opt.check)?;
    }

    Ok(())
}
