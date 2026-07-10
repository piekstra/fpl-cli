//! `fpl bills` — current/projected bill, history, budget billing, PDF download.

use crate::cli::BillsCommand;
use crate::commands::{account_ctx, Ctx};
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &BillsCommand) -> Result<(), AppError> {
    let fpl = ctx.connect()?;
    match cmd {
        BillsCommand::List { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::render(&fpl.bill_history(&account)?);
        }
        BillsCommand::Get { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let c = account_ctx(&fpl, &account)?;
            output::render(&fpl.energy_usage(&account, &c.premise, &c.last_billed, &c.meter)?);
        }
        BillsCommand::Projected { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let c = account_ctx(&fpl, &account)?;
            output::render(&fpl.projected_bill(&account, &c.premise, &c.last_billed)?);
        }
        BillsCommand::Budget { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::render(&fpl.budget_billing(&account)?);
        }
        BillsCommand::Download { account_id, output } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let bytes = fpl.download_bill(&account)?;
            std::fs::write(output, &bytes)
                .map_err(|e| AppError::Other(format!("writing {output}: {e}")))?;
            if !ctx.cli.quiet {
                eprintln!("wrote {} bytes to {output}", bytes.len());
            }
        }
    }
    Ok(())
}
