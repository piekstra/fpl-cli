//! Secret handling, shared with the CLI family via `pk-cli-secrets`:
//! keychain-only storage, `--stdin`/`--from-env` ingestion, redacting
//! zeroize-on-drop `Secret`.

pub use pk_cli_secrets::{read_from_env, read_stdin, CredentialStore, Secret};
