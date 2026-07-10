//! On-disk config for fpl-cli. Stores only non-secret preferences (the default
//! FPL username and, optionally, a default account number) so day-to-day
//! commands don't need `--username`/`--account` every time. The password itself
//! never lands here — it lives in the OS keychain (see [`crate::secrets`]).

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Default FPL login (email). Not a secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Default FPL account number, if the user pinned one. Not a secret, but
    /// account-scoped, so we only write it when explicitly set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// `${XDG_CONFIG_HOME:-~/.config}/fpl-cli/config.json`.
fn config_path() -> Result<PathBuf, AppError> {
    let base = if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            PathBuf::from(x)
        } else {
            home_config()?
        }
    } else {
        home_config()?
    };
    Ok(base.join("fpl-cli").join("config.json"))
}

fn home_config() -> Result<PathBuf, AppError> {
    let home = std::env::var("HOME")
        .map_err(|_| AppError::Other("cannot locate home directory ($HOME unset)".into()))?;
    Ok(PathBuf::from(home).join(".config"))
}

impl Config {
    pub fn load() -> Result<Config, AppError> {
        let path = config_path()?;
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s)
                .map_err(|e| AppError::Other(format!("parsing {}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(AppError::Other(format!("reading {}: {e}", path.display()))),
        }
    }

    pub fn save(&self) -> Result<(), AppError> {
        let path = config_path()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| AppError::Other(format!("creating {}: {e}", dir.display())))?;
        }
        let body = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Other(format!("serializing config: {e}")))?;
        fs::write(&path, body)
            .map_err(|e| AppError::Other(format!("writing {}: {e}", path.display())))
    }

    /// Remove the config file entirely (used by `logout --forget`).
    pub fn clear() -> Result<bool, AppError> {
        let path = config_path()?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(AppError::Other(format!("removing {}: {e}", path.display()))),
        }
    }
}
