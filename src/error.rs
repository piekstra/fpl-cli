//! Error type shared with the CLI family: `pk_cli_core::CliError` carries the
//! stable exit-code contract (0–6) and the `--json` error envelope.

pub use pk_cli_core::CliError as AppError;
