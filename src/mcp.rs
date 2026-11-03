//! `fpl mcp`: the shared `utility/v1` tools (`pk_cli_utility::mcp`) over FPL.
//!
//! This module supplies what is FPL's own: [`Portal`], the reads the commands
//! already make, and [`FplSource`], which maps them with the commands' mappers
//! in `output.rs`. Tests put fixtures behind [`Portal`], so nothing here needs a
//! login to test.
//!
//! The server signs in as the commands do, with the stored password, and keeps
//! the session while it runs. It reads the credential and never writes one: a
//! password still under the legacy keychain service stays there rather than
//! being migrated, because an MCP server must not change the keychain.
//!
//! The commands' mappers are best-effort, for a person who can see the raw
//! payload. A tool's answer goes to a model as fact, so here an answer FPL did
//! not give (a balance no field states, rows none of which can be read) is an
//! upstream error, never 0.00 or an empty list.

use std::sync::{Mutex, PoisonError};

use pk_cli_mcp::Server;
use pk_cli_utility::mcp::{UtilityAccount, UtilitySource, UtilityTools};
use pk_cli_utility::{Payment, Statement, UsagePeriod, UtilitySummary};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{AccountSummary, Fpl};
use crate::commands::{LEGACY_SERVICE, SERVICE};
use crate::config::Config;
use crate::error::AppError;
use crate::output;
use crate::secrets::{CredentialStore, Secret};

/// The password's environment variable, read before the keychain, so a Claude
/// Desktop extension can pass what its settings hold.
const PASSWORD_ENV: &str = "FPL_PASSWORD";

/// The tools `fpl mcp` serves.
pub fn tools(cli: &Cli, cfg: &Config) -> UtilityTools<FplSource<LivePortal>> {
    let source = FplSource {
        portal: LivePortal::new(cli.username.clone().or_else(|| cfg.username.clone())),
        default_account: cli
            .account
            .clone()
            .or_else(|| cfg.account.clone())
            .filter(|a| !a.is_empty()),
    };
    let password_in_env = std::env::var(PASSWORD_ENV).is_ok_and(|v| !v.is_empty());
    with_source(source, password_in_env)
}

fn with_source<P: Portal>(
    source: FplSource<P>,
    password_in_env: bool,
) -> UtilityTools<FplSource<P>> {
    let mut server = Server::new("fpl", env!("CARGO_PKG_VERSION"), "FPL", "fpl").instructions(
        "FPL access here is read-only: no tool can make a payment or change anything. Payments \
         come from FPL's 25 most recent ledger entries, so an older payment is not listed. Usage \
         is the kWh on each bill. A bill's paid state is not reported.",
    );
    // The keychain is not read while the variable is set, and a Claude Desktop
    // extension bundles its binary, so `fpl auth login` would fix nothing.
    if password_in_env {
        server = server.hosted(
            "through the open-source `fpl` tool; the password stays on the user's machine, in \
             the settings of the program that started this server",
            "ask the user to update the FPL password where it is set: for the Claude Desktop \
             extension, in its settings; otherwise $FPL_PASSWORD",
        );
    }
    UtilityTools::new(server, source).metering("electricity use")
}

/// The FPL reads the tools need. The JSON is FPL's own, as `output.rs` maps it.
pub trait Portal {
    fn accounts(&self) -> Result<Vec<AccountSummary>, AppError>;
    fn account_detail(&self, account: &str) -> Result<Value, AppError>;
    fn bill_history(&self, account: &str) -> Result<Value, AppError>;
    fn account_history(&self, account: &str) -> Result<Value, AppError>;
}

/// `Portal` over fpl.com.
pub struct LivePortal {
    username: Option<String>,
    state: Mutex<SignIn<Fpl>>,
}

