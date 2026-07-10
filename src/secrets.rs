//! Secret handling for fpl-cli.
//!
//! Runtime secrets live only in the OS keychain. Getting a secret *into* the
//! keychain is a setup-time concern handled by `fpl init` / `fpl set-credential`,
//! which ingest via stdin or a named env var — never a `--value` flag (that
//! leaks into `ps`, shell history, and pasted transcripts).
//!
//! Secrets never appear in `Debug`/`Display` output and are zeroized on drop.

use std::fmt;
use std::io::Read;

use keyring::Entry;
use zeroize::Zeroize;

use crate::error::AppError;

/// Read exactly one secret from stdin (all of it, trailing newline trimmed).
/// The scriptable ingress path: `op read … | fpl set-credential --stdin`.
pub fn read_stdin() -> Result<Secret, AppError> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| AppError::Other(format!("reading secret from stdin: {e}")))?;
    // Trim a single trailing newline (and CR) so heredocs/echo pipes work.
    let trimmed = buf.strip_suffix('\n').unwrap_or(&buf);
    let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
    Ok(Secret::new(trimmed.to_string()))
}

/// Read one secret from a named environment variable (`--from-env FPL_PASSWORD`).
/// Bounded-scope ingress for `op run --`-style invocations.
pub fn read_from_env(var: &str) -> Result<Secret, AppError> {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => Ok(Secret::new(v)),
        Ok(_) => Err(AppError::Usage(format!("${var} is set but empty"))),
        Err(_) => Err(AppError::Usage(format!("${var} is not set"))),
    }
}

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

    /// No-echo interactive prompt. Caller must have already confirmed a TTY.
    pub fn prompt(label: &str) -> Result<Secret, AppError> {
        let v = rpassword::prompt_password(format!("{label}: "))
            .map_err(|e| AppError::Other(format!("reading password: {e}")))?;
        Ok(Secret::new(v))
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

/// OS-keychain-backed credential store. The only runtime source of secrets.
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
}
