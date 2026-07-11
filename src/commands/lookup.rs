//! `fpl lookup` — public FPL reference data (cities, ZIP codes).

use crate::cli::LookupCommand;
use crate::commands::Ctx;
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &LookupCommand) -> Result<(), AppError> {
    let fpl = ctx.connect()?;
    let v = match cmd {
        LookupCommand::Cities => fpl.cities()?,
        LookupCommand::Zips => fpl.zips()?,
    };
    output::emit(ctx.cli.json, &v, output::render);
    Ok(())
}
