//! FPL customer-portal HTTP client (main region, "FL01").
//!
//! FPL exposes no official public API. Everything here targets the same
//! `www.fpl.com` JSON services the account-management web app and iOS app use.
//! Endpoint paths were mapped from the site's own service registry
//! (`/data/serviceconfparameters.js`) and cross-checked against the community
//! Home Assistant integration. See `docs/api.md`.
//!
//! Auth model: HTTP Basic on the login endpoint returns a `jwttoken` response
//! header plus session cookies. Subsequent calls send that JWT in a `jwttoken`
//! header and reuse the cookie jar.
//!
//! Northwest / former-Gulf-Power accounts ("FL02") use a different AWS Cognito
//! login flow and are not supported yet.

use std::time::Duration;

use pk_cli_core::transport::{require_secure_url, same_host_redirects, secure_redirects};
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::secrets::Secret;

pub const API_HOST: &str = "https://www.fpl.com";

/// Public, unauthenticated county-level outage feed behind fplmaps.com.
pub const OUTAGE_COUNTY_URL: &str = "https://www.fplmaps.com/customer/outage/CountyOutages.json";

/// A recent desktop Chrome UA. FPL's edge is picky about obviously-bot clients.
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

const LOGIN_PATH: &str =
    "/cs/customer/v1/registration/loginAndUseMigration?migrationToggle=Y&view=LoginMini";
const LOGOUT_PATH: &str = "/cs/customer/v1/registration/logout";

/// A logged-in FPL session.
pub struct Fpl {
    client: reqwest::blocking::Client,
    jwt: String,
}

/// One account as returned by the account-list endpoint.
#[derive(Debug, Serialize)]
pub struct AccountSummary {
    pub account_number: String,
    pub status_category: Option<String>,
    pub address: Option<String>,
    pub balance: Option<String>,
}

fn build_client(
    redirects: reqwest::redirect::Policy,
) -> Result<reqwest::blocking::Client, AppError> {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        // Total request budget, plus an explicit connect budget so a stalled
        // TCP/TLS handshake (which the overall `.timeout()` doesn't reliably
        // bound) fails fast instead of hanging.
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(15))
        .cookie_store(true)
        .redirect(redirects)
        .build()
        .map_err(|e| AppError::Other(format!("failed to build HTTP client: {e}")))
}

/// Pull a short human hint out of an error response body: a `message` /
/// `messageCode` field if the body is JSON, else a trimmed snippet. Returns a
/// leading `" — …"` (or empty) so it slots into an error message.
fn body_hint(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        for key in ["message", "messageCode", "error", "errorMessage"] {
            if let Some(m) = v.get(key).and_then(|x| x.as_str()) {
                if !m.is_empty() {
                    return format!(" — {m}");
                }
            }
        }
        if let Some(m) = v.pointer("/messages/0/message").and_then(|x| x.as_str()) {
            return format!(" — {m}");
        }
    }
    format!(" — {}", trimmed.chars().take(120).collect::<String>())
}

/// A failed login with any status but 401. FPL documents 401 as its refusal of
/// the credentials; anything else (a 5xx, a 429, a 403 from its edge) says
/// nothing about them.
fn login_failure(status: u16) -> AppError {
    AppError::Upstream(format!("FPL login failed (HTTP {status})."))
}

/// The client a session uses. The session JWT is FPL's own header, which reqwest
/// keeps on every hop, so a redirect to another host is refused.
fn session_client() -> Result<reqwest::blocking::Client, AppError> {
    build_client(same_host_redirects())
}

/// Turn a service path into a full URL. Accepts either an absolute URL or a
/// leading-slash path relative to [`API_HOST`]. An absolute URL is what `fpl api`
/// passes through, and the session JWT goes with every request, so it must be
/// `https`, or `http` only to this machine.
fn url_for(path: &str) -> Result<String, AppError> {
    if path.starts_with("http://") || path.starts_with("https://") {
        require_secure_url(path)?;
        Ok(path.to_string())
    } else if let Some(rest) = path.strip_prefix('/') {
        Ok(format!("{API_HOST}/{rest}"))
    } else {
        Ok(format!("{API_HOST}/{path}"))
    }
}

