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

    let name = ctx
        .cli
        .username
        .clone()
        .or_else(|| ctx.cfg.username.clone());
    let named = name.is_some();
    let session = match &name {
        Some(u) => crate::commands::delete_credential(u),
        None => Ok(false),
    };
    if !forget {
        say_logged_out(session?, named, ctx.cli.quiet);
        return Ok(());
    }
    let token = CredentialStore::for_binary("fpl").delete(pk_cli_mcp::HTTP_TOKEN_ACCOUNT);
    // A retry needs the username to find the password, so a failed delete keeps the config.
    let config = if session.is_ok() {
        Config::clear()
    } else {
        Ok(false)
    };
    if let Ok(removed) = &session {
        say_logged_out(*removed, named, ctx.cli.quiet);
    }
    finish_forget(session.map(drop), token, config, ctx.cli.quiet)
}

/// A login can be gone while its pieces are not: an older build's `--forget` clears the name.
fn say_logged_out(removed: bool, named: bool, quiet: bool) {
    if !quiet {
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
}

/// The end of `auth logout --forget`. Nothing records whether `mcp --new-token` ran, so the token's
/// delete is ungated. Every step is attempted and every failure reported here: none hides another,
/// or the note that a removed token still needs, since a rerun finds no item.
fn finish_forget(
    session: Result<(), AppError>,
    token: Result<bool, AppError>,
    config: Result<bool, AppError>,
    quiet: bool,
) -> Result<(), AppError> {
    if matches!(token, Ok(true)) && !quiet {
        eprintln!("{}", pk_cli_mcp::stop_note("fpl"));
    }
    match (session, token, config) {
        (Ok(()), Ok(_), Ok(_)) => Ok(()),
        (Ok(()), Ok(_), Err(e)) => Err(e),
        (session, token, config) => {
            let opening = if session.is_ok() {
                "logged out, but "
            } else {
                ""
            };
            let mut failed = Vec::new();
            if let Err(e) = session {
                failed.push(format!(
                    "the stored password could not be removed ({})",
                    e.message()
                ));
            }
            if let Err(e) = token {
                failed.push(format!(
                    "the connector token of `fpl mcp --http` could not be removed ({}) and would \
                     still open the connector",
                    e.message()
                ));
            }
            if let Err(e) = config {
                failed.push(format!("the config was not cleared ({})", e.message()));
            }
            Err(AppError::Keychain(format!(
                "{opening}{}; run `fpl auth logout --forget` again",
                failed.join(", and ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;

    #[test]
    fn a_forget_that_could_not_remove_the_connector_token_says_so() {
        let denied = || AppError::Keychain("denied".into());
        let says = super::finish_forget(Ok(()), Err(denied()), Ok(true), true)
            .unwrap_err()
            .to_string();
        assert!(says.contains("logged out, but"), "{says}");
        assert!(says.contains("connector token"), "{says}");
        assert!(says.contains("`fpl auth logout --forget`"), "{says}");
        assert!(says.contains("denied"), "{says}");
        assert!(!says.contains("config"), "{says}");
    }

    #[test]
    fn no_failure_of_a_forget_hides_another() {
        let says = super::finish_forget(
            Err(AppError::Keychain("locked".into())),
            Err(AppError::Keychain("denied".into())),
            Err(AppError::Other("read-only".into())),
            true,
        )
        .unwrap_err()
        .to_string();
        for cause in ["locked", "denied", "read-only"] {
            assert!(says.contains(cause), "{says}");
        }
        assert!(!says.contains("logged out, but"), "{says}");
        let e = super::finish_forget(
            Ok(()),
            Ok(true),
            Err(AppError::Other("read-only".into())),
            true,
        )
        .unwrap_err();
        assert!(e.to_string().contains("read-only"), "{e}");
    }

    #[test]
    fn a_forget_with_a_token_or_without_one_finishes() {
        assert!(super::finish_forget(Ok(()), Ok(true), Ok(true), true).is_ok());
        assert!(super::finish_forget(Ok(()), Ok(false), Ok(false), false).is_ok());
    }
}
