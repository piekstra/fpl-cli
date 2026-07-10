mod cli;
mod client;
mod commands;
mod config;
mod dates;
mod error;
mod output;
mod secrets;

use clap::Parser;

use cli::{Cli, Command};
use commands::Ctx;
use config::Config;
use error::AppError;

fn main() {
    let cli = Cli::parse();
    let quiet = cli.quiet;
    if let Err(e) = run(cli) {
        if !quiet {
            eprintln!("error: {e}");
        }
        std::process::exit(e.exit_code());
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    let cfg = Config::load()?;
    let ctx = Ctx {
        cli: &cli,
        cfg: &cfg,
    };
    match &cli.command {
        Command::Init(args) => commands::init::run(&ctx, args),
        Command::SetCredential(args) => commands::set_credential::run(&ctx, args),
        Command::Auth(cmd) => commands::auth::run(&ctx, cmd),
        Command::Accounts(cmd) => commands::accounts::run(&ctx, cmd),
        Command::Bills(cmd) => commands::bills::run(&ctx, cmd),
        Command::Payments(cmd) => commands::payments::run(&ctx, cmd),
        Command::Usage(cmd) => commands::usage::run(&ctx, cmd),
        Command::History(cmd) => commands::history::run(&ctx, cmd),
        Command::Outages(cmd) => commands::outages::run(&ctx, cmd),
        Command::Api(args) => commands::api::run(&ctx, args),
    }
}