impl Fpl {
    /// Authenticate with FPL and capture the session JWT + cookies.
    pub fn login(username: &str, password: &Secret) -> Result<Fpl, AppError> {
        let client = session_client()?;
        let resp = client
            .get(url_for(LOGIN_PATH)?)
            .basic_auth(username, Some(password.expose()))
            .header("Accept", "application/json")
            .send()?;

        let status = resp.status();
        if status.is_success() {
            let jwt = resp
                .headers()
                .get("jwttoken")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            return Ok(Fpl { client, jwt });
        }

        if status.as_u16() == 401 {
            // Body carries a machine code: NOTVALIDUSER / FAILEDPASSWORD / ...
            let body: Value = resp.json().unwrap_or(Value::Null);
            let code = body
                .get("messageCode")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let msg = match code {
                "NOTVALIDUSER" => "FPL rejected the username (no such account).".to_string(),
                "FAILEDPASSWORD" => "FPL rejected the password.".to_string(),
                other if !other.is_empty() => format!("FPL login failed ({other})."),
                _ => "FPL login failed (401).".to_string(),
            };
            return Err(AppError::Auth(format!(
                "{msg} Re-run `fpl init` with the credentials you use at fpl.com."
            )));
        }

        Err(login_failure(status.as_u16()))
    }

    fn auth_headers(
        &self,
        mut req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        req = req.header("Accept", "application/json");
        if !self.jwt.is_empty() {
            req = req.header("jwttoken", &self.jwt);
        }
        req
    }

    fn handle(&self, resp: reqwest::blocking::Response, path: &str) -> Result<Value, AppError> {
        let status = resp.status();
        // Read the body once; include a snippet in error messages so a 4xx from
        // FPL explains itself instead of just showing a status code.
        let text = resp.text().unwrap_or_default();
        if matches!(status.as_u16(), 401 | 403) {
            return Err(AppError::Auth(format!(
                "FPL returned {} for {path} — session expired. Run `fpl init` again.",
                status.as_u16()
            )));
        }
        if status.as_u16() == 404 {
            return Err(AppError::NotFound(format!("{path} (HTTP 404)")));
        }
        if !status.is_success() {
            return Err(AppError::Upstream(format!(
                "FPL HTTP {} for {path}{}",
                status.as_u16(),
                body_hint(&text)
            )));
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|_| {
            AppError::Other(format!(
                "FPL returned a non-JSON response for {path} (first bytes: {:?})",
                text.chars().take(60).collect::<String>()
            ))
        })
    }

