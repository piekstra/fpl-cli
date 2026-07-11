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

// ---- typed cell helpers for the tailored presenters ----------------------

/// Format an optional numeric/string field as `$X.XX`.
fn money(v: Option<&Value>) -> String {
    match v {
        Some(Value::Number(n)) => format!("${:.2}", n.as_f64().unwrap_or(0.0)),
        Some(Value::String(s)) if !s.is_empty() => {
            if s.starts_with('$') {
                s.clone()
            } else {
                format!("${s}")
            }
        }
        _ => String::new(),
    }
}

/// The `YYYY-MM-DD` prefix of a date/datetime string.
fn short_date(v: Option<&Value>) -> String {
    v.and_then(|x| x.as_str())
        .map(|s| s.chars().take(10).collect())
        .unwrap_or_default()
}

fn cell(v: Option<&Value>) -> String {
    v.map(scalar).unwrap_or_default()
}

// ---- billing presenters ---------------------------------------------------

/// Prior bills as a clean table (bill-history nests rows under `data.data`).
pub fn bills_list(v: &Value) {
    let rows = v
        .pointer("/data/data")
        .or_else(|| v.get("data"))
        .and_then(|x| x.as_array());
    match rows {
        Some(rows) if !rows.is_empty() => {
            println!("BILL DATE | AMOUNT | DUE | KWH | DAYS");
            for r in rows {
                println!(
                    "{} | {} | {} | {} | {}",
                    short_date(r.get("dateBilled")),
                    money(r.get("totalBillAmount")),
                    short_date(r.get("dueDate")),
                    cell(r.get("consumptionUnit")),
                    cell(r.get("daysBilled")),
                );
            }
        }
        _ => render(v),
    }
}

/// Current-period bill projection (mobile-energy-service `CurrentUsage`, or the
/// dedicated projected-bill payload — both carry the same fields under `data`).
pub fn bill_summary(v: &Value) {
    let node = v.pointer("/data/CurrentUsage").or_else(|| v.get("data"));
    let Some(node) = node else {
        return render(v);
    };
    let mut any = false;
    for (label, key, is_money) in [
        ("Projected bill", "projectedBill", true),
        ("Bill to date", "billToDate", true),
        ("Daily average", "dailyAvg", true),
        ("Projected kWh", "projectedKWH", false),
        ("Days into cycle", "asOfDays", false),
        ("Service days", "serviceDays", false),
        ("Avg high temp", "avgHighTemp", false),
    ] {
        if let Some(val) = node.get(key).filter(|x| !x.is_null()) {
            let s = if is_money { money(Some(val)) } else { scalar(val) };
            if !s.is_empty() {
                println!("{label:<16}{s}");
                any = true;
            }
        }
    }
    let (start, end) = (node.get("billStartDate"), node.get("billEndDate"));
    if start.is_some() && end.is_some() {
        println!(
            "{:<16}{} to {}",
            "Cycle",
            short_date(start),
            short_date(end)
        );
    }
    if !any {
        render(v);
    }
}

/// Budget Billing plan status + the monthly graph.
pub fn budget(v: &Value) {
    let Some(d) = v.get("data") else {
        return render(v);
    };
    let yesno = |k: &str| match d.get(k).and_then(|x| x.as_bool()) {
        Some(true) => "yes",
        Some(false) => "no",
        None => "—",
    };
    println!("Enrolled:        {}", yesno("enrolled"));
    println!("Eligible:        {}", yesno("eligibleForBudgetBilling"));
    println!("Budget amount:   {}", money(d.get("bbAmt")));
    println!("Actual this bill:{}", money(d.get("eleAmt")));
    println!("Deferred balance:{}", money(d.get("defAmt")));
    if let Some(rows) = d.get("graphData").and_then(|x| x.as_array()) {
        if !rows.is_empty() {
            println!();
            println!("MONTH | ACTUAL | BUDGET | DEFERRED");
            for r in rows {
                let m = format!("{} {}", cell(r.get("month")), cell(r.get("year")));
                println!(
                    "{} | {} | {} | {}",
                    m.trim(),
                    money(r.get("actuallBillAmt")),
                    money(r.get("budgetBillAmt")),
                    money(r.get("deferredBalAmt")),
                );
            }
        }
    }
}

// ---- usage presenters -----------------------------------------------------

