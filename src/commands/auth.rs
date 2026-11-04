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

    let removed_with_name = match ctx
        .cli
        .username
        .clone()
        .or_else(|| ctx.cfg.username.clone())
    {
        Some(u) => Some(crate::commands::delete_credential(&u)?),
        None => None,
    };
    // A login can be gone while its pieces are not: an older build's `--forget` clears the name.
    let named = removed_with_name.is_some();
    let removed = removed_with_name.unwrap_or(false);
    let outcome = forget.then(|| {
        (
            CredentialStore::for_binary("fpl").delete(pk_cli_mcp::HTTP_TOKEN_ACCOUNT),
            Config::clear(),
        )
    });
    if !ctx.cli.quiet {
        eprintln!(
            "logged out{}",
            if removed {
                " and cleared stored password"
            } else if named {
                ""
            } else {
                "; no username is known, so no stored password was looked for (--username names one)"
            }
        );
    }
    match outcome {
        Some((token, config)) => finish_forget(token, config, ctx.cli.quiet),
        None => Ok(()),
    }
}

/// The end of `auth logout --forget`. Nothing records whether `mcp --new-token` ran, so the token's
/// delete is ungated, and it and the config's are both attempted and both reported here: neither
/// failure hides the other, or the note that a removed token still needs.
fn finish_forget(
    token: Result<bool, AppError>,
    config: Result<bool, AppError>,
    quiet: bool,
) -> Result<(), AppError> {
    if matches!(token, Ok(true)) && !quiet {
        eprintln!("{}", pk_cli_mcp::stop_note("fpl"));
    }
    match (token, config) {
        (Err(failure), config) => Err(AppError::Keychain(format!(
            "logged out, but the connector token of `fpl mcp --http` could not be removed ({}) and would \
             still open the connector{}; run `fpl auth logout --forget` again",
            failure.message(),
            config
                .err()
                .map(|e| format!(", and the config was not cleared ({})", e.message()))
                .unwrap_or_default(),
        ))),
        (Ok(_), Err(e)) => Err(e),
        (Ok(_), Ok(_)) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;

    #[test]
    fn a_forget_that_could_not_remove_the_connector_token_says_so() {
        let denied = || AppError::Keychain("denied".into());
        let says = super::finish_forget(Err(denied()), Ok(true), true)
            .unwrap_err()
            .to_string();
        assert!(says.contains("connector token"), "{says}");
        assert!(says.contains("`fpl auth logout --forget`"), "{says}");
        assert!(says.contains("denied"), "{says}");
        assert!(!says.contains("config"), "{says}");
    }

    #[test]
    fn neither_failure_of_a_forget_hides_the_other() {
        let says = super::finish_forget(
            Err(AppError::Keychain("denied".into())),
            Err(AppError::Other("read-only".into())),
            true,
        )
        .unwrap_err()
        .to_string();
        assert!(
            says.contains("denied") && says.contains("read-only"),
            "{says}"
        );
        let e = super::finish_forget(Ok(true), Err(AppError::Other("read-only".into())), true)
            .unwrap_err();
        assert!(e.to_string().contains("read-only"), "{e}");
    }

    #[test]
    fn a_forget_with_a_token_or_without_one_finishes() {
        assert!(super::finish_forget(Ok(true), Ok(true), true).is_ok());
        assert!(super::finish_forget(Ok(false), Ok(false), false).is_ok());
    }
}