    pub fn get(&self, path: &str) -> Result<Value, AppError> {
        let resp = self.auth_headers(self.client.get(url_for(path)?)).send()?;
        self.handle(resp, path)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value, AppError> {
        let resp = self
            .auth_headers(self.client.post(url_for(path)?))
            .header("Content-Type", "application/json")
            .json(body)
            .send()?;
        self.handle(resp, path)
    }

    pub fn put(&self, path: &str, body: &Value) -> Result<Value, AppError> {
        let resp = self
            .auth_headers(self.client.put(url_for(path)?))
            .header("Content-Type", "application/json")
            .json(body)
            .send()?;
        self.handle(resp, path)
    }

    /// Raw request escape hatch used by `fpl api`. `method` is case-insensitive.
    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, AppError> {
        let m = method.to_uppercase();
        match m.as_str() {
            "GET" => self.get(path),
            "POST" => self.post(path, body.unwrap_or(&Value::Null)),
            "PUT" => self.put(path, body.unwrap_or(&Value::Null)),
            "DELETE" => {
                let resp = self
                    .auth_headers(self.client.delete(url_for(path)?))
                    .send()?;
                self.handle(resp, path)
            }
            other => Err(AppError::Usage(format!(
                "unsupported HTTP method {other:?} (use GET, POST, PUT, or DELETE)"
            ))),
        }
    }

    // ---- Account discovery -------------------------------------------------

    /// The portal header: the signed-in customer plus every account with its
    /// service address, balance, and status.
    pub fn header(&self) -> Result<Value, AppError> {
        self.get("/cs/customer/v1/resources/header")
    }

    /// Parse the header's per-account rows into summaries (address + balance).
    fn header_accounts(&self) -> Result<Vec<AccountSummary>, AppError> {
        let header = self.header()?;
        let rows = header
            .pointer("/data/accounts/data/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for a in &rows {
            let Some(num) = a.get("accountNumber").and_then(|v| v.as_str()) else {
                continue;
            };
            let address = a.get("serviceAddress").map(|addr| {
                let line1 = addr.get("line1").and_then(|v| v.as_str()).unwrap_or("");
                let city = addr.get("city").and_then(|v| v.as_str()).unwrap_or("");
                [line1, city]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            let balance = a.get("balance").map(|b| match b {
                Value::Number(n) => format!("${:.2}", n.as_f64().unwrap_or(0.0)),
                other => other.to_string(),
            });
            out.push(AccountSummary {
                account_number: num.to_string(),
                status_category: a
                    .get("statusName")
                    .or_else(|| a.get("statusCategory"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                address: address.filter(|s| !s.is_empty()),
                balance,
            });
        }
        Ok(out)
    }

    /// List the caller's accounts. Prefers the header endpoint (one call, with
    /// address + balance), falling back to the paginated list + `loginNew`.
    pub fn accounts(&self) -> Result<Vec<AccountSummary>, AppError> {
        if let Ok(list) = self.header_accounts() {
            if !list.is_empty() {
                return Ok(list);
            }
        }
        let mut out = Vec::new();
        let mut start = 1;
        let page = 10;
        loop {
            let path = format!(
                "/cs/customer/v1/resources/account?sortBy=status&count={page}&start={start}"
            );
            let body = self.get(&path)?;
            let rows = body
                .get("data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if rows.is_empty() {
                break;
            }
            for a in &rows {
                if let Some(num) = a.get("accountNumber").and_then(|v| v.as_str()) {
                    out.push(AccountSummary {
                        account_number: num.to_string(),
                        status_category: a
                            .get("statusCategory")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        address: a
                            .get("serviceAddress")
                            .or_else(|| a.get("premiseAddress"))
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        balance: None,
                    });
                }
            }
            let has_more = body
                .get("hasMore")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !has_more {
                break;
            }
            let count = body.get("count").and_then(|v| v.as_i64()).unwrap_or(page);
            start += count;
        }
        // Fall back to the app's `loginNew` fan-out if the paginated endpoint
        // returned nothing (some account types only surface there).
        if out.is_empty() {
            if let Ok(list) = self.account_list() {
                if let Some(rows) = list
                    .pointer("/data/AccountList/data")
                    .and_then(|v| v.as_array())
                {
                    for a in rows {
                        if let Some(num) = a.get("accountNumber").and_then(|v| v.as_str()) {
                            out.push(AccountSummary {
                                account_number: num.to_string(),
                                status_category: a
                                    .get("accountStatus")
                                    .or_else(|| a.get("statusCategory"))
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                address: a
                                    .get("serviceAddress")
                                    .or_else(|| a.get("premiseAddress"))
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                balance: a
                                    .get("balance")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                            });
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Select an account server-side and return its detail record.
    pub fn account_detail(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/accountservices/resources/account/{account}/select?view=account-lander"
        ))
    }

    /// Account list with balances (the app's `loginNew` fan-out).
    pub fn account_list(&self) -> Result<Value, AppError> {
        self.post(
            "/cs/customer/v1/accountservices/resources/loginNew?mediaChannel=IOS",
            &json!({}),
        )
    }

    /// Extract the 9-digit, zero-padded premise number from an account detail.
    pub fn premise_of(detail: &Value) -> Option<String> {
        detail
            .pointer("/data/premiseNumber")
            .and_then(|v| {
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
            .map(|s| format!("{s:0>9}"))
    }

    // ---- Billing -----------------------------------------------------------

    /// Balance and due status for one account. The dedicated `/balance` endpoint
    /// is finicky about pagination params, so we read the balance fields off the
    /// `loginNew` account list, which returns a clean per-account summary.
    pub fn balance(&self, account: &str) -> Result<Value, AppError> {
        let list = self.account_list()?;
        let rows = list
            .pointer("/data/AccountList/data")
            .and_then(|v| v.as_array());
        if let Some(rows) = rows {
            for a in rows {
                if a.get("accountNumber").and_then(|v| v.as_str()) == Some(account) {
                    return Ok(a.clone());
                }
            }
        }
        Err(AppError::NotFound(format!(
            "no balance information for account {account}"
        )))
    }

    pub fn bill_history(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/sumbillaccount/resources/account/{account}/bill-history"
        ))
    }

    /// Fetch a bill statement PDF. `date_billed` / `date_print` come from a
    /// `bill-history` row (`YYYY-MM-DD`); FPL wants them dash-stripped. The
    /// response is `{ data: { bytes: "<base64 PDF>", … } }`.
    pub fn download_bill(
        &self,
        account: &str,
        date_billed: &str,
        date_print: &str,
    ) -> Result<Value, AppError> {
        // All values are URL-safe (digits / fixed ASCII tokens).
        let bill_date = date_billed.replace('-', "");
        let print_date = date_print.replace('-', "");
        let query = format!(
            "channel=WEB&jobType=BL&requestedDocType=PDF&billAccountNumber={account}\
             &billDate={bill_date}&printDate={print_date}&fileName=billPdf-{print_date}.pdf\
             &docPathEncryptDecryptRequired=true&requestFor=ViewBillPdfRequest"
        );
        self.get(&format!(
            "/cs/customer/v1/documentretrieval/resources/account/{account}/download?{query}"
        ))
    }

    pub fn projected_bill(
        &self,
        account: &str,
        premise: &str,
        last_billed_mmddyyyy: &str,
    ) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/accountservices/resources/account/{account}/projectedBill?premiseNumber={premise}&lastBilledDate={last_billed_mmddyyyy}"
        ))
    }

    pub fn budget_billing(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/budgetbillingapi/resources/account/{account}/budgetBillingGraph"
        ))
    }

    // ---- Payments ----------------------------------------------------------

    pub fn payment_options(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/paymentservices/resources/account/{account}/payment-option"
        ))
    }

    /// Submit a payment. FPL's payment service creates via HTTP **PUT** (its
    /// `ServiceInvoker._create` uses PUT, not POST — a POST here 404s).
    pub fn make_payment(&self, account: &str, body: &Value) -> Result<Value, AppError> {
        self.put(
            &format!("/cs/customer/v1/paymentservices/resources/account/{account}/payment"),
            body,
        )
    }

    // ---- History -----------------------------------------------------------

    pub fn account_history(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/accounthistory/resources/account/{account}/account-history?count=25&start=1&sortBy=date"
        ))
    }

