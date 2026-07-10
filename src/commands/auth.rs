//! `fpl auth` — session and credential status.

use serde_json::json;

use crate::cli::AuthCommand;
use crate::commands::{Ctx, SERVICE};
use crate::config::Config;
use crate::error::AppError;
use crate::output;
use crate::secrets::CredentialStore;

pub fn run(ctx: &Ctx, cmd: &AuthCommand) -> Result<(), AppError> {
    match cmd {
        AuthCommand::Status { json } => status(ctx, *json),
        AuthCommand::Logout { forget } => logout(ctx, *forget),
    }
}

fn status(ctx: &Ctx, json_flag: bool) -> Result<(), AppError> {
    let username = ctx
        .cli
        .username
        .clone()
        .or_else(|| ctx.cfg.username.clone());
    let store = CredentialStore::new(SERVICE);
    let has_password = match &username {
        Some(u) => store.get(u)?.is_some(),
        None => false,
    };
    let account = ctx.cli.account.clone().or_else(|| ctx.cfg.account.clone());

    if json_flag {
        output::json(&json!({
            "username": username,
            "account": account,
            "password_in_keychain": has_password,
        }));
    } else {
        println!("username: {}", username.as_deref().unwrap_or("(unset)"));
        println!("account:  {}", account.as_deref().unwrap_or("(unset)"));
        println!(
            "password: {}",
            if has_password {
                "stored in keychain"
            } else {
                "not stored"
            }
        );
    }
    Ok(())
}

fn logout(ctx: &Ctx, forget: bool) -> Result<(), AppError> {
    // Best-effort server-side logout if we can build a session.
    if let Ok(fpl) = ctx.connect() {
        let _ = fpl.logout();
    }

    let store = CredentialStore::new(SERVICE);
    let mut removed = false;
    if let Some(u) = ctx
        .cli
        .username
        .clone()
        .or_else(|| ctx.cfg.username.clone())
    {
        removed = store.delete(&u)?;
    }
    if forget {
        Config::clear()?;
    }
    if !ctx.cli.quiet {
        eprintln!(
            "logged out{}",
            if removed {
                " and cleared stored password"
            } else {
                ""
            }
        );
    }
    Ok(())
}
