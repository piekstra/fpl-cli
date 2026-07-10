//! `fpl outages` — public county-level outage counts (no login required).

use crate::cli::OutagesCommand;
use crate::commands::Ctx;
use crate::error::AppError;
use crate::{client, output};

pub fn run(_ctx: &Ctx, cmd: &OutagesCommand) -> Result<(), AppError> {
    match cmd {
        OutagesCommand::List { name } => {
            let v = client::county_outages()?;
            output::outages(&v, name.as_deref());
            Ok(())
        }
    }
}
