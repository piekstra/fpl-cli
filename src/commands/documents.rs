//! `fpl documents` — bill statement PDFs as the documents/v1 profile.
//!
//! FPL has no document ids: statements are addressed by bill date, so a
//! document's id is its ISO bill date. `documents list` enumerates the bill
//! history; `documents download <id>` fetches the PDF for that date — the same
//! statement `bills download --date <id>` produces. Reuses the bill history and
//! [`bills::bill_pdf`] so there is one download path, not two.

use std::io::Write;
use std::path::{Path, PathBuf};

use pk_cli_documents::{Document, DownloadBatch, Paged, SavedDocument};
use serde_json::Value;

use crate::cli::DocumentsCommand;
use crate::client::Fpl;
use crate::commands::{bills, Ctx};
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &DocumentsCommand) -> Result<(), AppError> {
    // Validate range flags before touching the keychain or the network.
    let bounds = match cmd {
        DocumentsCommand::List { range } => Some(output::range_bounds(range)?),
        _ => None,
    };
    let fpl = ctx.connect()?;
    let account = ctx.resolve_account(None, &fpl)?;
    match cmd {
        DocumentsCommand::List { range } => {
            let (since, until) = bounds.unwrap_or_default();
            let rows = bill_rows(&fpl, &account)?;
            let rows = output::apply_range(&rows, "dateBilled", since, until, range.limit);
            let docs: Vec<Document> = rows.iter().filter_map(document_of).collect();
            Paged::new("document", docs).emit(ctx.cli.json);
        }
        DocumentsCommand::Download { id, all, output } => {
            if *all {
                download_all(ctx, &fpl, &account, output.as_deref())?;
            } else {
                download_one(ctx, &fpl, &account, id.as_deref(), output.as_deref())?;
            }
        }
    }
    Ok(())
}

/// The bill-history rows, or an empty list if FPL's shape drifts (a redesign
/// empties the surface rather than crashing it).
fn bill_rows(fpl: &Fpl, account: &str) -> Result<Vec<Value>, AppError> {
    let history = fpl.bill_history(account)?;
    Ok(history
        .pointer("/data/data")
        .and_then(Value::as_array)
        .or_else(|| history.get("data").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default())
}

/// One bill-history row → a documents/v1 [`Document`]. The id is the ISO bill
/// date (FPL addresses statements by date); no financial fields — a bill's
/// amount is the utility/v1 statement's concern, not the file's.
fn document_of(r: &Value) -> Option<Document> {
    let date = output::iso_date(r.get("dateBilled")?.as_str()?);
    let mut d = Document::new(date.clone(), format!("FPL statement {date}"));
    d.date = Some(date);
    d.category = Some("bill".into());
    Some(d)
}

fn download_one(
    ctx: &Ctx,
    fpl: &Fpl,
    account: &str,
    id: Option<&str>,
    out: Option<&str>,
) -> Result<(), AppError> {
    let rows = bill_rows(fpl, account)?;
    if rows.is_empty() {
        return Err(AppError::NotFound(format!(
            "no statements found for account {account}"
        )));
    }
    // Match on the ISO-normalized date so an id from `documents list`
    // (`2026-03-15`) finds a row whose raw dateBilled carries a time tail.
    let row = match id {
        Some(want) => rows
            .iter()
            .find(|r| iso_of(r).as_deref() == Some(want))
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "no statement dated {want} — see `fpl documents list`"
                ))
            })?,
        None => &rows[0],
    };
    let (date_billed, date_print) = bill_dates(row)?;
    let pdf = bills::bill_pdf(fpl, account, &date_billed, &date_print)?;
    let iso = output::iso_date(&date_billed);

    // `-o -` streams the raw PDF to stdout; diagnostics go to stderr.
    if out == Some("-") {
        std::io::stdout()
            .write_all(&pdf)
            .map_err(|e| AppError::Other(format!("writing PDF to stdout: {e}")))?;
        return Ok(());
    }
    let default_name = default_name(account, &iso);
    let path = match out {
        Some(o) if Path::new(o).is_dir() => Path::new(o).join(&default_name),
        Some(o) => PathBuf::from(o),
        None => PathBuf::from(&default_name),
    };
    std::fs::write(&path, &pdf)
        .map_err(|e| AppError::Other(format!("writing {}: {e}", path.display())))?;

    let saved = saved_doc(&iso, &path, pdf.len());
    if ctx.cli.json {
        output::json(&serde_json::to_value(&saved).unwrap_or_default());
    } else {
        println!("{}", path.display());
        if !ctx.cli.quiet {
            eprintln!(
                "saved statement {iso} ({} KB) to {}",
                pdf.len() / 1024,
                path.display()
            );
        }
    }
    Ok(())
}

