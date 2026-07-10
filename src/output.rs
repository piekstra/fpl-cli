//! Output rendering. Every command supports `--json` (pretty JSON on stdout).
//! Human output is a light, readable summary; where a response shape isn't
//! pinned down we fall back to pretty JSON so nothing is silently dropped.

use serde_json::Value;

use crate::client::AccountSummary;

pub fn json(v: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
    );
}

/// Generic fallback: pretty JSON in both modes. Used for endpoints whose exact
/// human layout isn't verified yet.
pub fn value(v: &Value, _json: bool) {
    json(v);
}

pub fn accounts(list: &[AccountSummary], json_flag: bool) {
    if json_flag {
        println!(
            "{}",
            serde_json::to_string_pretty(list).unwrap_or_else(|_| "[]".into())
        );
        return;
    }
    if list.is_empty() {
        println!("(no accounts found on this login)");
        return;
    }
    for a in list {
        let status = a.status_category.as_deref().unwrap_or("?");
        println!("{}  [{status}]", a.account_number);
        if let Some(addr) = a.address.as_deref() {
            if !addr.is_empty() {
                println!("   {addr}");
            }
        }
    }
}

/// Render the county outage feed, optionally filtered by county substring.
pub fn outages(v: &Value, filter: Option<&str>, json_flag: bool) {
    let rows = v.get("outages").and_then(|x| x.as_array());
    let needle = filter.map(|s| s.to_lowercase());

    let matched: Vec<&Value> = rows
        .map(|arr| {
            arr.iter()
                .filter(|row| match &needle {
                    None => true,
                    Some(n) => row
                        .get("County Name")
                        .and_then(|c| c.as_str())
                        .map(|c| c.to_lowercase().contains(n))
                        .unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();

    if json_flag {
        let out: Vec<&Value> = matched;
        println!(
            "{}",
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "[]".into())
        );
        return;
    }

    if matched.is_empty() {
        println!("(no matching counties)");
        return;
    }

    let mut total_out: i64 = 0;
    println!("{:<20} {:>12} {:>16}", "County", "Out", "Served");
    println!("{:-<20} {:->12} {:->16}", "", "", "");
    for row in &matched {
        let name = row
            .get("County Name")
            .and_then(|c| c.as_str())
            .unwrap_or("?");
        let out = row
            .get("Customers Out")
            .and_then(|c| c.as_str())
            .unwrap_or("0");
        let served = row
            .get("Customers Served")
            .and_then(|c| c.as_str())
            .unwrap_or("0");
        total_out += out.replace(',', "").parse::<i64>().unwrap_or(0);
        println!("{name:<20} {out:>12} {served:>16}");
    }
    println!("{:-<20} {:->12} {:->16}", "", "", "");
    println!("{:<20} {:>12}", "TOTAL OUT", total_out);
}

/// Pull a few commonly-present fields off a balance/account payload for the
/// human view, then print the raw JSON beneath for anything we didn't surface.
pub fn balance(v: &Value, json_flag: bool) {
    if json_flag {
        json(v);
        return;
    }
    let data = v.get("data").unwrap_or(v);
    let mut printed = false;
    for (label, key) in [
        ("Balance", "amount"),
        ("Balance", "balance"),
        ("Balance", "actualBalance"),
        ("Due date", "dueDate"),
        ("Due date", "dueDateVal"),
        ("Past due", "pastDueAmount"),
    ] {
        if let Some(val) = data.get(key) {
            if !val.is_null() {
                println!("{label:<10} {}", scalar(val));
                printed = true;
            }
        }
    }
    if !printed {
        json(v);
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
