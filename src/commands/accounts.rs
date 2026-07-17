//! `fpl accounts` — list, detail, active-account selection, balance.

use crate::cli::AccountsCommand;
use crate::commands::Ctx;
use crate::config::Config;
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &AccountsCommand) -> Result<(), AppError> {
    match cmd {
        AccountsCommand::List => {
            let fpl = ctx.connect()?;
            output::accounts(&fpl.accounts()?);
        }
        AccountsCommand::Get { account_id } => {
            let fpl = ctx.connect()?;
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::emit(
                ctx.cli.json,
                &fpl.account_detail(&account)?,
                output::account_detail,
            );
        }
        AccountsCommand::Use { account_id } => {
            let mut cfg = Config::load()?;
            cfg.account = Some(account_id.clone());
            cfg.save()?;
            if !ctx.cli.quiet {
                eprintln!("active account set to {account_id}");
            }
        }
        AccountsCommand::Balance { account_id } => {
            let fpl = ctx.connect()?;
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::emit(ctx.cli.json, &fpl.balance(&account)?, output::balance);
        }
    }
    Ok(())
}
