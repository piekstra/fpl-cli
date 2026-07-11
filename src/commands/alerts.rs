//! `fpl alerts` — account alert/banner state.

use crate::commands::Ctx;
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, account_id: Option<&str>) -> Result<(), AppError> {
    let fpl = ctx.connect()?;
    let account = ctx.resolve_account(account_id, &fpl)?;
    output::emit(ctx.cli.json, &fpl.alerts(&account)?, output::render);
    Ok(())
}
