use clap::Parser;

/// ruci command line
#[derive(Parser, Debug)]
#[command(author = "e")]
#[command(version, about, long_about = None)]
struct Args {
    /// rucimp mode, c(chain) or s(suit)
    #[arg(short, long, default_value_t = 'c')]
    mode: char,

    /// basic config file
    #[arg(short, long, default_value = "local.lua")]
    config_file: String,
}

fn main() {
    let args = Args::parse();

    println!("Hello {}!", args.mode);
    println!("Hello {}!", args.config_file)
}