    pub fn deposit_history(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/accounthistory/resources/account/{account}/deposit-history?count=25&start=1&sortBy=date"
        ))
    }

    // ---- Usage -------------------------------------------------------------

    /// Current-period energy summary (projected kWh, daily average, bill-to-date).
    pub fn energy_usage(
        &self,
        account: &str,
        premise: &str,
        last_billed_mmddyyyy: &str,
        meter_no: &str,
    ) -> Result<Value, AppError> {
        let body = json!({
            "status": "2",
            "accountType": "RESIDENTIAL",
            "premiseNumber": premise,
            "lastBilledDate": last_billed_mmddyyyy,
            "amrFlag": "Y",
            "revCode": "1",
            "meterNo": meter_no,
        });
        self.post(
            &format!("/cs/customer/v1/energydashboard/resources/energy-usage/account/{account}/mobile-energy-service"),
            &body,
        )
    }

    /// Hourly usage for a single day (`MM-DD-YYYY`).
    pub fn hourly_usage(
        &self,
        account: &str,
        premise: &str,
        date_mmddyyyy: &str,
    ) -> Result<Value, AppError> {
        let body = json!({ "premiseNumber": premise, "startDate": date_mmddyyyy });
        self.post(
            &format!("/cs/customer/v1/energydashboard/resources/energy-usage/account/{account}/mobile-hourly-usage"),
            &body,
        )
    }

    /// Appliance-level disaggregated usage for the latest bill period.
    pub fn appliance_usage(&self, account: &str, premise: &str) -> Result<Value, AppError> {
        let body = json!({ "premiseId": premise, "accountNumber": account });
        self.post(
            &format!("/cs/customer/v1/energyanalyzer/resources/{account}/getDisaggResp"),
            &body,
        )
    }

    // ---- Diagnostics & notifications ---------------------------------------

    /// Smart-meter (AMI) status: whether the meter is reporting, breaker state,
    /// and the ping window. What the outage-status page uses to check a meter.
    pub fn meter_status(&self, account: &str) -> Result<Value, AppError> {
        self.post(
            "/cs/customer/v1/wors/public/amiping",
            &json!({ "accountNumber": account }),
        )
    }

    /// Account alert/banner state (balance alerts, collection thresholds, flags).
    pub fn alerts(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/profileservices/resources/account/{account}/alert-notification"
        ))
    }

    // ---- Public reference data ---------------------------------------------

    /// FPL-served Florida cities.
    pub fn cities(&self) -> Result<Value, AppError> {
        self.get("/cs/customer/v1/connect-journey/public/city")
    }

    /// FPL-served Florida ZIP codes.
    pub fn zips(&self) -> Result<Value, AppError> {
        self.get("/cs/customer/v1/connect-journey/public/zip")
    }

    // ---- Session -----------------------------------------------------------

    pub fn logout(&self) -> Result<(), AppError> {
        // Best-effort; ignore the body.
        let _ = self
            .auth_headers(self.client.get(url_for(LOGOUT_PATH)?))
            .send();
        Ok(())
    }
}

