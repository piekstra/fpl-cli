use std::fmt;

/// Application errors mapped to a stable exit-code contract so scripts and
/// agents can branch on `$?`.
#[derive(Debug)]
pub enum AppError {
    /// Bad usage / missing required input.
    Usage(String),
    /// Authentication required or rejected by FPL.
    Auth(String),
    /// Nothing matched (unknown account, empty history, etc.).
    NotFound(String),
    /// Network or upstream (non-2xx) failure.
    Network(String),
    /// OS keychain failure.
    Keychain(String),
    /// Anything else.
    Other(String),
}

impl AppError {
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::Usage(_) => 2,
            AppError::Auth(_) => 3,
            AppError::NotFound(_) => 4,
            AppError::Network(_) => 5,
            AppError::Keychain(_) | AppError::Other(_) => 1,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Usage(m) => write!(f, "{m}"),
            AppError::Auth(m) => write!(f, "{m}"),
            AppError::NotFound(m) => write!(f, "not found: {m}"),
            AppError::Network(m) => write!(f, "network/upstream error: {m}"),
            AppError::Keychain(m) => write!(f, "keychain error: {m}"),
            AppError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Network(e.to_string())
    }
}
