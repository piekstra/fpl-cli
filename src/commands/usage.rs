//! `fpl usage` — current-period summary, hourly detail, appliance breakdown.

use crate::cli::UsageCommand;
use crate::commands::{account_ctx, Ctx};
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &UsageCommand) -> Result<(), AppError> {
    let fpl = ctx.connect()?;
    match cmd {
        UsageCommand::Get { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let c = account_ctx(&fpl, &account)?;
            output::emit(
                ctx.cli.json,
                &fpl.energy_usage(&account, &c.premise, &c.last_billed, &c.meter)?,
                output::usage_summary,
            );
        }
        UsageCommand::Hourly { account_id, date } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let c = account_ctx(&fpl, &account)?;
            let day = date
                .clone()
                .unwrap_or_else(|| crate::dates::fmt_mm_dd_yyyy(crate::dates::yesterday()));
            output::emit(
                ctx.cli.json,
                &fpl.hourly_usage(&account, &c.premise, &day)?,
                output::hourly,
            );
        }
        UsageCommand::Appliances { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let c = account_ctx(&fpl, &account)?;
            output::emit(
                ctx.cli.json,
                &fpl.appliance_usage(&account, &c.premise)?,
                output::appliances,
            );
        }
    }
    Ok(())
}
