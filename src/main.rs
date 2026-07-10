mod cli;
mod client;
mod config;
mod dates;
mod error;
mod output;
mod secrets;

use std::io::{IsTerminal, Write};

use clap::Parser;
use serde_json::{json, Value};

use cli::{BillCommand, Cli, Command, HistoryCommand, PayCommand, UsageCommand};
use client::Fpl;
use config::Config;
use error::AppError;
use secrets::{CredentialStore, Secret};

/// Keychain service name. One password entry per FPL username.
const SERVICE: &str = "fpl-cli";

fn main() {
    let cli = Cli::parse();
    let quiet = cli.quiet;
    if let Err(e) = run(cli) {
        if !quiet {
            eprintln!("error: {e}");
        }
        std::process::exit(e.exit_code());
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    let cfg = Config::load()?;
    match &cli.command {
        Command::Login { no_verify } => cmd_login(&cli, &cfg, *no_verify),
        Command::Logout { forget } => cmd_logout(&cli, &cfg, *forget),
        Command::Status => cmd_status(&cli, &cfg),
        Command::Outages { county } => cmd_outages(&cli, county.as_deref()),

        Command::Accounts => {
            let fpl = connect(&cli, &cfg)?;
            let list = fpl.accounts()?;
            output::accounts(&list, cli.json);
            Ok(())
        }
        Command::Account => {
            let (fpl, account) = connect_acct(&cli, &cfg)?;
            let detail = fpl.account_detail(&account)?;
            output::value(&detail, cli.json);
            Ok(())
        }
        Command::Balance => {
            let (fpl, account) = connect_acct(&cli, &cfg)?;
            let bal = fpl.balance(&account)?;
            output::balance(&bal, cli.json);
            Ok(())
        }

        Command::Bill(sub) => cmd_bill(&cli, &cfg, sub),
        Command::Pay(sub) => cmd_pay(&cli, &cfg, sub),
        Command::Usage(sub) => cmd_usage(&cli, &cfg, sub),
        Command::History(sub) => cmd_history(&cli, &cfg, sub),

        Command::Api { method, path, data } => cmd_api(&cli, &cfg, method, path, data.as_deref()),
    }
}

// ---- auth / session -------------------------------------------------------

fn resolve_username(cli: &Cli, cfg: &Config) -> Result<String, AppError> {
    if let Some(u) = cli.username.clone().filter(|s| !s.is_empty()) {
        return Ok(u);
    }
    if let Some(u) = cfg.username.clone().filter(|s| !s.is_empty()) {
        return Ok(u);
    }
    Err(AppError::Auth(
        "no FPL username configured — run `fpl login` (or pass --username / set $FPL_USERNAME)"
            .into(),
    ))
}

/// Log in and return an authenticated session.
fn connect(cli: &Cli, cfg: &Config) -> Result<Fpl, AppError> {
    let username = resolve_username(cli, cfg)?;
    let store = CredentialStore::new(SERVICE);
    let (password, source) = store.resolve(
        &username,
        "FPL_PASSWORD",
        &format!("FPL password for {username}"),
        true,
    )?;
    if cli.verbose && !cli.quiet {
        eprintln!("logging in to FPL as {username} (password from {source})");
    }
    Fpl::login(&username, &password)
}

/// Log in and resolve which account to act on.
fn connect_acct(cli: &Cli, cfg: &Config) -> Result<(Fpl, String), AppError> {
    let fpl = connect(cli, cfg)?;
    let account = resolve_account(cli, cfg, &fpl)?;
    if cli.verbose && !cli.quiet {
        eprintln!("using account {account}");
    }
    Ok((fpl, account))
}

fn resolve_account(cli: &Cli, cfg: &Config, fpl: &Fpl) -> Result<String, AppError> {
    if let Some(a) = cli.account.clone().filter(|s| !s.is_empty()) {
        return Ok(a);
    }
    if let Some(a) = cfg.account.clone().filter(|s| !s.is_empty()) {
        return Ok(a);
    }
    let list = fpl.accounts()?;
    list.into_iter()
        .next()
        .map(|a| a.account_number)
        .ok_or_else(|| {
            AppError::NotFound(
                "no account found on this login — pass --account or set $FPL_ACCOUNT".into(),
            )
        })
}

// ---- login / logout / status ----------------------------------------------

fn cmd_login(cli: &Cli, cfg: &Config, no_verify: bool) -> Result<(), AppError> {
    let username = match cli.username.clone().or_else(|| cfg.username.clone()) {
        Some(u) if !u.is_empty() => u,
        _ => prompt_line("FPL username (email)")?,
    };

    let password = match std::env::var("FPL_PASSWORD") {
        Ok(p) if !p.is_empty() => Secret::new(p),
        _ => {
            if !std::io::stdin().is_terminal() {
                return Err(AppError::Usage(
                    "no password: set $FPL_PASSWORD or run `fpl login` in an interactive terminal"
                        .into(),
                ));
            }
            Secret::new(
                rpassword::prompt_password(format!("FPL password for {username}: "))
                    .map_err(|e| AppError::Other(format!("reading password: {e}")))?,
            )
        }
    };
    if password.is_empty() {
        return Err(AppError::Usage("empty password — nothing stored".into()));
    }

    let store = CredentialStore::new(SERVICE);
    store.set(&username, &password)?;

    let mut new_cfg = Config::load()?;
    new_cfg.username = Some(username.clone());

    if no_verify {
        new_cfg.save()?;
        if !cli.quiet {
            eprintln!("stored credentials for {username} in the keychain (not verified)");
        }
        if cli.json {
            output::json(&json!({"status": "stored", "username": username, "verified": false}));
        }
        return Ok(());
    }

    // Verify by logging in, and remember the default account if there's just one.
    let fpl = Fpl::login(&username, &password)?;
    if new_cfg.account.is_none() {
        if let Ok(list) = fpl.accounts() {
            if let Some(first) = list.first() {
                new_cfg.account = Some(first.account_number.clone());
            }
        }
    }
    new_cfg.save()?;

    if !cli.quiet {
        match &new_cfg.account {
            Some(a) => eprintln!("logged in as {username}; default account {a}"),
            None => eprintln!("logged in as {username}"),
        }
    }
    if cli.json {
        output::json(&json!({
            "status": "ok",
            "username": username,
            "account": new_cfg.account,
            "verified": true,
        }));
    }
    Ok(())
}

fn cmd_logout(cli: &Cli, cfg: &Config, forget: bool) -> Result<(), AppError> {
    // Best-effort server-side logout if we can build a session.
    if let Ok(fpl) = connect(cli, cfg) {
        let _ = fpl.logout();
    }

    let store = CredentialStore::new(SERVICE);
    let mut removed = false;
    if let Some(u) = cli.username.clone().or_else(|| cfg.username.clone()) {
        removed = store.delete(&u)?;
    }
    if forget {
        Config::clear()?;
    }
    if !cli.quiet {
        eprintln!(
            "logged out{}",
            if removed {
                " and cleared stored password"
            } else {
                ""
            }
        );
    }
    if cli.json {
        output::json(
            &json!({"status": "ok", "password_removed": removed, "forgot_config": forget}),
        );
    }
    Ok(())
}

fn cmd_status(cli: &Cli, cfg: &Config) -> Result<(), AppError> {
    let username = cli.username.clone().or_else(|| cfg.username.clone());
    let store = CredentialStore::new(SERVICE);
    let has_password = match &username {
        Some(u) => store.get(u)?.is_some(),
        None => false,
    };
    let account = cli.account.clone().or_else(|| cfg.account.clone());

    if cli.json {
        output::json(&json!({
            "username": username,
            "account": account,
            "password_in_keychain": has_password,
        }));
    } else {
        println!("username:  {}", username.as_deref().unwrap_or("(unset)"));
        println!("account:   {}", account.as_deref().unwrap_or("(unset)"));
        println!(
            "password:  {}",
            if has_password {
                "stored in keychain"
            } else {
                "not stored"
            }
        );
    }
    Ok(())
}

// ---- billing --------------------------------------------------------------

/// Premise, meter, and last-billed date (`MMDDYYYY`) pulled from account detail.
struct AcctCtx {
    premise: String,
    meter: String,
    last_billed: String,
}

fn account_ctx(fpl: &Fpl, account: &str) -> Result<AcctCtx, AppError> {
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

fn cmd_bill(cli: &Cli, cfg: &Config, sub: &BillCommand) -> Result<(), AppError> {
    let (fpl, account) = connect_acct(cli, cfg)?;
    match sub {
        BillCommand::Current => {
            let ctx = account_ctx(&fpl, &account)?;
            let v = fpl.energy_usage(&account, &ctx.premise, &ctx.last_billed, &ctx.meter)?;
            output::value(&v, cli.json);
        }
        BillCommand::Projected => {
            let ctx = account_ctx(&fpl, &account)?;
            let v = fpl.projected_bill(&account, &ctx.premise, &ctx.last_billed)?;
            output::value(&v, cli.json);
        }
        BillCommand::History => {
            let v = fpl.bill_history(&account)?;
            output::value(&v, cli.json);
        }
        BillCommand::Budget => {
            let v = fpl.budget_billing(&account)?;
            output::value(&v, cli.json);
        }
        BillCommand::Download { out } => {
            let bytes = fpl.download_bill(&account)?;
            std::fs::write(out, &bytes)
                .map_err(|e| AppError::Other(format!("writing {out}: {e}")))?;
            if !cli.quiet {
                eprintln!("wrote {} bytes to {out}", bytes.len());
            }
            if cli.json {
                output::json(&json!({"status": "ok", "path": out, "bytes": bytes.len()}));
            }
        }
    }
    Ok(())
}

// ---- payments -------------------------------------------------------------

fn cmd_pay(cli: &Cli, cfg: &Config, sub: &PayCommand) -> Result<(), AppError> {
    let (fpl, account) = connect_acct(cli, cfg)?;
    match sub {
        PayCommand::Methods => {
            let v = fpl.payment_options(&account)?;
            output::value(&v, cli.json);
        }
        PayCommand::History => {
            let v = fpl.account_history(&account)?;
            output::value(&v, cli.json);
        }
        PayCommand::Make {
            amount,
            date,
            method,
            yes,
        } => {
            let pay_date = date
                .clone()
                .unwrap_or_else(|| dates::fmt_mm_dd_yyyy(dates::today()));

            // Money movement is hard to reverse: confirm unless --yes.
            if !yes {
                if !std::io::stdin().is_terminal() {
                    return Err(AppError::Usage(
                        "refusing to submit a payment without confirmation — re-run with --yes"
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

            let mut body = json!({
                "paymentAmount": amount,
                "paymentDate": pay_date,
            });
            if let Some(m) = method {
                body["paymentMethod"] = Value::String(m.clone());
            }
            let v = fpl.make_payment(&account, &body)?;
            output::value(&v, cli.json);
        }
    }
    Ok(())
}

// ---- usage ----------------------------------------------------------------

fn cmd_usage(cli: &Cli, cfg: &Config, sub: &UsageCommand) -> Result<(), AppError> {
    let (fpl, account) = connect_acct(cli, cfg)?;
    match sub {
        UsageCommand::Summary => {
            let ctx = account_ctx(&fpl, &account)?;
            let v = fpl.energy_usage(&account, &ctx.premise, &ctx.last_billed, &ctx.meter)?;
            output::value(&v, cli.json);
        }
        UsageCommand::Hourly { date } => {
            let ctx = account_ctx(&fpl, &account)?;
            let day = date
                .clone()
                .unwrap_or_else(|| dates::fmt_mm_dd_yyyy(dates::yesterday()));
            let v = fpl.hourly_usage(&account, &ctx.premise, &day)?;
            output::value(&v, cli.json);
        }
        UsageCommand::Appliances => {
            let ctx = account_ctx(&fpl, &account)?;
            let v = fpl.appliance_usage(&account, &ctx.premise)?;
            output::value(&v, cli.json);
        }
    }
    Ok(())
}

// ---- history --------------------------------------------------------------

fn cmd_history(cli: &Cli, cfg: &Config, sub: &HistoryCommand) -> Result<(), AppError> {
    let (fpl, account) = connect_acct(cli, cfg)?;
    let v = match sub {
        HistoryCommand::Account => fpl.account_history(&account)?,
        HistoryCommand::Deposit => fpl.deposit_history(&account)?,
        HistoryCommand::Documents => fpl.document_history(&account)?,
    };
    output::value(&v, cli.json);
    Ok(())
}

// ---- outages / raw --------------------------------------------------------

fn cmd_outages(cli: &Cli, county: Option<&str>) -> Result<(), AppError> {
    let v = client::county_outages()?;
    output::outages(&v, county, cli.json);
    Ok(())
}

fn cmd_api(
    cli: &Cli,
    cfg: &Config,
    method: &str,
    path: &str,
    data: Option<&str>,
) -> Result<(), AppError> {
    let body: Option<Value> = match data {
        Some(s) => Some(
            serde_json::from_str(s)
                .map_err(|e| AppError::Usage(format!("--data is not valid JSON: {e}")))?,
        ),
        None => None,
    };
    let fpl = connect(cli, cfg)?;
    let v = fpl.request(method, path, body.as_ref())?;
    output::json(&v);
    Ok(())
}

// ---- small helpers --------------------------------------------------------

fn detail_str(detail: &Value, key: &str) -> Option<String> {
    let v = detail.pointer(&format!("/data/{key}"))?;
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// FPL bill dates arrive as `YYYY-MM-DD...` or `YYYYMMDD`; the usage and
/// projected-bill endpoints want `MMDDYYYY`.
fn mmddyyyy_from_raw(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    let ymd = &digits[..8];
    Some(format!("{}{}{}", &ymd[4..6], &ymd[6..8], &ymd[0..4]))
}

fn prompt_line(label: &str) -> Result<String, AppError> {
    if !std::io::stdin().is_terminal() {
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

fn confirm(prompt: &str) -> Result<bool, AppError> {
    eprint!("{prompt}");
    std::io::stderr().flush().ok();
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| AppError::Other(format!("reading input: {e}")))?;
    Ok(matches!(s.trim().to_lowercase().as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
