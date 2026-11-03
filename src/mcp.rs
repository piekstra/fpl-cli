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
    with_source(source)
}

fn with_source<P: Portal>(source: FplSource<P>) -> UtilityTools<FplSource<P>> {
    let server = Server::new("fpl", env!("CARGO_PKG_VERSION"), "FPL", "fpl").instructions(
        "FPL access here is read-only: no tool can make a payment or change anything. Payments \
         come from FPL's 25 most recent ledger entries, so an older payment is not listed. Usage \
         is the kWh on each bill. A bill's paid state is not reported.",
    );
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
    state: Mutex<Session>,
}

#[derive(Default)]
struct Session {
    fpl: Option<Fpl>,
    /// The password FPL last refused. Not sent again, so a server left running
    /// after a password change cannot lock the account by retrying it: the
    /// next call that finds a different one tries that.
    refused: Option<Secret>,
}

impl LivePortal {
    fn new(username: Option<String>) -> Self {
        Self {
            username: username.filter(|u| !u.is_empty()),
            state: Mutex::default(),
        }
    }

    fn sign_in(&self, session: &mut Session) -> Result<(), AppError> {
        let username = self
            .username
            .as_deref()
            .ok_or_else(|| AppError::Auth("no FPL username is configured".into()))?;
        let password = password_from(std::env::var(PASSWORD_ENV).ok(), || read_keychain(username))?
            .ok_or_else(|| AppError::Auth(format!("no stored password for {username:?}")))?;
        let refused = session.refused.as_ref();
        if refused.is_some_and(|r| r.expose() == password.expose()) {
            return Err(AppError::Auth(
                "FPL refused this password on an earlier call, so it was not sent again".into(),
            ));
        }
        match Fpl::login(username, &password) {
            Ok(fpl) => {
                *session = Session {
                    fpl: Some(fpl),
                    refused: None,
                };
                Ok(())
            }
            Err(e) => {
                if matches!(e, AppError::Auth(_)) {
                    session.refused = Some(password);
                }
                Err(e)
            }
        }
    }

    /// One read on the kept session, signing in when there is none and once
    /// more when FPL says the session has lapsed.
    fn read<T>(&self, op: impl Fn(&Fpl) -> Result<T, AppError>) -> Result<T, AppError> {
        let mut session = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if session.fpl.is_none() {
            self.sign_in(&mut session)?;
        }
        let fpl = session.fpl.as_ref().expect("signed in above");
        match op(fpl) {
            Err(AppError::Auth(_)) => {
                session.fpl = None;
                self.sign_in(&mut session)?;
                op(session.fpl.as_ref().expect("signed in above"))
            }
            other => other,
        }
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
        Ok(output::utility_summary_dto(&detail, &account))
    }

    async fn statements(&self, account: Option<&str>) -> Result<Vec<Statement>, AppError> {
        let account = self.account(account)?;
        let mut rows: Vec<Statement> = self
            .bill_rows(&account)?
            .iter()
            .filter_map(output::statement_of)
            .collect();
        rows.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(rows)
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
        let mut rows: Vec<Payment> = rows
            .iter()
            .filter(|r| output::is_payment_row(r))
            .filter_map(output::payment_of)
            .collect();
        rows.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(rows)
    }

    async fn usage(&self, account: Option<&str>) -> Result<Vec<UsagePeriod>, AppError> {
        let account = self.account(account)?;
        let mut rows: Vec<UsagePeriod> = self
            .bill_rows(&account)?
            .iter()
            .filter_map(output::usage_of)
            .collect();
        rows.sort_by(|a, b| b.period.cmp(&a.period));
        Ok(rows)
    }
}

#[cfg(test)]
mod tests;
