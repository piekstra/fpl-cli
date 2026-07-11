//! `fpl history` — account, deposit, and document ledgers via `--type`.

use crate::cli::HistoryCommand;
use crate::commands::Ctx;
use crate::error::AppError;
use crate::output;

const TYPES: [&str; 2] = ["account", "deposit"];

pub fn run(ctx: &Ctx, cmd: &HistoryCommand) -> Result<(), AppError> {
    match cmd {
        HistoryCommand::Types => {
            for t in TYPES {
                println!("{t}");
            }
            Ok(())
        }
        HistoryCommand::List { account_id, r#type } => {
            let fpl = ctx.connect()?;
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            match r#type.to_lowercase().as_str() {
                "account" => output::ledger(&fpl.account_history(&account)?),
                "deposit" => output::render(&fpl.deposit_history(&account)?),
                other => {
                    return Err(AppError::Usage(format!(
                        "unknown history type {other:?} — valid: {} (see `fpl history types`)",
                        TYPES.join(", ")
                    )))
                }
            }
            Ok(())
        }
    }
}
