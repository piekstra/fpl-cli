//! Date helpers, shared with the CLI family via `pk-cli-core`. FPL's usage
//! endpoints take `MM-DD-YYYY`; the payment service takes ISO `YYYY-MM-DD`.

pub use pk_cli_core::dates::{fmt_iso, fmt_mm_dd_yyyy, parse_iso, today, yesterday};
