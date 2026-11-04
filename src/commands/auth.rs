//! `fpl auth` — session and credential status.

use pk_cli_auth::{AuthMethod, AuthStatus};
use pk_cli_secrets::CredentialStore;

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
    let token =
        forget.then(|| CredentialStore::for_binary("fpl").delete(pk_cli_mcp::HTTP_TOKEN_ACCOUNT));
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
    match token {
        Some(token) => finish_forget(token, ctx.cli.quiet),
        None => Ok(()),
    }
}

/// The end of `auth logout --forget`. Nothing records whether `mcp --new-token` ran, so the delete
/// is ungated: a removed token gets the note about a running server, and one that could not be
/// removed is an error, since it still opens the connector.
fn finish_forget(token: Result<bool, AppError>, quiet: bool) -> Result<(), AppError> {
    let removed = token.map_err(|failure| {
        AppError::Keychain(format!(
            "the connector token of `fpl mcp --http` could not be removed, if there is one, and \
             would still open the connector: {}. Run `fpl auth logout --forget` again.",
            failure.message()
        ))
    })?;
    if removed && !quiet {
        eprintln!("{}", pk_cli_mcp::stop_note("fpl"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;

    #[test]
    fn a_forget_that_could_not_remove_the_connector_token_says_so() {
        let denied = AppError::Keychain("denied".into());
        let says = super::finish_forget(Err(denied), true)
            .unwrap_err()
            .to_string();
        assert!(says.contains("connector token"), "{says}");
        assert!(says.contains("`fpl auth logout --forget`"), "{says}");
        assert!(says.contains("denied"), "{says}");
    }

    #[test]
    fn a_forget_with_a_token_or_without_one_finishes() {
        assert!(super::finish_forget(Ok(true), true).is_ok());
        assert!(super::finish_forget(Ok(false), false).is_ok());
    }
}
