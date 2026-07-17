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

use pk_cli_core::output::scalar;
use serde_json::Value;

use crate::client::AccountSummary;

pub use pk_cli_core::output::{fail, json};

/// With `--json`, emit the raw payload; otherwise run the text renderer.
pub fn emit(json_mode: bool, v: &Value, text: impl FnOnce(&Value)) {
    if json_mode {
        json(v);
    } else {
        text(v);
    }
}

/// Default text renderer for a resource read. Unwraps FPL's `{data: …}`
/// envelope, then hands off to the shared renderer.
pub fn render(v: &Value) {
    pk_cli_core::output::render(v.get("data").unwrap_or(v));
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
    emit_lines(detail, fmt_profile(detail));
}

fn fmt_profile(detail: &Value) -> Option<Vec<String>> {
    let p = detail
        .pointer("/data/accountProfile")
        .filter(|p| p.is_object())?;
    let field = |ptr: &str| p.pointer(ptr).map(scalar).filter(|s| !s.is_empty());

    let mut out = Vec::new();
    if let Some(name) = field("/accountName").or_else(|| field("/name/fullName")) {
        out.push(format!("Name:    {name}"));
    }
    if let Some(email) = field("/emailAddress").or_else(|| field("/emailAddressData/value")) {
        out.push(format!("Email:   {email}"));
    }
    if let Some(phone) = field("/accountPhone/value") {
        out.push(format!("Phone:   {phone}"));
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
        out.push(format!(
            "Address: {street}{}{rest}",
            if rest.is_empty() { "" } else { ", " }
        ));
    }
    Some(out)
}

/// Account detail summary: identity, service address, meter, bill cycle, money.
pub fn account_detail(v: &Value) {
    emit_lines(v, fmt_account_detail(v));
}

