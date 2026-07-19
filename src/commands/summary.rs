//! `fpl summary` — at-a-glance dashboard for one account: balance, bill cycle,
//! projected bill and usage. Aggregates the account detail (address, balance,
//! cycle) with the current-period energy service (projection).

use crate::client::Fpl;
use crate::commands::{detail_str, mmddyyyy_from_raw, Ctx};
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, account_id: Option<&str>) -> Result<(), AppError> {
    let fpl = ctx.connect()?;
    let account = ctx.resolve_account(account_id, &fpl)?;

    let detail = fpl.account_detail(&account)?;

    // utility/v1: `--json` emits the canonical utility-summary/v1 card (no
    // energy fetch needed). The full dashboard stays in text mode.
    if ctx.cli.json {
        output::utility_summary(&detail, &account);
        return Ok(());
    }

    let premise = Fpl::premise_of(&detail).ok_or_else(|| {
        AppError::NotFound("could not read the premise number from account detail".into())
    })?;
    let meter = detail_str(&detail, "meterNo").unwrap_or_default();
    let last_billed = detail_str(&detail, "currentBillDate")
        .and_then(|raw| mmddyyyy_from_raw(&raw))
        .ok_or_else(|| {
            AppError::NotFound("could not read the current bill date from account detail".into())
        })?;

    let energy = fpl.energy_usage(&account, &premise, &last_billed, &meter)?;
    output::summary_text(&detail, &energy);
    Ok(())
}
