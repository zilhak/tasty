//! `tasty-tui-sim` — standalone binary entry point for the VTE sequence
//! simulator. All logic lives in the library (`tasty_tui_simulator`) so it can
//! be shared with the `tasty debug sim` subcommand.

use clap::Parser;

use tasty_tui_simulator::Commands;

#[derive(Parser)]
#[command(name = "tasty-tui-sim", about = "VTE sequence simulator for tasty")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

fn main() {
    let cli = Cli::parse();
    tasty_tui_simulator::run(&cli.command);
}
