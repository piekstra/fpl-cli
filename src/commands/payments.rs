//! `fpl payments` — history, saved methods, and making a payment.
//!
//! `payments create` moves real money: a non-reversible mutation, so it
//! confirms by default and skips only with `--force`. A non-TTY run without
//! `--force` fails with a hint rather than auto-submitting.
//!
//! The request body mirrors what fpl.com's own pay-bill page builds
//! (`{ amount, paymentDate, donations }`, drawing the bank account on file).
//! It's reconstructed from the site's JS, not confirmed against a live submit —
//! a malformed body is rejected upstream, so it fails safe rather than
//! misrouting a payment.

use serde_json::json;

use crate::cli::PaymentsCommand;
use crate::commands::{confirm, stdin_is_tty, Ctx};
use crate::error::AppError;
use crate::output;

pub fn run(ctx: &Ctx, cmd: &PaymentsCommand) -> Result<(), AppError> {
    let fpl = ctx.connect()?;
    match cmd {
        PaymentsCommand::List { account_id } => {
            let account = ctx.resolve_account(account_id.as_deref(), &fpl)?;
            output::emit(
                ctx.cli.json,
                &fpl.account_history(&account)?,
                output::payments_list,
            );
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

            // Body shape mirrors fpl.com's pay-bill request: the amount, the
            // scheduled date, and a (usually empty) donations list. The draw
            // account is the bank account on file, not passed here.
            let body = json!({
                "amount": amount,
                "paymentDate": pay_date,
                "donations": [],
            });
            output::emit(
                ctx.cli.json,
                &fpl.make_payment(&account, &body)?,
                output::render,
            );
        }
    }
    Ok(())
}