/// A kept session, and the password FPL last refused.
struct SignIn<S> {
    session: Option<S>,
    /// Not sent again, so a server left running after a password change cannot
    /// lock the account by retrying it: the next call that finds a different
    /// password tries that. Only a refusal lands here; FPL being down does not.
    refused: Option<Secret>,
}

impl<S> Default for SignIn<S> {
    fn default() -> Self {
        Self {
            session: None,
            refused: None,
        }
    }
}

impl<S> SignIn<S> {
    /// One read on the kept session, signing in when there is none and once
    /// more when FPL says the session has lapsed.
    fn read<T>(
        &mut self,
        password: impl Fn() -> Result<Secret, AppError>,
        login: impl Fn(&Secret) -> Result<S, AppError>,
        op: impl Fn(&S) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        if self.session.is_none() {
            self.sign_in(&password, &login)?;
        }
        match op(self.session.as_ref().expect("signed in above")) {
            Err(AppError::Auth(_)) => {
                self.session = None;
                self.sign_in(&password, &login)?;
                op(self.session.as_ref().expect("signed in above"))
            }
            other => other,
        }
    }

    fn sign_in(
        &mut self,
        password: &impl Fn() -> Result<Secret, AppError>,
        login: &impl Fn(&Secret) -> Result<S, AppError>,
    ) -> Result<(), AppError> {
        let password = password()?;
        let refused = self.refused.as_ref();
        if refused.is_some_and(|r| r.expose() == password.expose()) {
            return Err(AppError::Auth(
                "FPL refused this password on an earlier call, so it was not sent again".into(),
            ));
        }
        match login(&password) {
            Ok(session) => {
                self.session = Some(session);
                self.refused = None;
                Ok(())
            }
            Err(e) => {
                if matches!(e, AppError::Auth(_)) {
                    self.refused = Some(password);
                }
                Err(e)
            }
        }
    }
}

impl LivePortal {
    fn new(username: Option<String>) -> Self {
        Self {
            username: username.filter(|u| !u.is_empty()),
            state: Mutex::default(),
        }
    }

    fn read<T>(&self, op: impl Fn(&Fpl) -> Result<T, AppError>) -> Result<T, AppError> {
        let username = self
            .username
            .as_deref()
            .ok_or_else(|| AppError::Auth("no FPL username is configured".into()))?;
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .read(
                || {
                    password_from(std::env::var(PASSWORD_ENV).ok(), || read_keychain(username))?
                        .ok_or_else(|| {
                            AppError::Auth(format!("no stored password for {username:?}"))
                        })
                },
                |password| Fpl::login(username, password),
                op,
            )
    }
}

impl Portal for LivePortal {
    fn accounts(&self) -> Result<Vec<AccountSummary>, AppError> {
        self.read(Fpl::accounts)
    }

    fn account_detail(&self, account: &str) -> Result<Value, AppError> {
        self.read(|fpl| fpl.account_detail(account))
    }

    fn bill_history(&self, account: &str) -> Result<Value, AppError> {
        self.read(|fpl| fpl.bill_history(account))
    }

    fn account_history(&self, account: &str) -> Result<Value, AppError> {
        self.read(|fpl| fpl.account_history(account))
    }
}

/// The environment variable when it holds something, else the keychain.
fn password_from(
    env: Option<String>,
    keychain: impl FnOnce() -> Result<Option<Secret>, AppError>,
) -> Result<Option<Secret>, AppError> {
    match env.filter(|v| !v.is_empty()) {
        Some(value) => Ok(Some(Secret::new(value))),
        None => keychain(),
    }
}

fn read_keychain(username: &str) -> Result<Option<Secret>, AppError> {
    if let Some(secret) = CredentialStore::new(SERVICE).get(username)? {
        return Ok(Some(secret));
    }
    CredentialStore::new(LEGACY_SERVICE).get(username)
}

/// [`UtilitySource`] over a [`Portal`].
pub struct FplSource<P> {
    portal: P,
    /// `--account`, `$FPL_ACCOUNT`, or the config's active account.
    default_account: Option<String>,
}