fn download_all(ctx: &Ctx, fpl: &Fpl, account: &str, out: Option<&str>) -> Result<(), AppError> {
    let rows = bill_rows(fpl, account)?;
    let dir = out.map(PathBuf::from).unwrap_or_default();
    if !dir.as_os_str().is_empty() && !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| AppError::Other(format!("creating {}: {e}", dir.display())))?;
    }

    let mut saved = Vec::with_capacity(rows.len());
    let mut failed: Vec<String> = Vec::new();
    for row in &rows {
        let Ok((date_billed, date_print)) = bill_dates(row) else {
            continue; // no dateBilled → nothing to fetch
        };
        let iso = output::iso_date(&date_billed);
        // FPL may not have every historical statement on file; a single missing
        // one must not abort the batch (and strand the files already written).
        // Skip it, record the date, and report the skips at the end.
        let pdf = match bills::bill_pdf(fpl, account, &date_billed, &date_print) {
            Ok(pdf) => pdf,
            Err(_) => {
                failed.push(iso);
                continue;
            }
        };
        let name = default_name(account, &iso);
        let path = if dir.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            dir.join(&name)
        };
        std::fs::write(&path, &pdf)
            .map_err(|e| AppError::Other(format!("writing {}: {e}", path.display())))?;
        saved.push(saved_doc(&iso, &path, pdf.len()));
    }

    let where_to = if dir.as_os_str().is_empty() {
        ".".to_string()
    } else {
        dir.display().to_string()
    };
    let batch = DownloadBatch::new(where_to, saved);
    if ctx.cli.json {
        output::json(&serde_json::to_value(&batch).unwrap_or_default());
    } else {
        for it in &batch.items {
            println!("{}", it.path);
        }
        if !ctx.cli.quiet {
            eprintln!(
                "saved {} statement(s), {} bytes → {}",
                batch.count, batch.bytes_total, batch.dir
            );
        }
    }
    // Diagnostics on stderr keep stdout (the DTO / the paths) clean. Not in the
    // document-download-batch/v1 DTO, which reports only what was written.
    if !failed.is_empty() && !ctx.cli.quiet {
        eprintln!(
            "skipped {} statement(s) FPL had no PDF for: {}",
            failed.len(),
            failed.join(", ")
        );
    }
    Ok(())
}

/// Raw `dateBilled` / `datePrint` for the download API (datePrint falls back to
/// dateBilled, mirroring `bills download`).
fn bill_dates(row: &Value) -> Result<(String, String), AppError> {
    let date_billed = row
        .get("dateBilled")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Other("statement row is missing dateBilled".into()))?
        .to_string();
    let date_print = row
        .get("datePrint")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| date_billed.clone());
    Ok((date_billed, date_print))
}

fn iso_of(r: &Value) -> Option<String> {
    Some(output::iso_date(r.get("dateBilled")?.as_str()?))
}

fn default_name(account: &str, iso: &str) -> String {
    format!("fpl-bill-{account}-{iso}.pdf")
}

fn saved_doc(id: &str, path: &Path, bytes: usize) -> SavedDocument {
    let mut doc = Document::new(id.to_string(), format!("FPL statement {id}"));
    doc.date = Some(id.to_string());
    doc.category = Some("bill".into());
    SavedDocument::from_document(&doc, path.display().to_string(), bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn document_of_uses_the_iso_bill_date_as_id() {
        // A raw datetime dateBilled normalizes to an ISO id, and the row maps to
        // a conforming documents/v1 Document (category "bill", no amount).
        let row = json!({"dateBilled": "2026-05-28T00:00:00.000", "totalBillAmount": 196.16});
        let d = document_of(&row).expect("maps to a Document");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["id"], "2026-05-28");
        assert_eq!(v["date"], "2026-05-28");
        assert_eq!(v["category"], "bill");
        assert!(
            v.get("amount").is_none(),
            "no financial fields on a document"
        );
    }

    #[test]
    fn document_list_envelope_is_document_list_v1() {
        let rows = [
            json!({"dateBilled": "2026-06-26"}),
            json!({"dateBilled": "2026-05-28"}),
        ];
        let docs: Vec<Document> = rows.iter().filter_map(document_of).collect();
        let v = serde_json::to_value(Paged::new("document", docs)).unwrap();
        assert_eq!(v["schema"], "document-list/v1");
        assert_eq!(v["items"][0]["id"], "2026-06-26");
    }

    #[test]
    fn a_row_without_a_date_is_skipped_not_panicked() {
        assert!(document_of(&json!({"totalBillAmount": 10.0})).is_none());
    }

    #[test]
    fn an_iso_id_matches_a_datebilled_carrying_a_time_tail() {
        // The headline fix: an id from `documents list` (`2026-03-15`) must
        // resolve a row whose raw dateBilled is `2026-03-15T00:00:00.000`, which
        // is why `download_one` matches on the ISO-normalized date, not the raw.
        let rows = [
            json!({"dateBilled": "2026-04-27"}),
            json!({"dateBilled": "2026-03-15T00:00:00.000"}),
        ];
        assert_eq!(iso_of(&rows[1]).as_deref(), Some("2026-03-15"));
        let found = rows
            .iter()
            .find(|r| iso_of(r).as_deref() == Some("2026-03-15"));
        assert!(found.is_some(), "ISO id resolves the time-tailed row");
        // A raw-string compare (the pre-fix behavior) would miss it.
        assert_ne!(
            rows[1].get("dateBilled").and_then(Value::as_str),
            Some("2026-03-15")
        );
    }
}
