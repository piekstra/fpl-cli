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
            output::emit(
                ctx.cli.json,
                &fpl.bill_history(&account)?,
                output::bills_list,
            );
        }
        BillsCommand::Get { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let c = account_ctx(&fpl, &account)?;
            output::emit(
                ctx.cli.json,
                &fpl.energy_usage(&account, &c.premise, &c.last_billed, &c.meter)?,
                output::bill_summary,
            );
        }
        BillsCommand::Projected { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let c = account_ctx(&fpl, &account)?;
            output::emit(
                ctx.cli.json,
                &fpl.projected_bill(&account, &c.premise, &c.last_billed)?,
                output::bill_summary,
            );
        }
        BillsCommand::Budget { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::emit(ctx.cli.json, &fpl.budget_billing(&account)?, output::budget);
        }
    }
    Ok(())
}