/// Current-period energy summary (kWh angle).
pub fn usage_summary(v: &Value) {
    let node = v.pointer("/data/CurrentUsage").or_else(|| v.get("data"));
    let Some(node) = node else {
        return render(v);
    };
    let mut any = false;
    for (label, key) in [
        ("Projected kWh", "projectedKWH"),
        ("kWh to date", "billToDateKWH"),
        ("Daily avg kWh", "dailyAverageKWH"),
        ("Service days", "serviceDays"),
        ("Days into cycle", "asOfDays"),
        ("Avg high temp", "avgHighTemp"),
    ] {
        if let Some(val) = node.get(key).filter(|x| !x.is_null()) {
            let s = scalar(val);
            if !s.is_empty() {
                println!("{label:<16}{s}");
                any = true;
            }
        }
    }
    if !any {
        render(v);
    }
}

/// Hourly usage for a day as a table, with daily totals.
pub fn hourly(v: &Value) {
    let rows = v
        .pointer("/data/HourlyUsage/data")
        .and_then(|x| x.as_array());
    match rows {
        Some(rows) if !rows.is_empty() => {
            println!("HOUR | KWH | COST | TEMP");
            let mut kwh = 0.0;
            let mut cost = 0.0;
            for r in rows {
                kwh += r.get("kwhActual").and_then(|x| x.as_f64()).unwrap_or(0.0);
                cost += r.get("billingCharged").and_then(|x| x.as_f64()).unwrap_or(0.0);
                println!(
                    "{} | {} | {} | {}",
                    cell(r.get("hour")),
                    cell(r.get("kwhActual")),
                    money(r.get("billingCharged")),
                    cell(r.get("temperature")),
                );
            }
            println!("TOTAL | {kwh:.1} | ${cost:.2} |");
        }
        _ => render(v),
    }
}

/// Appliance-level breakdown for the most recent bill period.
pub fn appliances(v: &Value) {
    let periods = v.pointer("/data/billPeriods").and_then(|x| x.as_array());
    let latest = periods.and_then(|ps| {
        ps.iter()
            .find(|p| p.get("billPeriod").map(|b| scalar(b) == "1").unwrap_or(false))
            .or_else(|| ps.first())
    });
    let Some(p) = latest else {
        return render(v);
    };
    println!(
        "Period:  {} to {}   ({} kWh, {})",
        short_date(p.get("startDate")),
        short_date(p.get("endDate")),
        cell(p.get("kwh")),
        money(p.get("dollars")),
    );
    if let Some(cats) = p.get("categories").and_then(|x| x.as_array()) {
        println!();
        println!("CATEGORY | KWH | COST | %");
        for c in cats {
            println!(
                "{} | {} | {} | {}",
                cell(c.get("category")),
                cell(c.get("kwh")),
                money(c.get("cost")),
                cell(c.get("percentage")),
            );
        }
    }
}

// ---- ledger presenters ----------------------------------------------------

/// Account ledger (charges + payments + adjustments) as a table.
pub fn ledger(v: &Value) {
    let rows = v.get("data").and_then(|x| x.as_array());
    match rows {
        Some(rows) if !rows.is_empty() => {
            println!("DATE | TYPE | AMOUNT | KWH | BALANCE");
            for r in rows {
                println!(
                    "{} | {} | {} | {} | {}",
                    short_date(r.get("debitCreditTransactionDate")),
                    cell(r.get("debitCreditDescriptionCode")),
                    money(r.get("debitCreditAmount")),
                    cell(r.get("kwh")),
                    money(r.get("balanceAmount")),
                );
            }
        }
        _ => render(v),
    }
}

/// Payments only, filtered out of the account ledger.
pub fn payments_list(v: &Value) {
    let rows = v.get("data").and_then(|x| x.as_array());
    match rows {
        Some(rows) => {
            let pmts: Vec<&Value> = rows
                .iter()
                .filter(|r| {
                    r.get("debitCreditDescriptionCode")
                        .and_then(|c| c.as_str())
                        .map(|c| c.eq_ignore_ascii_case("PYMT"))
                        .unwrap_or(false)
                })
                .collect();
            if pmts.is_empty() {
                println!("(no payments in the recent ledger)");
                return;
            }
            println!("DATE | AMOUNT");
            for r in pmts {
                // Payments are credits (negative in the ledger); show the magnitude.
                let amt = r
                    .get("debitCreditAmount")
                    .and_then(|x| x.as_f64())
                    .map(|n| format!("${:.2}", n.abs()))
                    .unwrap_or_default();
                println!("{} | {}", short_date(r.get("debitCreditTransactionDate")), amt);
            }
        }
        _ => render(v),
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
