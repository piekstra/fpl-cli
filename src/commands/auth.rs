//! `fpl auth` — session and credential status.

use pk_cli_auth::{AuthMethod, AuthStatus};

use crate::cli::AuthCommand;
use crate::commands::Ctx;
use crate::config::Config;
use crate::error::AppError;

pub fn run(ctx: &Ctx, cmd: &AuthCommand) -> Result<(), AppError> {
    match cmd {
        AuthCommand::Login(args) => crate::commands::init::run(ctx, args),
        AuthCommand::SetCredential(args) => crate::commands::set_credential::run(ctx, args),
        AuthCommand::Status { json } => status(ctx, *json || ctx.cli.json),
        AuthCommand::Logout { forget } => logout(ctx, *forget),
    }
}

fn status(ctx: &Ctx, json_flag: bool) -> Result<(), AppError> {
    let username = ctx
        .cli
        .username
        .clone()
        .or_else(|| ctx.cfg.username.clone());
    let has_password = match &username {
        Some(u) => crate::commands::get_credential_migrating(u)?.is_some(),
        None => false,
    };
    let account = ctx.cli.account.clone().or_else(|| ctx.cfg.account.clone());

    let mut st = AuthStatus::new(true, has_password, AuthMethod::Password);
    st.username = username;
    st.account = account;
    st.credential_in_keychain = Some(has_password);
    st.emit(json_flag);
    Ok(())
}

fn logout(ctx: &Ctx, forget: bool) -> Result<(), AppError> {
    // Best-effort server-side logout if we can build a session.
    if let Ok(fpl) = ctx.connect() {
        let _ = fpl.logout();
    }

    let mut removed = false;
    if let Some(u) = ctx
        .cli
        .username
        .clone()
        .or_else(|| ctx.cfg.username.clone())
    {
        removed = crate::commands::delete_credential(&u)?;
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
