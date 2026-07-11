//! `fpl payments` — history, saved methods, and making a payment.
//!
//! `payments create` moves real money: a non-reversible mutation, so it
//! confirms by default and skips only with `--force`. A non-TTY run without
//! `--force` fails with a hint rather than auto-submitting.

use serde_json::{json, Value};

use crate::cli::PaymentsCommand;
use crate::commands::{confirm, stdin_is_tty, Ctx};
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &PaymentsCommand) -> Result<(), AppError> {
    let fpl = ctx.connect()?;
    match cmd {
        PaymentsCommand::List { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::payments_list(&fpl.account_history(&account)?);
        }
        PaymentsCommand::Methods { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::render(&fpl.payment_options(&account)?);
        }
        PaymentsCommand::Create {
            amount,
            date,
            method,
            account_id,
            force,
        } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            let pay_date = date
                .clone()
                .unwrap_or_else(|| crate::dates::fmt_mm_dd_yyyy(crate::dates::today()));

            if !force {
                if !stdin_is_tty() {
                    return Err(AppError::Usage(
                        "stdin is not a TTY — pass --force to submit the payment \
                         non-interactively"
                            .into(),
                    ));
                }
                eprintln!(
                    "About to pay ${amount} on account {account} (date {pay_date}{}).",
                    method
                        .as_deref()
                        .map(|m| format!(", method {m}"))
                        .unwrap_or_default()
                );
                if !confirm("Submit this payment? [y/N] ")? {
                    return Err(AppError::Usage("payment cancelled".into()));
                }
            }

            let mut body = json!({ "paymentAmount": amount, "paymentDate": pay_date });
            if let Some(m) = method {
                body["paymentMethod"] = Value::String(m.clone());
            }
            output::render(&fpl.make_payment(&account, &body)?);
        }
    }
    Ok(())
}
