//! `fpl history` — account, deposit, and document ledgers via `--type`.

use serde_json::{json, Value};

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
        HistoryCommand::List {
            account_id,
            r#type,
            range,
        } => {
            let (since, until) = output::range_bounds(range)?;
            let kind = r#type.to_lowercase();
            // Each ledger keeps its established text renderer.
            let text: fn(&Value) = match kind.as_str() {
                "account" => output::ledger,
                "deposit" => output::render,
                other => {
                    return Err(AppError::Usage(format!(
                        "unknown history type {other:?} — valid: {} (see `fpl history types`)",
                        TYPES.join(", ")
                    )))
                }
            };
            let fpl = ctx.connect()?;
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let ledger = match kind.as_str() {
                "account" => fpl.account_history(&account)?,
                _ => fpl.deposit_history(&account)?,
            };
            match ledger.get("data").and_then(Value::as_array) {
                Some(rows) => {
                    let rows = output::apply_range(
                        rows,
                        "debitCreditTransactionDate",
                        since,
                        until,
                        range.limit,
                    );
                    if ctx.cli.json {
                        // utility/v1: transaction-list/v1 envelope.
                        output::transactions(&rows, &kind);
                    } else {
                        text(&json!({ "data": rows }));
                    }
                }
                // Shape drift: keep the schema promise (an empty list) in JSON
                // mode and the old fallback rendering in text mode.
                None if ctx.cli.json => output::transactions(&[], &kind),
                None => text(&ledger),
            }
            Ok(())
        }
    }
}
