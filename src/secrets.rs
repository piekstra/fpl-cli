//! Secret handling for fpl-cli.
//!
//! Precedence: **OS keychain → environment variable → interactive prompt**.
//! Secrets never appear in `Debug`/`Display` output and are zeroized on drop.

use std::fmt;
use std::io::IsTerminal;

use keyring::Entry;
use zeroize::Zeroize;

use crate::error::AppError;

/// A secret string that refuses to reveal itself via `Debug`/`Display` and is
/// zeroized from memory when dropped. Read it only at the point of use, with
/// [`Secret::expose`], and never log the result.
pub struct Secret {
    inner: String,
}

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret {
            inner: value.into(),
        }
    }

    /// Borrow the underlying secret. Use at the call site only — never log it.
    pub fn expose(&self) -> &str {
        &self.inner
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***redacted***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***redacted***")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.inner.zeroize();
    }
}

/// Where a resolved credential was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Keychain,
    Env(String),
    Prompt,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Keychain => write!(f, "keychain"),
            Source::Env(var) => write!(f, "env:{var}"),
            Source::Prompt => write!(f, "prompt"),
        }
    }
}

/// OS-keychain-backed credential store with env + prompt fallbacks.
pub struct CredentialStore {
    service: String,
}

impl CredentialStore {
    pub fn new(service: impl Into<String>) -> Self {
        CredentialStore {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str) -> Result<Entry, AppError> {
        Entry::new(&self.service, account)
            .map_err(|e| AppError::Keychain(format!("opening keychain entry: {e}")))
    }

    /// Keychain only. `None` if no entry exists.
    pub fn get(&self, account: &str) -> Result<Option<Secret>, AppError> {
        match self.entry(account)?.get_password() {
            Ok(p) => Ok(Some(Secret::new(p))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Keychain(format!("reading credential: {e}"))),
        }
    }

    /// Store (or overwrite) a credential in the keychain.
    pub fn set(&self, account: &str, secret: &Secret) -> Result<(), AppError> {
        self.entry(account)?
            .set_password(secret.expose())
            .map_err(|e| AppError::Keychain(format!("storing credential: {e}")))
    }

    /// Delete a credential. Returns `true` if something was removed, `false` if
    /// there was nothing stored.
    pub fn delete(&self, account: &str) -> Result<bool, AppError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(AppError::Keychain(format!("deleting credential: {e}"))),
        }
    }

    /// Full precedence resolution: keychain, then `$env_var`, then (if a TTY and
    /// `allow_prompt`) a no-echo prompt. Errors with [`AppError::Auth`] if none
    /// resolve.
    pub fn resolve(
        &self,
        account: &str,
        env_var: &str,
        prompt_label: &str,
        allow_prompt: bool,
    ) -> Result<(Secret, Source), AppError> {
        if let Some(s) = self.get(account)? {
            return Ok((s, Source::Keychain));
        }
        if let Ok(v) = std::env::var(env_var) {
            if !v.is_empty() {
                return Ok((Secret::new(v), Source::Env(env_var.to_string())));
            }
        }
        if allow_prompt && std::io::stdin().is_terminal() {
            let v = rpassword::prompt_password(format!("{prompt_label}: "))
                .map_err(|e| AppError::Other(format!("reading prompt: {e}")))?;
            return Ok((Secret::new(v), Source::Prompt));
        }
        Err(AppError::Auth(format!(
            "no password for {account:?} — run `fpl login --username {account}` or set ${env_var}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_redacts_but_exposes_on_demand() {
        let s = Secret::new("super-secret-token");
        assert_eq!(format!("{s}"), "***redacted***");
        assert_eq!(format!("{s:?}"), "Secret(***redacted***)");
        assert_eq!(s.expose(), "super-secret-token");
        assert!(!s.is_empty());
        assert!(Secret::new("").is_empty());
    }

    #[test]
    fn source_displays_human_readable() {
        assert_eq!(Source::Keychain.to_string(), "keychain");
        assert_eq!(
            Source::Env("FPL_PASSWORD".into()).to_string(),
            "env:FPL_PASSWORD"
        );
        assert_eq!(Source::Prompt.to_string(), "prompt");
    }
}
