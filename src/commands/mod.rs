//! Command handlers, one module per resource group. Shared session/account
//! resolution and prompt helpers live here on [`Ctx`].

pub mod accounts;
pub mod api;
pub mod auth;
pub mod bills;
pub mod history;
pub mod init;
pub mod outages;
pub mod payments;
pub mod set_credential;
pub mod usage;

use std::io::{IsTerminal, Write};

use serde_json::Value;

use crate::cli::Cli;
use crate::client::Fpl;
use crate::config::Config;
use crate::error::AppError;
use crate::secrets::CredentialStore;

/// Keychain service name. One password entry per FPL username.
pub const SERVICE: &str = "fpl-cli";

/// Per-invocation context threaded to every command handler.
pub struct Ctx<'a> {
    pub cli: &'a Cli,
    pub cfg: &'a Config,
}

impl Ctx<'_> {
    pub fn resolve_username(&self) -> Result<String, AppError> {
        if let Some(u) = self.cli.username.clone().filter(|s| !s.is_empty()) {
            return Ok(u);
        }
        if let Some(u) = self.cfg.username.clone().filter(|s| !s.is_empty()) {
            return Ok(u);
        }
        Err(AppError::Auth(
            "no FPL username configured — run `fpl init` (or pass --username / set $FPL_USERNAME)"
                .into(),
        ))
    }

    /// Open an authenticated session. Runtime secrets come only from the
    /// keychain; `fpl init` / `fpl set-credential` are how they get there.
    pub fn connect(&self) -> Result<Fpl, AppError> {
        let username = self.resolve_username()?;
        let store = CredentialStore::new(SERVICE);
        let secret = store.get(&username)?.ok_or_else(|| {
            AppError::Auth(format!(
                "no stored password for {username:?} — run `fpl init` or \
                 `fpl set-credential --stdin`"
            ))
        })?;
        if self.cli.verbose && !self.cli.quiet {
            eprintln!("logging in to FPL as {username}");
        }
        Fpl::login(&username, &secret)
    }

    /// Resolve the account to act on: explicit positional, then global
    /// `--account`, then the active account in config, then the first account.
    pub fn resolve_account(&self, positional: Option<&str>, fpl: &Fpl) -> Result<String, AppError> {
        if let Some(a) = positional.filter(|s| !s.is_empty()) {
            return Ok(a.to_string());
        }
        if let Some(a) = self.cli.account.clone().filter(|s| !s.is_empty()) {
            return Ok(a);
        }
        if let Some(a) = self.cfg.account.clone().filter(|s| !s.is_empty()) {
            return Ok(a);
        }
        let list = fpl.accounts()?;
        list.into_iter()
            .next()
            .map(|a| a.account_number)
            .ok_or_else(|| {
                AppError::NotFound(
                    "no account found on this login — pass an account id or --account".into(),
                )
            })
    }
}

/// Premise, meter, and last-billed date (`MMDDYYYY`) from account detail —
/// needed by the usage and projected-bill endpoints.
pub struct AcctCtx {
    pub premise: String,
    pub meter: String,
    pub last_billed: String,
}

pub fn account_ctx(fpl: &Fpl, account: &str) -> Result<AcctCtx, AppError> {
    let detail = fpl.account_detail(account)?;
    let premise = Fpl::premise_of(&detail).ok_or_else(|| {
        AppError::NotFound("could not read the premise number from account detail".into())
    })?;
    let meter = detail_str(&detail, "meterNo").unwrap_or_default();
    let last_billed = detail_str(&detail, "currentBillDate")
        .and_then(|raw| mmddyyyy_from_raw(&raw))
        .ok_or_else(|| {
            AppError::NotFound("could not read the current bill date from account detail".into())
        })?;
    Ok(AcctCtx {
        premise,
        meter,
        last_billed,
    })
}

pub fn detail_str(detail: &Value, key: &str) -> Option<String> {
    let v = detail.pointer(&format!("/data/{key}"))?;
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// FPL bill dates arrive as `YYYY-MM-DD…` or `YYYYMMDD`; the usage and
/// projected-bill endpoints want `MMDDYYYY`.
pub fn mmddyyyy_from_raw(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    let ymd = &digits[..8];
    Some(format!("{}{}{}", &ymd[4..6], &ymd[6..8], &ymd[0..4]))
}

/// Prompt for one line on a TTY (non-secret input, e.g. a username).
pub fn prompt_line(label: &str) -> Result<String, AppError> {
    if !stdin_is_tty() {
        return Err(AppError::Usage(format!(
            "{label} is required (run interactively or pass it as a flag)"
        )));
    }
    eprint!("{label}: ");
    std::io::stderr().flush().ok();
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| AppError::Other(format!("reading input: {e}")))?;
    let s = s.trim().to_string();
    if s.is_empty() {
        return Err(AppError::Usage(format!("{label} cannot be empty")));
    }
    Ok(s)
}

/// One-shot `y/N` safety confirmation. Reads from stdin.
pub fn confirm(prompt: &str) -> Result<bool, AppError> {
    eprint!("{prompt}");
    std::io::stderr().flush().ok();
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| AppError::Other(format!("reading input: {e}")))?;
    Ok(matches!(s.trim().to_lowercase().as_str(), "y" | "yes"))
}

pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mmddyyyy_reformats() {
        assert_eq!(
            mmddyyyy_from_raw("2024-06-15T00:00:00").as_deref(),
            Some("06152024")
        );
        assert_eq!(mmddyyyy_from_raw("20240615").as_deref(), Some("06152024"));
        assert_eq!(mmddyyyy_from_raw("2024").as_deref(), None);
    }

    #[test]
    fn detail_reads_nested_fields() {
        let d = json!({ "data": { "meterNo": "ABC123", "premiseNumber": 42 } });
        assert_eq!(detail_str(&d, "meterNo").as_deref(), Some("ABC123"));
        assert_eq!(detail_str(&d, "premiseNumber").as_deref(), Some("42"));
        assert_eq!(detail_str(&d, "missing"), None);
    }
}
