//! Minimal date helpers so we don't pull in a calendar crate. We only need
//! "today"/"yesterday" and two FPL string formats.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days since the Unix epoch in UTC.
fn epoch_days() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    secs.div_euclid(86_400)
}

/// Convert days-since-epoch to a civil `(year, month, day)` date.
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn today() -> (i64, u32, u32) {
    civil_from_days(epoch_days())
}

pub fn yesterday() -> (i64, u32, u32) {
    civil_from_days(epoch_days() - 1)
}

/// `MM-DD-YYYY` — the format FPL's usage and payment endpoints expect.
pub fn fmt_mm_dd_yyyy((y, m, d): (i64, u32, u32)) -> String {
    format!("{m:02}-{d:02}-{y:04}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_epoch_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(18_993), (2022, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn format_pads() {
        assert_eq!(fmt_mm_dd_yyyy((2024, 3, 5)), "03-05-2024");
    }
}