/// Fetch the public county outage feed (no auth required).
pub fn county_outages() -> Result<Value, AppError> {
    county_outages_from(OUTAGE_COUNTY_URL)
}

fn county_outages_from(url: &str) -> Result<Value, AppError> {
    let client = build_client(secure_redirects())?;
    let resp = client.get(url).send()?;
    if !resp.status().is_success() {
        return Err(AppError::Upstream(format!(
            "outage feed HTTP {}",
            resp.status().as_u16()
        )));
    }
    Ok(resp.json::<Value>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A stub on 127.0.0.1. Answers each request with `respond(request)` as a raw
    /// HTTP response, and keeps each request so a test can see what arrived.
    fn stub(
        respond: impl Fn(&str) -> String + Send + 'static,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                let request = loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).into_owned();
                    let complete = text.split_once("\r\n\r\n").is_some_and(|(head, body)| {
                        let length = head
                            .lines()
                            .find_map(|l| {
                                let (name, value) = l.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        body.len() >= length
                    });
                    if n == 0 || complete {
                        break text;
                    }
                };
                let response = respond(&request);
                log.lock().unwrap().push(request);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (base, seen)
    }

    fn redirect_to(location: &str) -> String {
        format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    fn ok_json(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn session() -> Fpl {
        Fpl {
            client: session_client().unwrap(),
            jwt: "dummy-jwt".into(),
        }
    }

    #[test]
    fn a_redirect_to_another_host_is_refused_and_the_jwt_stays_here() {
        let (other, reached) = stub(|_| ok_json("{}"));
        let target = format!("{other}/next");
        let (base, seen) = stub(move |_| redirect_to(&target));
        let err = session().get(&format!("{base}/start")).unwrap_err();
        assert!(err.to_string().contains("another host"), "{err}");
        assert!(seen.lock().unwrap()[0].contains("dummy-jwt"));
        assert!(reached.lock().unwrap().is_empty());
    }

    #[test]
    fn a_redirect_on_the_same_host_is_followed() {
        let (base, seen) = stub(|request| {
            if request.starts_with("GET /start") {
                redirect_to("/next")
            } else {
                ok_json(r#"{"ok":true}"#)
            }
        });
        let v = session().get(&format!("{base}/start")).unwrap();
        assert_eq!(v, json!({"ok": true}));
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn the_outage_feed_refuses_a_redirect_that_leaves_https() {
        let (base, _) = stub(|_| redirect_to("http://outages.example.invalid/feed"));
        let err = county_outages_from(&format!("{base}/feed")).unwrap_err();
        assert!(err.to_string().contains("leaves https"), "{err}");
    }

    #[test]
    fn a_login_failure_other_than_401_is_upstream_not_auth() {
        for status in [403, 404, 429, 500, 503] {
            assert!(
                matches!(login_failure(status), AppError::Upstream(_)),
                "{status}"
            );
        }
    }

    #[test]
    fn url_for_handles_paths_and_absolutes() {
        assert_eq!(url_for("/cs/x").unwrap(), "https://www.fpl.com/cs/x");
        assert_eq!(url_for("cs/x").unwrap(), "https://www.fpl.com/cs/x");
        assert_eq!(url_for("https://other/y").unwrap(), "https://other/y");
        assert_eq!(
            url_for("http://127.0.0.1:8080/y").unwrap(),
            "http://127.0.0.1:8080/y"
        );
    }

    /// The JWT goes with every request, so `fpl api GET http://…` must not send.
    /// This host resolves nowhere, so only a refusal before the send gets exit 2.
    #[test]
    fn a_plain_http_url_is_refused_before_anything_is_sent() {
        let session = Fpl {
            client: build_client(same_host_redirects()).unwrap(),
            jwt: "dummy-jwt".into(),
        };
        let url = "http://www.example.invalid/x?account=1234567890";
        for result in [
            session.get(url),
            session.post(url, &Value::Null),
            session.put(url, &Value::Null),
            session.request("DELETE", url, None),
        ] {
            let err = result.unwrap_err();
            assert_eq!(err.exit_code(), 2, "{err}");
            assert!(err.to_string().contains("only https"), "{err}");
            assert!(!err.to_string().contains("1234567890"), "{err}");
        }
    }

    #[test]
    fn premise_zero_pads_to_nine() {
        let d = json!({ "data": { "premiseNumber": 12345 } });
        assert_eq!(Fpl::premise_of(&d).as_deref(), Some("000012345"));
        let s = json!({ "data": { "premiseNumber": "987654321" } });
        assert_eq!(Fpl::premise_of(&s).as_deref(), Some("987654321"));
        assert_eq!(Fpl::premise_of(&json!({})), None);
    }

    #[test]
    fn body_hint_empty_is_blank() {
        assert_eq!(body_hint(""), "");
        assert_eq!(body_hint("   \n"), "");
    }

    #[test]
    fn body_hint_prefers_json_message_fields() {
        // Real FPL 400s carry the explanation in a `message` field.
        assert_eq!(
            body_hint(r#"{"message":"Required Integer parameter 'count' is not present"}"#),
            " — Required Integer parameter 'count' is not present"
        );
        // Falls through the key priority to messageCode / error.
        assert_eq!(
            body_hint(r#"{"messageCode":"FAILEDPASSWORD"}"#),
            " — FAILEDPASSWORD"
        );
        assert_eq!(body_hint(r#"{"error":"boom"}"#), " — boom");
        // Nested messages[].message array.
        assert_eq!(
            body_hint(r#"{"messages":[{"message":"Unable to retrieve premise"}]}"#),
            " — Unable to retrieve premise"
        );
    }

    #[test]
    fn body_hint_falls_back_to_snippet() {
        // A plain-text body (e.g. the 555 "No file path available in DB").
        assert_eq!(
            body_hint("No file path available in DB"),
            " — No file path available in DB"
        );
        // JSON without a recognized key is shown as its raw (truncated) text.
        assert_eq!(body_hint(r#"{"foo":"bar"}"#), r#" — {"foo":"bar"}"#);
        // Long bodies are truncated to 120 chars.
        let long = "x".repeat(200);
        assert_eq!(body_hint(&long).chars().count(), 120 + 3); // " — " + 120
    }
}
