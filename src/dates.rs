//! Date helpers, shared with the CLI family via `pk-cli-core`. FPL's usage
//! and payment endpoints take `MM-DD-YYYY`.

pub use pk_cli_core::dates::{fmt_mm_dd_yyyy, parse_iso, today, yesterday};
