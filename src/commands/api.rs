//! `fpl api` — raw authenticated request to any FPL endpoint. Round-trip JSON,
//! so it always emits JSON. The escape hatch for endpoints without a first-class
//! command, and for inspecting response shapes while they're being mapped.

use serde_json::Value;

use crate::cli::ApiArgs;
use crate::commands::Ctx;
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, args: &ApiArgs) -> Result<(), AppError> {
    let body: Option<Value> = match &args.data {
        Some(s) => Some(
            serde_json::from_str(s)
                .map_err(|e| AppError::Usage(format!("--data is not valid JSON: {e}")))?,
        ),
        None => None,
    };
    let fpl = ctx.connect()?;
    let v = fpl.request(&args.method, &args.path, body.as_ref())?;
    output::json(&v);
    Ok(())
}