impl<P: Portal> FplSource<P> {
    /// The named account, else the default, else the login's first, the order
    /// the commands use.
    fn account(&self, named: Option<&str>) -> Result<String, AppError> {
        if let Some(account) = named.or(self.default_account.as_deref()) {
            return Ok(account.to_string());
        }
        self.portal
            .accounts()?
            .into_iter()
            .next()
            .map(|a| a.account_number)
            .ok_or_else(|| AppError::NotFound("no account found on this login".into()))
    }

    /// The bill-history rows, which nest under `data.data` or sit in `data`.
    fn bill_rows(&self, account: &str) -> Result<Vec<Value>, AppError> {
        let history = self.portal.bill_history(account)?;
        history
            .pointer("/data/data")
            .and_then(Value::as_array)
            .or_else(|| history.get("data").and_then(Value::as_array))
            .cloned()
            .ok_or_else(|| shape_error("bill history"))
    }
}

/// A body with no list where one belongs. An empty result here would read as
/// "no bills" when the truth is that FPL changed its answer.
fn shape_error(what: &str) -> AppError {
    AppError::Upstream(format!("FPL's {what} did not have the expected shape"))
}

/// Rows FPL sent, none of which could be read, mean its answer changed. Passing
/// that on as an empty list would read as "no bills".
fn none_read_is_an_error<R, T>(rows: &[R], read: &[T], what: &str) -> Result<(), AppError> {
    if !rows.is_empty() && read.is_empty() {
        return Err(shape_error(what));
    }
    Ok(())
}

impl<P: Portal> UtilitySource for FplSource<P> {
    async fn accounts(&self) -> Result<Vec<UtilityAccount>, AppError> {
        Ok(self
            .portal
            .accounts()?
            .into_iter()
            .map(|a| UtilityAccount {
                id: a.account_number,
                name: a.address,
            })
            .collect())
    }

    async fn summary(&self, account: Option<&str>) -> Result<UtilitySummary, AppError> {
        let account = self.account(account)?;
        let detail = self.portal.account_detail(&account)?;
        if output::balance_of(&detail).is_none() {
            return Err(shape_error("account detail"));
        }
        Ok(output::utility_summary_dto(&detail, &account))
    }

    async fn statements(&self, account: Option<&str>) -> Result<Vec<Statement>, AppError> {
        let account = self.account(account)?;
        let rows = self.bill_rows(&account)?;
        let mut statements: Vec<Statement> = rows
            .iter()
            .filter(|r| output::states_an_amount(r))
            .filter_map(output::statement_of)
            .collect();
        none_read_is_an_error(&rows, &statements, "bill history")?;
        statements.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(statements)
    }

    async fn payments(
        &self,
        account: Option<&str>,
        _since: Option<&str>,
    ) -> Result<Vec<Payment>, AppError> {
        let account = self.account(account)?;
        let ledger = self.portal.account_history(&account)?;
        let rows = ledger
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| shape_error("account history"))?;
        let payment_rows: Vec<&Value> = rows.iter().filter(|r| output::is_payment_row(r)).collect();
        let mut payments: Vec<Payment> = payment_rows
            .iter()
            .filter_map(|r| output::payment_of(r))
            .collect();
        none_read_is_an_error(&payment_rows, &payments, "account history")?;
        payments.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(payments)
    }

    async fn usage(&self, account: Option<&str>) -> Result<Vec<UsagePeriod>, AppError> {
        let account = self.account(account)?;
        let rows = self.bill_rows(&account)?;
        let mut usage: Vec<UsagePeriod> = rows.iter().filter_map(output::usage_of).collect();
        none_read_is_an_error(&rows, &usage, "bill history")?;
        usage.sort_by(|a, b| b.period.cmp(&a.period));
        Ok(usage)
    }
}

#[cfg(test)]
mod tests;
