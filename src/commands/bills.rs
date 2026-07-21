//! `fpl bills` — current/projected bill, history, budget billing, PDF download.

use std::io::Write;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};

use crate::cli::BillsCommand;
use crate::client::Fpl;
use crate::commands::{account_ctx, Ctx};
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &BillsCommand) -> Result<(), AppError> {
    // Validate range flags before opening a session — a usage error should
    // fail fast, without touching the keychain or the network.
    let bounds = match cmd {
        BillsCommand::List { range, .. } => Some(output::range_bounds(range)?),
        _ => None,
    };
    let fpl = ctx.connect()?;
    match cmd {
        BillsCommand::List { account_id, range } => {
            let (since, until) = bounds.unwrap_or_default();
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let history = fpl.bill_history(&account)?;
            // Rows nest under `data.data`; tolerate a flat `data` array too
            // (mirroring the text renderer's fallback).
            let rows = history
                .pointer("/data/data")
                .and_then(Value::as_array)
                .or_else(|| history.get("data").and_then(Value::as_array));
            match rows {
                Some(rows) => {
                    let rows = output::apply_range(rows, "dateBilled", since, until, range.limit);
                    if ctx.cli.json {
                        // utility/v1: statement-list/v1 envelope.
                        output::statements(&rows);
                    } else {
                        output::bills_list(&json!({ "data": { "data": rows } }));
                    }
                }
                // Shape drift: keep the schema promise (an empty list) in JSON
                // mode and the old fallback rendering in text mode.
                None if ctx.cli.json => output::statements(&[]),
                None => output::bills_list(&history),
            }
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
        BillsCommand::Download {
            account_id,
            date,
            output,
        } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            return download(ctx, &fpl, &account, date.as_deref(), output.as_deref());
        }
    }
    Ok(())
}

/// Fetch a bill PDF and write it to a file (or stdout with `-o -`).
fn download(
    ctx: &Ctx,
    fpl: &Fpl,
    account: &str,
    date: Option<&str>,
    output: Option<&str>,
) -> Result<(), AppError> {
    let history = fpl.bill_history(account)?;
    let bills = history
        .pointer("/data/data")
        .and_then(|v| v.as_array())
        .filter(|b| !b.is_empty())
        .ok_or_else(|| AppError::NotFound(format!("no bills found for account {account}")))?;

    // Pick the requested bill (by billed date) or the most recent one.
    let field =
        |b: &serde_json::Value, k: &str| b.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let bill = match date {
        Some(d) => bills
            .iter()
            .find(|b| field(b, "dateBilled").as_deref() == Some(d))
            .ok_or_else(|| {
                AppError::NotFound(format!("no bill dated {d} — see `fpl bills list`"))
            })?,
        None => &bills[0],
    };
    let date_billed = field(bill, "dateBilled")
        .ok_or_else(|| AppError::Other("bill row is missing dateBilled".into()))?;
    let date_print = field(bill, "datePrint").unwrap_or_else(|| date_billed.clone());

    let resp = fpl.download_bill(account, &date_billed, &date_print)?;
    let b64: String = resp
        .pointer("/data/bytes")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Upstream("bill download returned no PDF data".into()))?
        .split_whitespace()
        .collect();
    let pdf = STANDARD
        .decode(b64)
        .map_err(|e| AppError::Other(format!("could not decode the bill PDF: {e}")))?;
    if !pdf.starts_with(b"%PDF") {
        return Err(AppError::Upstream(
            "bill download did not return a PDF (FPL may not have this statement on file)".into(),
        ));
    }

    // `-o -` streams the raw PDF to stdout; otherwise write a file.
    if output == Some("-") {
        std::io::stdout()
            .write_all(&pdf)
            .map_err(|e| AppError::Other(format!("writing PDF to stdout: {e}")))?;
        return Ok(());
    }
    let default_name = format!("fpl-bill-{account}-{date_billed}.pdf");
    let path = output.unwrap_or(&default_name);
    std::fs::write(path, &pdf).map_err(|e| AppError::Other(format!("writing {path}: {e}")))?;

    if ctx.cli.json {
        output::json(&json!({
            "account": account,
            "billDate": date_billed,
            "file": path,
            "sizeBytes": pdf.len(),
        }));
    } else {
        // Path to stdout (scriptable); human note to stderr.
        println!("{path}");
        if !ctx.cli.quiet {
            eprintln!(
                "saved bill {date_billed} ({} KB) to {path}",
                pdf.len() / 1024
            );
        }
    }
    Ok(())
}
