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
}

fn build_client() -> Result<reqwest::blocking::Client, AppError> {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .cookie_store(true)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Other(format!("failed to build HTTP client: {e}")))
}

/// Turn a service path into a full URL. Accepts either an absolute URL or a
/// leading-slash path relative to [`API_HOST`].
fn url_for(path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else if let Some(rest) = path.strip_prefix('/') {
        format!("{API_HOST}/{rest}")
    } else {
        format!("{API_HOST}/{path}")
    }
}

impl Fpl {
    /// Authenticate with FPL and capture the session JWT + cookies.
    pub fn login(username: &str, password: &Secret) -> Result<Fpl, AppError> {
        let client = build_client()?;
        let resp = client
            .get(url_for(LOGIN_PATH))
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
                "{msg} Re-run `fpl login` with the credentials you use at fpl.com."
            )));
        }

        Err(AppError::Auth(format!(
            "FPL login failed (HTTP {}).",
            status.as_u16()
        )))
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
        if matches!(status.as_u16(), 401 | 403) {
            return Err(AppError::Auth(format!(
                "FPL returned {} for {path} — session expired. Run `fpl login` again.",
                status.as_u16()
            )));
        }
        if status.as_u16() == 404 {
            return Err(AppError::NotFound(format!("{path} (HTTP 404)")));
        }
        if !status.is_success() {
            return Err(AppError::Network(format!(
                "FPL HTTP {} for {path}",
                status.as_u16()
            )));
        }
        // Some endpoints (PDF download) are not JSON; callers that need bytes use
        // `get_bytes`. Here we best-effort parse JSON and surface a clear error.
        let text = resp.text()?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|_| {
            AppError::Other(format!(
                "FPL returned a non-JSON response for {path} (first bytes: {:?})",
                &text.chars().take(60).collect::<String>()
            ))
        })
    }

    pub fn get(&self, path: &str) -> Result<Value, AppError> {
        let resp = self.auth_headers(self.client.get(url_for(path))).send()?;
        self.handle(resp, path)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value, AppError> {
        let resp = self
            .auth_headers(self.client.post(url_for(path)))
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
            "PUT" => {
                let resp = self
                    .auth_headers(self.client.put(url_for(path)))
                    .header("Content-Type", "application/json")
                    .json(body.unwrap_or(&Value::Null))
                    .send()?;
                self.handle(resp, path)
            }
            "DELETE" => {
                let resp = self
                    .auth_headers(self.client.delete(url_for(path)))
                    .send()?;
                self.handle(resp, path)
            }
            other => Err(AppError::Usage(format!(
                "unsupported HTTP method {other:?} (use GET, POST, PUT, or DELETE)"
            ))),
        }
    }

    /// Fetch raw bytes (e.g. a bill PDF) from an authenticated endpoint.
    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>, AppError> {
        let resp = self.auth_headers(self.client.get(url_for(path))).send()?;
        let status = resp.status();
        if matches!(status.as_u16(), 401 | 403) {
            return Err(AppError::Auth(format!(
                "FPL returned {} for {path} — session expired. Run `fpl login` again.",
                status.as_u16()
            )));
        }
        if !status.is_success() {
            return Err(AppError::Network(format!(
                "FPL HTTP {} for {path}",
                status.as_u16()
            )));
        }
        Ok(resp.bytes()?.to_vec())
    }

    // ---- Account discovery -------------------------------------------------

    /// List the caller's accounts (paginates the customer account endpoint).
    pub fn accounts(&self) -> Result<Vec<AccountSummary>, AppError> {
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

    pub fn balance(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/accountservices/resources/account/{account}/balance"
        ))
    }

    pub fn bill_history(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/sumbillaccount/resources/account/{account}/bill-history"
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

    pub fn download_bill(&self, account: &str) -> Result<Vec<u8>, AppError> {
        self.get_bytes(&format!(
            "/cs/customer/v1/documentretrieval/resources/account/{account}/download"
        ))
    }

    // ---- Payments ----------------------------------------------------------

    pub fn payment_options(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/paymentservices/resources/account/{account}/payment-option"
        ))
    }

    pub fn make_payment(&self, account: &str, body: &Value) -> Result<Value, AppError> {
        self.post(
            &format!("/cs/customer/v1/paymentservices/resources/account/{account}/payment"),
            body,
        )
    }

    // ---- History -----------------------------------------------------------

    pub fn account_history(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/accounthistory/resources/account/{account}/account-history"
        ))
    }

    pub fn deposit_history(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/accounthistory/resources/account/{account}/deposit-history"
        ))
    }

    pub fn document_history(&self, account: &str) -> Result<Value, AppError> {
        self.get(&format!(
            "/cs/customer/v1/documentretrieval/resources/account/{account}/document-history"
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

    // ---- Session -----------------------------------------------------------

    pub fn logout(&self) -> Result<(), AppError> {
        // Best-effort; ignore the body.
        let _ = self
            .auth_headers(self.client.get(url_for(LOGOUT_PATH)))
            .send();
        Ok(())
    }
}

/// Fetch the public county outage feed (no auth required).
pub fn county_outages() -> Result<Value, AppError> {
    let client = build_client()?;
    let resp = client.get(OUTAGE_COUNTY_URL).send()?;
    if !resp.status().is_success() {
        return Err(AppError::Network(format!(
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

    #[test]
    fn url_for_handles_paths_and_absolutes() {
        assert_eq!(url_for("/cs/x"), "https://www.fpl.com/cs/x");
        assert_eq!(url_for("cs/x"), "https://www.fpl.com/cs/x");
        assert_eq!(url_for("https://other/y"), "https://other/y");
    }

    #[test]
    fn premise_zero_pads_to_nine() {
        let d = json!({ "data": { "premiseNumber": 12345 } });
        assert_eq!(Fpl::premise_of(&d).as_deref(), Some("000012345"));
        let s = json!({ "data": { "premiseNumber": "987654321" } });
        assert_eq!(Fpl::premise_of(&s).as_deref(), Some("987654321"));
        assert_eq!(Fpl::premise_of(&json!({})), None);
    }
}
