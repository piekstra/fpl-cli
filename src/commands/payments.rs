//! `fpl payments` — history, saved methods, and making a payment.
//!
//! `payments create` moves real money: a non-reversible mutation, so it
//! confirms by default and skips only with `--force`. A non-TTY run without
//! `--force` fails with a hint rather than auto-submitting.
//!
//! The submit is an HTTP **PUT** to the payment resource with the verified body
//! `{ amount, paymentDate, donations }`, drawing the bank account on file.
//!
//! **Critical, confirmed against a live submit:** FPL's payment endpoint
//! *commits the payment and then may still return an HTTP error* (a post-commit
//! confirmation step failing). The HTTP status is therefore **not** a reliable
//! success signal — a 400 does **not** mean the money didn't move. So rather
//! than trust the response, `create` records the account balance before
//! submitting and re-reads it after: a balance that dropped by the amount means
//! the payment posted, whatever the HTTP response said.

use serde_json::{json, Value};

use crate::cli::PaymentsCommand;
use crate::client::Fpl;
use crate::commands::{confirm, stdin_is_tty, Ctx};
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &PaymentsCommand) -> Result<(), AppError> {
    // Validate range flags before opening a session — a usage error should
    // fail fast, without touching the keychain or the network.
    let bounds = match cmd {
        PaymentsCommand::List { range, .. } => Some(output::range_bounds(range)?),
        _ => None,
    };
    let fpl = ctx.connect()?;
    match cmd {
        PaymentsCommand::List { account_id, range } => {
            let (since, until) = bounds.unwrap_or_default();
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let ledger = fpl.account_history(&account)?;
            match ledger.get("data").and_then(Value::as_array) {
                Some(rows) => {
                    let pymts: Vec<Value> = rows
                        .iter()
                        .filter(|r| output::is_payment_row(r))
                        .cloned()
                        .collect();
                    let rows = output::apply_range(
                        &pymts,
                        "debitCreditTransactionDate",
                        since,
                        until,
                        range.limit,
                    );
                    if ctx.cli.json {
                        // utility/v1: payment-list/v1 envelope.
                        output::payments(&rows);
                    } else {
                        output::payments_list(&json!({ "data": rows }));
                    }
                }
                // Shape drift: keep the schema promise (an empty list) in JSON
                // mode and the old fallback rendering in text mode.
                None if ctx.cli.json => output::payments(&[]),
                None => output::payments_list(&ledger),
            }
        }
        PaymentsCommand::Methods { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::emit(
                ctx.cli.json,
                &fpl.payment_options(&account)?,
                output::render,
            );
        }
        PaymentsCommand::Create {
            amount,
            date,
            account_id,
            force,
        } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            // The payment service takes an ISO `YYYY-MM-DD` date; normalize
            // whatever the user passed and default to today.
            let pay_date = match date {
                Some(d) => crate::dates::parse_iso(d)
                    .map(crate::dates::fmt_iso)
                    .unwrap_or_else(|_| d.clone()),
                None => crate::dates::fmt_iso(crate::dates::today()),
            };

            if !force {
                if !stdin_is_tty() {
                    return Err(AppError::ConfirmationRequired(
                        "stdin is not a TTY — pass --force to submit the payment \
                         non-interactively"
                            .into(),
                    ));
                }
                eprintln!("About to pay ${amount} on account {account} (date {pay_date}).");
                if !confirm("Submit this payment? [y/N] ")? {
                    return Err(AppError::Usage("payment cancelled".into()));
                }
            }

            let amount_f = amount
                .trim()
                .trim_start_matches('$')
                .replace(',', "")
                .parse::<f64>()
                .map_err(|_| {
                    AppError::Usage(format!(
                        "invalid --amount {amount:?} (use a number like 12.34)"
                    ))
                })?;

            return submit_and_verify(ctx, &fpl, &account, amount, amount_f, &pay_date);
        }
    }
    Ok(())
}

/// Read `data.currentAccountBalance` from a payment-option payload.
fn balance_of(opts: &Value) -> Option<f64> {
    opts.pointer("/data/currentAccountBalance")
        .and_then(Value::as_f64)
}

/// Submit a payment, then decide success from the account balance rather than
/// the HTTP status (FPL commits even when it returns an error — see module doc).
fn submit_and_verify(
    ctx: &Ctx,
    fpl: &Fpl,
    account: &str,
    amount: &str,
    amount_f: f64,
    pay_date: &str,
) -> Result<(), AppError> {
    // Body shape mirrors fpl.com's pay-bill request: the amount, the scheduled
    // date, and a (usually empty) donations list. The draw account is the bank
    // account on file, not passed here.
    let body = json!({ "amount": amount, "paymentDate": pay_date, "donations": [] });

    let before = balance_of(&fpl.payment_options(account)?);
    let submit = fpl.make_payment(account, &body);
    let after = fpl
        .payment_options(account)
        .ok()
        .as_ref()
        .and_then(balance_of);

    // Posted iff the balance dropped by (about) the amount paid.
    let posted =
        matches!((before, after), (Some(b0), Some(b1)) if (b0 - b1 - amount_f).abs() < 0.005);

    if ctx.cli.json {
        output::json(&json!({
            "posted": posted,
            "amount": amount_f,
            "account": account,
            "paymentDate": pay_date,
            "balanceBefore": before,
            "balanceAfter": after,
            "httpError": submit.as_ref().err().map(|e| e.to_string()),
        }));
    }

    if posted {
        if !ctx.cli.json {
            let bal = after
                .map(|b| format!("; account balance now {}", money(b)))
                .unwrap_or_default();
            println!(
                "Payment of {} posted to account {account}{bal}.",
                money(amount_f)
            );
            if submit.is_err() {
                eprintln!(
                    "note: FPL returned an error response, but the payment posted \
                     (verified against your balance)."
                );
            }
        }
        return Ok(());
    }

    // Not posted. Surface FPL's message if we have one; otherwise say so plainly.
    match submit {
        Err(e) => Err(e),
        Ok(_) => Err(AppError::Upstream(format!(
            "payment of {} did not post — balance unchanged (verify with `fpl payments methods`)",
            money(amount_f)
        ))),
    }
}

fn money(v: f64) -> String {
    format!("${v:.2}")
}
