//! Output rendering.
//!
//! **Text is the primary format.** Resource reads render token-dense
//! `Key: value` blocks and pipe-delimited tables (`ALL_CAPS` headers). JSON is
//! reserved for control-plane signals — `init` / `set-credential` results,
//! `auth status`, and the raw `api` payload — never bolted onto resource reads.
//! Data goes to stdout; diagnostics and confirmations go to stderr.
//!
//! Many FPL response shapes aren't pinned down yet, so [`render`] flattens
//! whatever JSON comes back into readable text. As shapes are confirmed, add a
//! purpose-built renderer next to the generic one. For the raw structure, use
//! `fpl api`.

use serde_json::Value;

use crate::client::AccountSummary;

/// Pretty JSON on stdout. Control-plane only.
pub fn json(v: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
    );
}

/// Default text renderer for a resource read. Unwraps FPL's `{data: …}`
/// envelope, then renders an object as a key/value block or an array as a
/// pipe-delimited table.
pub fn render(v: &Value) {
    let body = v.get("data").unwrap_or(v);
    match body {
        Value::Array(arr) => render_table(arr),
        Value::Object(_) => render_kv(body, 0),
        Value::Null => println!("(no data)"),
        other => println!("{}", scalar(other)),
    }
}

fn render_kv(obj: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    if let Some(map) = obj.as_object() {
        for (k, val) in map {
            match val {
                Value::Object(_) => {
                    println!("{pad}{k}:");
                    render_kv(val, indent + 2);
                }
                Value::Array(arr) if arr.iter().all(|x| !x.is_object() && !x.is_array()) => {
                    let joined = arr.iter().map(scalar).collect::<Vec<_>>().join(", ");
                    println!("{pad}{k}: {joined}");
                }
                Value::Array(arr) => {
                    println!("{pad}{k}: [{} items]", arr.len());
                    render_table(arr);
                }
                other => println!("{pad}{k}: {}", scalar(other)),
            }
        }
    }
}

/// Render an array of objects as a pipe-delimited table with `ALL_CAPS`
/// headers. Falls back to one value per line for arrays of scalars.
fn render_table(arr: &[Value]) {
    if arr.is_empty() {
        println!("(none)");
        return;
    }
    if arr.iter().all(|x| !x.is_object()) {
        for x in arr {
            println!("{}", scalar(x));
        }
        return;
    }
    // Column order = union of keys, first-seen order.
    let mut cols: Vec<String> = Vec::new();
    for row in arr {
        if let Some(map) = row.as_object() {
            for k in map.keys() {
                if !cols.iter().any(|c| c == k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    println!(
        "{}",
        cols.iter()
            .map(|c| c.to_uppercase())
            .collect::<Vec<_>>()
            .join(" | ")
    );
    for row in arr {
        let cells: Vec<String> = cols
            .iter()
            .map(|c| row.get(c).map(scalar).unwrap_or_default())
            .collect();
        println!("{}", cells.join(" | "));
    }
}

pub fn accounts(list: &[AccountSummary]) {
    if list.is_empty() {
        println!("(no accounts found on this login)");
        return;
    }
    println!("ACCOUNT | STATUS | BALANCE | ADDRESS");
    for a in list {
        println!(
            "{} | {} | {} | {}",
            a.account_number,
            a.status_category.as_deref().unwrap_or(""),
            a.balance.as_deref().unwrap_or(""),
            a.address.as_deref().unwrap_or("")
        );
    }
}

/// The account holder's contact profile, pulled from account detail.
pub fn profile(detail: &Value) {
    let p = detail
        .pointer("/data/accountProfile")
        .unwrap_or(&Value::Null);
    if !p.is_object() {
        render(detail);
        return;
    }
    let field = |ptr: &str| p.pointer(ptr).map(scalar).filter(|s| !s.is_empty());

    if let Some(name) = field("/accountName").or_else(|| field("/name/fullName")) {
        println!("Name:    {name}");
    }
    if let Some(email) = field("/emailAddress").or_else(|| field("/emailAddressData/value")) {
        println!("Email:   {email}");
    }
    if let Some(phone) = field("/accountPhone/value") {
        println!("Phone:   {phone}");
    }
    let addr = [
        "/billAddress/line1",
        "/billAddress/city",
        "/billAddress/state",
        "/billAddress/zip",
    ]
    .iter()
    .filter_map(|ptr| field(ptr))
    .collect::<Vec<_>>();
    if !addr.is_empty() {
        // "line1, city, state zip"
        let street = addr.first().cloned().unwrap_or_default();
        let rest = addr[1..].join(" ");
        println!(
            "Address: {street}{}{rest}",
            if rest.is_empty() { "" } else { ", " }
        );
    }
}

/// County outage feed, optionally filtered by county-name substring.
pub fn outages(v: &Value, filter: Option<&str>) {
    let needle = filter.map(|s| s.to_lowercase());
    let matched: Vec<&Value> = v
        .get("outages")
        .and_then(|x| x.as_array())
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

    if matched.is_empty() {
        println!("(no matching counties)");
        return;
    }

    let mut total_out: i64 = 0;
    println!("COUNTY | OUT | SERVED");
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
        println!("{name} | {out} | {served}");
    }
    println!("TOTAL OUT | {total_out}");
}

/// Balance read: a concise Balance / Due / Past-due block, else flatten.
pub fn balance(v: &Value) {
    let d = v.get("data").unwrap_or(v);
    let first = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .filter_map(|k| d.get(*k))
            .find(|x| !x.is_null())
            .map(scalar)
    };

    let mut printed = false;
    if let Some(bal) = first(&["balance", "actualBalance", "amount"]) {
        println!("Balance:  {bal}");
        printed = true;
    }
    if let Some(due) = first(&["dueDateVal", "dueDate", "balance_due_date"]) {
        if !due.is_empty() {
            println!("Due:      {due}");
            printed = true;
        }
    }
    if let Some(past) = first(&["pastDueAmount", "pastDueAmt"]) {
        if !matches!(past.as_str(), "" | "0" | "0.0" | "$0.00") {
            println!("Past due: {past}");
        }
    }
    if !printed {
        render(v);
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scalar_unwraps_strings() {
        assert_eq!(scalar(&json!("hi")), "hi");
        assert_eq!(scalar(&json!(3)), "3");
        assert_eq!(scalar(&Value::Null), "");
    }
}