fn fmt_account_detail(v: &Value) -> Option<Vec<String>> {
    let d = v.get("data").filter(|d| d.is_object())?;
    let s = |k: &str| d.get(k).map(scalar).filter(|x| !x.is_empty());
    let mut out = Vec::new();

    if let Some(num) = s("accountNumber") {
        let extra = [
            s("accountType"),
            s("statusName").or_else(|| s("statusCategory")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        out.push(if extra.is_empty() {
            format!("Account:  {num}")
        } else {
            format!("Account:  {num}  ({extra})")
        });
    }
    if let Some(addr) = d.get("serviceAddress") {
        let a = |k: &str| addr.get(k).map(scalar).filter(|x| !x.is_empty());
        let street = a("line1").unwrap_or_default();
        let locality = [a("city"), a("state"), a("zip")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        let full = [street, locality]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !full.is_empty() {
            out.push(format!("Service:  {full}"));
        }
    }
    if let Some(meter) = s("meterNo") {
        let serial = s("meterSerialNo")
            .map(|x| format!("  (serial {x})"))
            .unwrap_or_default();
        out.push(format!("Meter:    {meter}{serial}"));
    }
    if let Some(p) = s("premiseNumber") {
        out.push(format!("Premise:  {p}"));
    }
    // riderCode is often the sentinel "NO RIDER CODE" — drop it when there's none.
    let rider = s("riderCode").filter(|r| !r.to_uppercase().contains("NO RIDER"));
    let rate = [s("rateCode"), rider]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if !rate.is_empty() {
        out.push(format!("Rate:     {rate}"));
    }
    if d.get("currentBillDate").is_some() && d.get("nextBillDate").is_some() {
        out.push(format!(
            "Cycle:    {} to {}",
            short_date(d.get("currentBillDate")),
            short_date(d.get("nextBillDate"))
        ));
    }
    if let Some(bal) = d.get("balance").filter(|x| !x.is_null()) {
        out.push(format!("Balance:  {}", money(Some(bal))));
    }
    if let Some(past) = d.get("pastDueAmt") {
        let m = money(Some(past));
        if !matches!(m.as_str(), "" | "$0.00") {
            out.push(format!("Past due: {m}"));
        }
    }
    if let Some(lp) = d.get("lastPaymentAmt").filter(|x| !x.is_null()) {
        let amt = money(Some(lp));
        if !amt.is_empty() && amt != "$0.00" {
            let date = short_date(d.get("lastPaymentDate"));
            let when = if date.is_empty() {
                String::new()
            } else {
                format!(" on {date}")
            };
            out.push(format!("Last pay: {amt}{when}"));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// At-a-glance dashboard combining account detail + current-period energy.
pub fn summary(json_mode: bool, detail: &Value, energy: &Value) {
    if json_mode {
        json(&summary_json(detail, energy));
    } else {
        for l in fmt_summary(detail, energy) {
            println!("{l}");
        }
    }
}

fn summary_json(detail: &Value, energy: &Value) -> Value {
    let d = detail.get("data").cloned().unwrap_or(Value::Null);
    let cu = energy
        .pointer("/data/CurrentUsage")
        .cloned()
        .unwrap_or(Value::Null);
    serde_json::json!({
        "account": d.get("accountNumber"),
        "accountType": d.get("accountType"),
        "city": d.pointer("/serviceAddress/city"),
        "balance": d.get("balance"),
        "pastDue": d.get("pastDueAmt"),
        "currentBillDate": d.get("currentBillDate"),
        "nextBillDate": d.get("nextBillDate"),
        "currentUsage": cu,
    })
}

/// Coerce a JSON number *or* numeric string to an integer. FPL returns many
/// numeric fields (kWh, day counts) as strings.
fn num_i64(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_f64().map(|f| f.round() as i64))
        .or_else(|| {
            v.as_str()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .map(|f| f.round() as i64)
        })
}

/// Render a kWh value as a whole number (`"651.0"` → `651 kWh`).
fn kwh(v: &Value) -> String {
    num_i64(v)
        .map(|n| format!("{n} kWh"))
        .unwrap_or_else(|| scalar(v))
}

fn fmt_summary(detail: &Value, energy: &Value) -> Vec<String> {
    let d = detail.get("data").unwrap_or(detail);
    let cu = energy.pointer("/data/CurrentUsage");
    let cf = |k: &str| cu.and_then(|c| c.get(k)).filter(|x| !x.is_null());
    let mut out = Vec::new();

    let mut hdr: Vec<String> = Vec::new();
    if let Some(n) = d.get("accountNumber").map(scalar).filter(|x| !x.is_empty()) {
        hdr.push(format!("Account {n}"));
    }
    if let Some(t) = d.get("accountType").map(scalar).filter(|x| !x.is_empty()) {
        hdr.push(t);
    }
    if let Some(c) = d
        .pointer("/serviceAddress/city")
        .map(scalar)
        .filter(|x| !x.is_empty())
    {
        hdr.push(c);
    }
    if !hdr.is_empty() {
        out.push(hdr.join("  ·  "));
    }

    if let Some(bal) = d.get("balance").filter(|x| !x.is_null()) {
        let mut line = format!("Balance         {}", money(Some(bal)));
        if let Some(past) = d.get("pastDueAmt") {
            let m = money(Some(past));
            if !matches!(m.as_str(), "" | "$0.00") {
                line.push_str(&format!("   (past due {m})"));
            }
        }
        out.push(line);
    }

    if d.get("currentBillDate").is_some() && d.get("nextBillDate").is_some() {
        let days = match (
            cf("asOfDays").and_then(num_i64),
            cf("serviceDays").and_then(num_i64),
        ) {
            (Some(a), Some(s)) => format!("   (day {a} of {s})"),
            _ => String::new(),
        };
        out.push(format!(
            "Cycle           {} → {}{days}",
            short_date(d.get("currentBillDate")),
            short_date(d.get("nextBillDate")),
        ));
    }

    if let Some(pb) = cf("projectedBill") {
        let btd = cf("billToDate")
            .map(|x| format!("   ·  bill-to-date {}", money(Some(x))))
            .unwrap_or_default();
        let avg = cf("dailyAvg")
            .map(|x| format!("   ·  ~{}/day", money(Some(x))))
            .unwrap_or_default();
        out.push(format!("Projected bill  {}{btd}{avg}", money(Some(pb))));
    }

    if let Some(pk) = cf("projectedKWH") {
        let so_far = cf("billToDateKWH")
            .map(|x| format!("   ·  {} so far", kwh(x)))
            .unwrap_or_default();
        let avg = cf("dailyAverageKWH")
            .map(|x| format!("   ·  ~{}/day", kwh(x)))
            .unwrap_or_default();
        out.push(format!("Projected use   {}{so_far}{avg}", kwh(pk)));
    }

    out
}

/// County outage feed, optionally filtered by county-name substring.
pub fn outages(v: &Value, filter: Option<&str>) {
    for l in fmt_outages(v, filter) {
        println!("{l}");
    }
}

fn fmt_outages(v: &Value, filter: Option<&str>) -> Vec<String> {
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
        return vec!["(no matching counties)".to_string()];
    }

    let mut out = vec!["COUNTY | OUT | SERVED".to_string()];
    let mut total_out: i64 = 0;
    for row in &matched {
        let name = row
            .get("County Name")
            .and_then(|c| c.as_str())
            .unwrap_or("?");
        let served = row
            .get("Customers Served")
            .and_then(|c| c.as_str())
            .unwrap_or("0");
        let count = row
            .get("Customers Out")
            .and_then(|c| c.as_str())
            .unwrap_or("0");
        total_out += count.replace(',', "").parse::<i64>().unwrap_or(0);
        out.push(format!("{name} | {count} | {served}"));
    }
    out.push(format!("TOTAL OUT | {total_out}"));
    out
}

/// Balance read: a concise Balance / Due / Past-due block, else flatten.
pub fn balance(v: &Value) {
    emit_lines(v, fmt_balance(v));
}

fn fmt_balance(v: &Value) -> Option<Vec<String>> {
    let d = v.get("data").unwrap_or(v);
    let first = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .filter_map(|k| d.get(*k))
            .find(|x| !x.is_null())
            .map(scalar)
    };

    let mut out = Vec::new();
    if let Some(bal) = first(&["balance", "actualBalance", "amount"]) {
        out.push(format!("Balance:  {bal}"));
    }
    if let Some(due) = first(&["dueDateVal", "dueDate", "balance_due_date"]) {
        if !due.is_empty() {
            out.push(format!("Due:      {due}"));
        }
    }
    if let Some(past) = first(&["pastDueAmount", "pastDueAmt"]) {
        if !matches!(past.as_str(), "" | "0" | "0.0" | "$0.00") {
            out.push(format!("Past due: {past}"));
        }
    }
    (!out.is_empty()).then_some(out)
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

/// Print `lines`, or fall back to the generic renderer when there is nothing
/// tailored to show. The presenters below split into a `pub fn` (this + print)
/// and a pure `fmt_*` core that returns the lines, so the cores are unit-tested.
fn emit_lines(v: &Value, lines: Option<Vec<String>>) {
    match lines {
        Some(lines) => {
            for l in lines {
                println!("{l}");
            }
        }
        None => render(v),
    }
}

/// Prior bills as a clean table (bill-history nests rows under `data.data`).
pub fn bills_list(v: &Value) {
    emit_lines(v, fmt_bills_list(v));
}

fn fmt_bills_list(v: &Value) -> Option<Vec<String>> {
    let rows = v
        .pointer("/data/data")
        .or_else(|| v.get("data"))
        .and_then(|x| x.as_array())
        .filter(|r| !r.is_empty())?;
    let mut out = vec!["BILL DATE | AMOUNT | DUE | KWH | DAYS".to_string()];
    for r in rows {
        out.push(format!(
            "{} | {} | {} | {} | {}",
            short_date(r.get("dateBilled")),
            money(r.get("totalBillAmount")),
            short_date(r.get("dueDate")),
            cell(r.get("consumptionUnit")),
            cell(r.get("daysBilled")),
        ));
    }
    Some(out)
}

/// Current-period bill projection (mobile-energy-service `CurrentUsage`, or the
/// dedicated projected-bill payload — both carry the same fields under `data`).
pub fn bill_summary(v: &Value) {
    emit_lines(v, fmt_bill_summary(v));
}

fn fmt_bill_summary(v: &Value) -> Option<Vec<String>> {
    let node = v.pointer("/data/CurrentUsage").or_else(|| v.get("data"))?;
    let mut out = Vec::new();
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
            let s = if is_money {
                money(Some(val))
            } else {
                scalar(val)
            };
            if !s.is_empty() {
                out.push(format!("{label:<16}{s}"));
            }
        }
    }
    let (start, end) = (node.get("billStartDate"), node.get("billEndDate"));
    if start.is_some() && end.is_some() {
        out.push(format!(
            "{:<16}{} to {}",
            "Cycle",
            short_date(start),
            short_date(end)
        ));
    }
    (!out.is_empty()).then_some(out)
}

/// Budget Billing plan status + the monthly graph.
pub fn budget(v: &Value) {
    emit_lines(v, fmt_budget(v));
}

fn fmt_budget(v: &Value) -> Option<Vec<String>> {
    let d = v.get("data")?;
    let yesno = |k: &str| match d.get(k).and_then(|x| x.as_bool()) {
        Some(true) => "yes",
        Some(false) => "no",
        None => "—",
    };
    let mut out = vec![
        format!("Enrolled:        {}", yesno("enrolled")),
        format!("Eligible:        {}", yesno("eligibleForBudgetBilling")),
        format!("Budget amount:   {}", money(d.get("bbAmt"))),
        format!("Actual this bill:{}", money(d.get("eleAmt"))),
        format!("Deferred balance:{}", money(d.get("defAmt"))),
    ];
    if let Some(rows) = d.get("graphData").and_then(|x| x.as_array()) {
        if !rows.is_empty() {
            out.push(String::new());
            out.push("MONTH | ACTUAL | BUDGET | DEFERRED".to_string());
            for r in rows {
                let m = format!("{} {}", cell(r.get("month")), cell(r.get("year")));
                out.push(format!(
                    "{} | {} | {} | {}",
                    m.trim(),
                    money(r.get("actuallBillAmt")),
                    money(r.get("budgetBillAmt")),
                    money(r.get("deferredBalAmt")),
                ));
            }
        }
    }
    Some(out)
}

// ---- usage presenters -----------------------------------------------------

/// Current-period energy summary (kWh angle).
pub fn usage_summary(v: &Value) {
    emit_lines(v, fmt_usage_summary(v));
}

fn fmt_usage_summary(v: &Value) -> Option<Vec<String>> {
    let node = v.pointer("/data/CurrentUsage").or_else(|| v.get("data"))?;
    let mut out = Vec::new();
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
                out.push(format!("{label:<16}{s}"));
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Hourly usage for a day as a table, with daily totals.
pub fn hourly(v: &Value) {
    emit_lines(v, fmt_hourly(v));
}

fn fmt_hourly(v: &Value) -> Option<Vec<String>> {
    let rows = v
        .pointer("/data/HourlyUsage/data")
        .and_then(|x| x.as_array())
        .filter(|r| !r.is_empty())?;
    let mut out = vec!["HOUR | KWH | COST | TEMP".to_string()];
    let mut kwh = 0.0;
    let mut cost = 0.0;
    for r in rows {
        kwh += r.get("kwhActual").and_then(|x| x.as_f64()).unwrap_or(0.0);
        cost += r
            .get("billingCharged")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        out.push(format!(
            "{} | {} | {} | {}",
            cell(r.get("hour")),
            cell(r.get("kwhActual")),
            money(r.get("billingCharged")),
            cell(r.get("temperature")),
        ));
    }
    out.push(format!("TOTAL | {kwh:.1} | ${cost:.2} |"));
    Some(out)
}

/// Appliance-level breakdown for the most recent bill period.
pub fn appliances(v: &Value) {
    emit_lines(v, fmt_appliances(v));
}

fn fmt_appliances(v: &Value) -> Option<Vec<String>> {
    let p = v
        .pointer("/data/billPeriods")
        .and_then(|x| x.as_array())
        .and_then(|ps| {
            ps.iter()
                .find(|p| {
                    p.get("billPeriod")
                        .map(|b| scalar(b) == "1")
                        .unwrap_or(false)
                })
                .or_else(|| ps.first())
        })?;
    let mut out = vec![format!(
        "Period:  {} to {}   ({} kWh, {})",
        short_date(p.get("startDate")),
        short_date(p.get("endDate")),
        cell(p.get("kwh")),
        money(p.get("dollars")),
    )];
    if let Some(cats) = p.get("categories").and_then(|x| x.as_array()) {
        out.push(String::new());
        out.push("CATEGORY | KWH | COST | %".to_string());
        for c in cats {
            out.push(format!(
                "{} | {} | {} | {}",
                cell(c.get("category")),
                cell(c.get("kwh")),
                money(c.get("cost")),
                cell(c.get("percentage")),
            ));
        }
    }
    Some(out)
}

// ---- ledger presenters ----------------------------------------------------

/// Account ledger (charges + payments + adjustments) as a table.
pub fn ledger(v: &Value) {
    emit_lines(v, fmt_ledger(v));
}

fn fmt_ledger(v: &Value) -> Option<Vec<String>> {
    let rows = v
        .get("data")
        .and_then(|x| x.as_array())
        .filter(|r| !r.is_empty())?;
    let mut out = vec!["DATE | TYPE | AMOUNT | KWH | BALANCE".to_string()];
    for r in rows {
        out.push(format!(
            "{} | {} | {} | {} | {}",
            short_date(r.get("debitCreditTransactionDate")),
            cell(r.get("debitCreditDescriptionCode")),
            money(r.get("debitCreditAmount")),
            cell(r.get("kwh")),
            money(r.get("balanceAmount")),
        ));
    }
    Some(out)
}

/// Payments only, filtered out of the account ledger.
pub fn payments_list(v: &Value) {
    emit_lines(v, fmt_payments_list(v));
}

fn fmt_payments_list(v: &Value) -> Option<Vec<String>> {
    let rows = v.get("data").and_then(|x| x.as_array())?;
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
        return Some(vec!["(no payments in the recent ledger)".to_string()]);
    }
    let mut out = vec!["DATE | AMOUNT".to_string()];
    for r in pmts {
        // Payments are credits (negative in the ledger); show the magnitude.
        let amt = r
            .get("debitCreditAmount")
            .and_then(|x| x.as_f64())
            .map(|n| format!("${:.2}", n.abs()))
            .unwrap_or_default();
        out.push(format!(
            "{} | {}",
            short_date(r.get("debitCreditTransactionDate")),
            amt
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn money_formats() {
        assert_eq!(money(Some(&json!(196.16))), "$196.16");
        assert_eq!(money(Some(&json!(0))), "$0.00");
        assert_eq!(money(Some(&json!("$12.00"))), "$12.00");
        assert_eq!(money(Some(&json!("12"))), "$12");
        assert_eq!(money(None), "");
        assert_eq!(money(Some(&Value::Null)), "");
    }

    #[test]
    fn short_date_truncates_to_ymd() {
        assert_eq!(
            short_date(Some(&json!("2026-06-26T00:00:00.000Z"))),
            "2026-06-26"
        );
        assert_eq!(short_date(Some(&json!("2026-07-17"))), "2026-07-17");
        assert_eq!(short_date(None), "");
    }

    #[test]
    fn bills_list_renders_rows_and_falls_back_when_empty() {
        let v = json!({"data":{"data":[
            {"dateBilled":"2026-06-26","totalBillAmount":196.16,"dueDate":"2026-07-17",
             "consumptionUnit":1246,"daysBilled":30}
        ]}});
        let out = fmt_bills_list(&v).unwrap();
        assert_eq!(out[0], "BILL DATE | AMOUNT | DUE | KWH | DAYS");
        assert_eq!(out[1], "2026-06-26 | $196.16 | 2026-07-17 | 1246 | 30");
        assert!(fmt_bills_list(&json!({"data":{"data":[]}})).is_none());
        assert!(fmt_bills_list(&json!({})).is_none());
    }

    #[test]
    fn bill_summary_from_current_usage() {
        let v = json!({"data":{"CurrentUsage":{
            "projectedBill":128.0,"billToDate":83.88,"dailyAvg":5.99,
            "asOfDays":14,"avgHighTemp":96.0,
            "billStartDate":"2026-06-26","billEndDate":"2026-07-28"
        }}});
        let out = fmt_bill_summary(&v).unwrap();
        assert!(out.contains(&"Projected bill  $128.00".to_string()));
        assert!(out.contains(&"Bill to date    $83.88".to_string()));
        assert!(out.iter().any(|l| l.contains("2026-06-26 to 2026-07-28")));
        assert!(fmt_bill_summary(&json!({"other":1})).is_none());
    }

    #[test]
    fn budget_status_and_graph() {
        let v = json!({"data":{
            "enrolled":false,"eligibleForBudgetBilling":false,
            "bbAmt":132.49,"eleAmt":196.16,"defAmt":63.67,
            "graphData":[{"month":"Jun","year":2026,"actuallBillAmt":196.16,
                          "budgetBillAmt":132.49,"deferredBalAmt":63.67}]
        }});
        let out = fmt_budget(&v).unwrap();
        assert_eq!(out[0], "Enrolled:        no");
        assert!(out.contains(&"Budget amount:   $132.49".to_string()));
        assert!(out.contains(&"MONTH | ACTUAL | BUDGET | DEFERRED".to_string()));
        assert!(out.contains(&"Jun 2026 | $196.16 | $132.49 | $63.67".to_string()));
    }

    #[test]
    fn hourly_table_totals() {
        let v = json!({"data":{"HourlyUsage":{"data":[
            {"hour":1,"kwhActual":1.45,"billingCharged":0.21,"temperature":85.0},
            {"hour":2,"kwhActual":1.71,"billingCharged":0.24,"temperature":85.0}
        ]}}});
        let out = fmt_hourly(&v).unwrap();
        assert_eq!(out[0], "HOUR | KWH | COST | TEMP");
        assert_eq!(out[1], "1 | 1.45 | $0.21 | 85.0");
        assert_eq!(out.last().unwrap(), "TOTAL | 3.2 | $0.45 |");
    }

    #[test]
    fn appliances_picks_latest_period() {
        let v = json!({"data":{"billPeriods":[
            {"billPeriod":"2","startDate":"2026-04-27","endDate":"2026-05-28","kwh":1420,
             "dollars":225.29,"categories":[]},
            {"billPeriod":"1","startDate":"2026-05-28","endDate":"2026-06-26","kwh":1246,
             "dollars":196.16,"categories":[
                {"category":"cooling","kwh":593.0,"cost":93.4,"percentage":48.0}
             ]}
        ]}});
        let out = fmt_appliances(&v).unwrap();
        assert!(out[0].contains("Period:  2026-05-28 to 2026-06-26"));
        assert!(out[0].contains("1246 kWh, $196.16"));
        assert!(out.contains(&"cooling | 593.0 | $93.40 | 48.0".to_string()));
    }

    #[test]
    fn ledger_table() {
        let v = json!({"data":[
            {"debitCreditTransactionDate":"2026-07-07T04:00:00.000Z",
             "debitCreditDescriptionCode":"PYMT","debitCreditAmount":-196.16,
             "kwh":0,"balanceAmount":0.0}
        ]});
        let out = fmt_ledger(&v).unwrap();
        assert_eq!(out[0], "DATE | TYPE | AMOUNT | KWH | BALANCE");
        assert_eq!(out[1], "2026-07-07 | PYMT | $-196.16 | 0 | $0.00");
    }

    #[test]
    fn profile_from_account_detail() {
        let v = json!({"data":{"accountProfile":{
            "accountName":"Caleb Piekstra",
            "emailAddress":"c@example.com",
            "accountPhone":{"value":"360-555-1212"},
            "billAddress":{"line1":"6810 CHURCH ST","city":"Jupiter","state":"FL","zip":33458}
        }}});
        let out = fmt_profile(&v).unwrap();
        assert!(out.contains(&"Name:    Caleb Piekstra".to_string()));
        assert!(out.contains(&"Email:   c@example.com".to_string()));
        assert!(out.contains(&"Phone:   360-555-1212".to_string()));
        assert!(out
            .iter()
            .any(|l| l == "Address: 6810 CHURCH ST, Jupiter FL 33458"));
        // No accountProfile → fall back to the generic renderer.
        assert!(fmt_profile(&json!({"data":{}})).is_none());
    }

    #[test]
    fn account_detail_summary() {
        let v = json!({"data":{
            "accountNumber":"4265842247","accountType":"RESIDENTIAL","statusName":"ACTIVE",
            "serviceAddress":{"line1":"6810 CHURCH ST","city":"Jupiter","state":"FL","zip":33458},
            "meterNo":"D9267","meterSerialNo":"22838523","premiseNumber":"598237201",
            "rateCode":"RS1","riderCode":"",
            "currentBillDate":"2026-06-26T05:00:00.000","nextBillDate":"2026-07-28T05:00:00.000",
            "balance":0.0,"pastDueAmt":0.0,
            "lastPaymentAmt":196.16,"lastPaymentDate":"2026-07-07T00:00:00.000"
        }});
        let out = fmt_account_detail(&v).unwrap();
        assert_eq!(out[0], "Account:  4265842247  (RESIDENTIAL, ACTIVE)");
        assert!(out.contains(&"Service:  6810 CHURCH ST, Jupiter FL 33458".to_string()));
        assert!(out.contains(&"Meter:    D9267  (serial 22838523)".to_string()));
        assert!(out.contains(&"Cycle:    2026-06-26 to 2026-07-28".to_string()));
        assert!(out.contains(&"Balance:  $0.00".to_string()));
        assert!(out.contains(&"Last pay: $196.16 on 2026-07-07".to_string()));
        // No past-due line when zero.
        assert!(!out.iter().any(|l| l.starts_with("Past due")));
        assert!(fmt_account_detail(&json!({"foo":1})).is_none());
    }

    #[test]
    fn summary_dashboard() {
        let detail = json!({"data":{
            "accountNumber":"4265842247","accountType":"RESIDENTIAL",
            "serviceAddress":{"city":"Jupiter"},
            "balance":0.0,"pastDueAmt":0.0,
            "currentBillDate":"2026-06-26T05:00:00.000","nextBillDate":"2026-07-28T05:00:00.000"
        }});
        // FPL returns these numeric fields as strings; the renderer must coerce.
        let energy = json!({"data":{"CurrentUsage":{
            "projectedBill":204.43,"billToDate":90.94,"dailyAvg":6.06,
            "projectedKWH":"1389","billToDateKWH":"651.0","dailyAverageKWH":"43",
            "asOfDays":"15","serviceDays":"32"
        }}});
        let out = fmt_summary(&detail, &energy);
        assert_eq!(out[0], "Account 4265842247  ·  RESIDENTIAL  ·  Jupiter");
        assert!(out.iter().any(|l| l == "Balance         $0.00"));
        assert!(out
            .iter()
            .any(|l| l.contains("2026-06-26 → 2026-07-28") && l.contains("day 15 of 32")));
        assert!(out.iter().any(|l| l.starts_with("Projected bill  $204.43")
            && l.contains("bill-to-date $90.94")
            && l.contains("~$6.06/day")));
        assert!(out.iter().any(|l| l.starts_with("Projected use   1389 kWh")
            && l.contains("651 kWh so far")
            && l.contains("~43 kWh/day")));
    }

    #[test]
    fn balance_block_and_fallback() {
        let v = json!({"data":{"balance":"$0.00","dueDateVal":"Your account is paid in full."}});
        let out = fmt_balance(&v).unwrap();
        assert_eq!(out[0], "Balance:  $0.00");
        assert_eq!(out[1], "Due:      Your account is paid in full.");
        // Nothing recognizable → fall back.
        assert!(fmt_balance(&json!({"data":{"foo":1}})).is_none());
    }

    #[test]
    fn outages_table_totals_and_filter() {
        let v = json!({"outages":[
            {"County Name":"Broward","Customers Out":"70","Customers Served":"853,654"},
            {"County Name":"Palm Beach","Customers Out":"1,135","Customers Served":"151,037"}
        ]});
        let all = fmt_outages(&v, None);
        assert_eq!(all[0], "COUNTY | OUT | SERVED");
        assert_eq!(all.last().unwrap(), "TOTAL OUT | 1205"); // 70 + 1,135
        let broward = fmt_outages(&v, Some("broward"));
        assert!(broward.iter().any(|l| l == "Broward | 70 | 853,654"));
        assert!(!broward.iter().any(|l| l.contains("Palm Beach")));
        assert_eq!(
            fmt_outages(&json!({"outages":[]}), Some("nope")),
            vec!["(no matching counties)"]
        );
    }

    #[test]
    fn payments_list_filters_to_pymt_and_shows_magnitude() {
        let v = json!({"data":[
            {"debitCreditTransactionDate":"2026-07-07T04:00:00.000Z",
             "debitCreditDescriptionCode":"PYMT","debitCreditAmount":-196.16},
            {"debitCreditTransactionDate":"2026-06-26T04:00:00.000Z",
             "debitCreditDescriptionCode":"ELEC","debitCreditAmount":196.16}
        ]});
        let out = fmt_payments_list(&v).unwrap();
        assert_eq!(out, vec!["DATE | AMOUNT", "2026-07-07 | $196.16"]);

        let none = json!({"data":[{"debitCreditDescriptionCode":"ELEC","debitCreditAmount":10.0}]});
        assert_eq!(
            fmt_payments_list(&none).unwrap(),
            vec!["(no payments in the recent ledger)"]
        );
    }
}
