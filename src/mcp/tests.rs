//! The `fpl mcp` tools, offline: `tests/fixtures/` behind the [`Portal`] seam,
//! mapped by the commands' own mappers, checked by cli-common's conformance
//! helper. `tests/mcp_stdio.rs` and `tests/mcp_http.rs` drive the real binary.

use std::cell::RefCell;
use std::rc::Rc;

use pk_cli_mcp::conformance::{assert_conforms, assert_fits, call, definitions_size};
use serde_json::json;

use super::*;

fn fixture(raw: &str) -> Value {
    serde_json::from_str(raw).expect("fixture parses")
}

struct Fixtures {
    accounts: Vec<AccountSummary>,
    detail: Value,
    bills: Value,
    ledger: Value,
    /// Every read, with its account.
    calls: Rc<RefCell<Vec<String>>>,
}

impl Fixtures {
    fn new() -> Self {
        Self {
            accounts: vec![
                AccountSummary {
                    account_number: "1234567890".into(),
                    status_category: Some("ACTIVE".into()),
                    address: Some("100 Example Street".into()),
                    balance: Some("12.34".into()),
                },
                AccountSummary {
                    account_number: "2345678901".into(),
                    status_category: Some("ACTIVE".into()),
                    address: Some("200 Example Street".into()),
                    balance: Some("0.00".into()),
                },
            ],
            detail: fixture(include_str!("../../tests/fixtures/account_detail.json")),
            bills: fixture(include_str!("../../tests/fixtures/bill_history.json")),
            ledger: fixture(include_str!("../../tests/fixtures/account_history.json")),
            calls: Rc::default(),
        }
    }

    fn log(&self, what: &str) {
        self.calls.borrow_mut().push(what.to_string());
    }
}

impl Portal for Fixtures {
    fn accounts(&self) -> Result<Vec<AccountSummary>, AppError> {
        self.log("accounts");
        Ok(self
            .accounts
            .iter()
            .map(|a| AccountSummary {
                account_number: a.account_number.clone(),
                status_category: a.status_category.clone(),
                address: a.address.clone(),
                balance: a.balance.clone(),
            })
            .collect())
    }

    fn account_detail(&self, account: &str) -> Result<Value, AppError> {
        self.log(&format!("detail {account}"));
        Ok(self.detail.clone())
    }

    fn bill_history(&self, account: &str) -> Result<Value, AppError> {
        self.log(&format!("bills {account}"));
        Ok(self.bills.clone())
    }

    fn account_history(&self, account: &str) -> Result<Value, AppError> {
        self.log(&format!("ledger {account}"));
        Ok(self.ledger.clone())
    }
}

fn tools_over(
    portal: Fixtures,
    default_account: Option<&str>,
) -> UtilityTools<FplSource<Fixtures>> {
    with_source(
        FplSource {
            portal,
            default_account: default_account.map(str::to_string),
        },
        false,
    )
}

fn ok(t: &UtilityTools<FplSource<Fixtures>>, tool: &str, args: Value) -> Value {
    let result = call(t, tool, args);
    assert_fits(t, tool, &result).clone()
}

#[test]
fn the_definitions_conform_and_touch_no_portal() {
    let portal = Fixtures::new();
    let calls = portal.calls.clone();
    let t = tools_over(portal, None);
    assert_conforms(&t);
    assert!(calls.borrow().is_empty());
    assert!(definitions_size(&t) < 8_000);
    let names: Vec<String> = pk_cli_mcp::Toolset::tools(&t)
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(
        names,
        ["fpl_balance", "fpl_bills", "fpl_payments", "fpl_usage"]
    );
}

#[test]
fn balance_maps_the_detail_and_names_the_other_account() {
    let t = tools_over(Fixtures::new(), None);
    let out = ok(&t, "fpl_balance", json!({}));
    assert_eq!(out["balance"], "12.34");
    assert_eq!(out["due_date"], "2026-08-01");
    assert_eq!(out["account"], "100 Example Street");
    assert_eq!(out["accounts"][1]["name"], "200 Example Street");
    assert!(!out.to_string().contains("1234567890"), "{out}");
}

#[test]
fn an_account_resolves_by_address_and_the_default_comes_before_the_first() {
    let portal = Fixtures::new();
    let calls = portal.calls.clone();
    let t = tools_over(portal, Some("2345678901"));
    ok(&t, "fpl_bills", json!({}));
    ok(&t, "fpl_bills", json!({"account": "100 example"}));
    assert_eq!(
        *calls.borrow(),
        ["bills 2345678901", "accounts", "bills 1234567890"]
    );
}

#[test]
fn bills_come_newest_first_with_dates_in_iso() {
    let t = tools_over(Fixtures::new(), None);
    let page = ok(&t, "fpl_bills", json!({}));
    assert_eq!(page["returned"], 3);
    assert_eq!(
        page["items"][0],
        json!({"date": "2026-06-15", "amount": "12.34", "due_date": "2026-07-05"})
    );
    assert_eq!(page["items"][2]["date"], "2026-04-15");
    let page = ok(
        &t,
        "fpl_bills",
        json!({"since": "2026-05-01", "until": "2026-05-31"}),
    );
    assert_eq!(page["returned"], 1);
    assert_eq!(page["items"][0]["amount"], "23.45");
}

#[test]
fn bills_read_a_flat_data_list_too() {
    let mut portal = Fixtures::new();
    portal.bills = json!({"data": portal.bills["data"]["data"]});
    let t = tools_over(portal, None);
    assert_eq!(ok(&t, "fpl_bills", json!({}))["returned"], 3);
}

#[test]
fn payments_are_the_ledger_s_payment_rows_as_magnitudes() {
    let t = tools_over(Fixtures::new(), None);
    let page = ok(&t, "fpl_payments", json!({}));
    assert_eq!(
        page["items"],
        json!([
            {"date": "2026-06-20", "amount": "23.45"},
            {"date": "2026-05-20", "amount": "34.56"}
        ])
    );
}

#[test]
fn usage_is_the_kwh_on_each_bill() {
    let t = tools_over(Fixtures::new(), None);
    let page = ok(&t, "fpl_usage", json!({}));
    assert_eq!(
        page["items"][0],
        json!({"period": "2026-06-15", "quantity": 111.0, "unit": "kWh", "cost": "12.34"})
    );
    assert_eq!(page["items"][1]["quantity"], 222.0);
    assert_eq!(page["items"][2]["period"], "2026-04-15");
}

#[test]
fn a_body_with_no_list_is_an_upstream_error_not_an_empty_list() {
    let mut portal = Fixtures::new();
    portal.bills = json!({"data": {"message": "not here"}});
    portal.ledger = json!({"message": "not here"});
    let t = tools_over(portal, None);
    for tool in ["fpl_bills", "fpl_usage", "fpl_payments"] {
        let result = call(&t, tool, json!({}));
        assert!(result.is_error(), "{tool}");
        assert!(result.text().starts_with("upstream:"), "{}", result.text());
    }
}

#[test]
fn a_balance_fpl_did_not_state_is_an_error_not_zero() {
    for detail in [
        Value::Null,
        json!({"data": {"accountNumber": "1234567890"}}),
    ] {
        let mut portal = Fixtures::new();
        portal.detail = detail;
        let t = tools_over(portal, None);
        let result = call(&t, "fpl_balance", json!({}));
        assert!(result.text().starts_with("upstream:"), "{}", result.text());
    }
}

#[test]
fn rows_none_of_which_can_be_read_are_an_error_not_an_empty_list() {
    let mut portal = Fixtures::new();
    portal.bills = json!({"data": {"data": [{"renamedDate": "2026-06-15"}]}});
    portal.ledger = json!({"data": [{"debitCreditDescriptionCode": "PYMT"}]});
    let t = tools_over(portal, None);
    for tool in ["fpl_bills", "fpl_usage", "fpl_payments"] {
        let result = call(&t, tool, json!({}));
        assert!(result.text().starts_with("upstream:"), "{}", result.text());
    }
}

#[test]
fn a_bill_with_no_readable_amount_is_left_out_rather_than_shown_as_zero() {
    let mut portal = Fixtures::new();
    portal.bills["data"]["data"][0]["totalBillAmount"] = Value::Null;
    let t = tools_over(portal, None);
    let page = ok(&t, "fpl_bills", json!({}));
    assert_eq!(page["returned"], 2);
}

#[test]
fn an_empty_answer_is_an_empty_list() {
    let mut portal = Fixtures::new();
    portal.bills = json!({"data": {"data": []}});
    portal.ledger = json!({"data": []});
    let t = tools_over(portal, None);
    assert_eq!(ok(&t, "fpl_bills", json!({}))["returned"], 0);
    assert_eq!(ok(&t, "fpl_payments", json!({}))["returned"], 0);
}

#[test]
fn since_narrows_payments_though_the_source_cannot() {
    let t = tools_over(Fixtures::new(), None);
    let page = ok(&t, "fpl_payments", json!({"since": "2026-06-01"}));
    assert_eq!(page["returned"], 1);
    assert_eq!(page["items"][0]["date"], "2026-06-20");
}

#[test]
fn a_password_in_the_environment_points_at_where_it_is_set() {
    fn expired(_: &str) -> Result<Value, AppError> {
        Err(AppError::Auth("FPL rejected the password.".into()))
    }
    struct Refusing;
    impl Portal for Refusing {
        fn accounts(&self) -> Result<Vec<AccountSummary>, AppError> {
            Ok(Vec::new())
        }
        fn account_detail(&self, a: &str) -> Result<Value, AppError> {
            expired(a)
        }
        fn bill_history(&self, a: &str) -> Result<Value, AppError> {
            expired(a)
        }
        fn account_history(&self, a: &str) -> Result<Value, AppError> {
            expired(a)
        }
    }
    let source = |account: &str| FplSource {
        portal: Refusing,
        default_account: Some(account.to_string()),
    };
    let text = call(&with_source(source("1"), true), "fpl_usage", json!({}))
        .text()
        .to_string();
    assert!(text.contains("Claude Desktop extension"), "{text}");
    assert!(!text.contains("fpl auth login"), "{text}");

    let text = call(&with_source(source("1"), false), "fpl_usage", json!({}))
        .text()
        .to_string();
    assert!(text.contains("`fpl auth login`"), "{text}");
}

mod sign_in {
    use std::cell::Cell;

    use super::*;

    /// A login that counts its calls and answers as told.
    fn login_with<'a>(
        calls: &'a Cell<u32>,
        answer: impl Fn(&str) -> Result<&'static str, AppError> + 'a,
    ) -> impl Fn(&Secret) -> Result<&'static str, AppError> + 'a {
        move |password| {
            calls.set(calls.get() + 1);
            answer(password.expose())
        }
    }

    fn refuse(_: &str) -> Result<&'static str, AppError> {
        Err(AppError::Auth("FPL rejected the password.".into()))
    }

    fn stored(value: &'static str) -> impl Fn() -> Result<Secret, AppError> {
        move || Ok(Secret::new(value))
    }

    fn read_ok(_: &&'static str) -> Result<u32, AppError> {
        Ok(7)
    }

    #[test]
    fn a_refused_password_is_not_sent_again_and_a_different_one_is() {
        let calls = Cell::new(0);
        let mut state = SignIn::default();
        let login = login_with(&calls, refuse);
        assert!(state.read(stored("old"), &login, read_ok).is_err());
        let again = state.read(stored("old"), &login, read_ok).unwrap_err();
        assert_eq!(calls.get(), 1, "the refused password went out again");
        assert!(again.to_string().contains("not sent again"), "{again}");

        let login = login_with(&calls, |_| Ok("session"));
        assert_eq!(state.read(stored("new"), &login, read_ok).unwrap(), 7);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn fpl_being_down_is_not_a_refusal() {
        let calls = Cell::new(0);
        let mut state = SignIn::default();
        let down = login_with(&calls, |_| Err(AppError::Upstream("HTTP 503".into())));
        assert!(state.read(stored("same"), &down, read_ok).is_err());
        assert!(state.read(stored("same"), &down, read_ok).is_err());
        assert_eq!(calls.get(), 2, "an outage must not stop the next try");
    }

    #[test]
    fn a_session_is_kept_and_signs_in_once_more_when_it_lapses() {
        let calls = Cell::new(0);
        let mut state = SignIn::default();
        let login = login_with(&calls, |_| Ok("session"));
        state.read(stored("p"), &login, read_ok).unwrap();
        state.read(stored("p"), &login, read_ok).unwrap();
        assert_eq!(calls.get(), 1, "the session should have been kept");

        let reads = Cell::new(0);
        let lapsed_once = |_: &&'static str| {
            reads.set(reads.get() + 1);
            if reads.get() == 1 {
                Err(AppError::Auth("session expired".into()))
            } else {
                Ok(7)
            }
        };
        assert_eq!(state.read(stored("p"), &login, lapsed_once).unwrap(), 7);
        assert_eq!(calls.get(), 2);
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn a_session_refused_twice_does_not_loop() {
        let calls = Cell::new(0);
        let mut state = SignIn::default();
        let login = login_with(&calls, |_| Ok("session"));
        let reads = Cell::new(0);
        let always_lapsed = |_: &&'static str| -> Result<u32, AppError> {
            reads.set(reads.get() + 1);
            Err(AppError::Auth("session expired".into()))
        };
        assert!(state.read(stored("p"), &login, always_lapsed).is_err());
        assert_eq!(calls.get(), 2);
        assert_eq!(reads.get(), 2);
    }
}

#[test]
fn a_bad_argument_reaches_no_portal() {
    let portal = Fixtures::new();
    let calls = portal.calls.clone();
    let t = tools_over(portal, None);
    for (tool, args) in [
        ("fpl_bills", json!({"since": "yesterday"})),
        ("fpl_payments", json!({"limit": 0})),
        ("fpl_usage", json!({"nope": 1})),
    ] {
        let result = call(&t, tool, args);
        assert!(result.text().starts_with("usage:"), "{}", result.text());
    }
    assert!(calls.borrow().is_empty());
}

#[test]
fn the_environment_password_comes_before_the_keychain_and_empty_counts_as_unset() {
    let never = || -> Result<Option<Secret>, AppError> { panic!("the keychain was read") };
    let got = password_from(Some("from-env".into()), never)
        .unwrap()
        .unwrap();
    assert_eq!(got.expose(), "from-env");

    let got = password_from(Some(String::new()), || Ok(Some(Secret::new("stored"))))
        .unwrap()
        .unwrap();
    assert_eq!(got.expose(), "stored");
    assert!(password_from(None, || Ok(None)).unwrap().is_none());
}

#[test]
fn a_session_with_no_username_is_an_auth_error_naming_no_other_command() {
    let portal = LivePortal::new(Some(String::new()));
    let err = Portal::accounts(&portal).unwrap_err();
    assert!(matches!(err, AppError::Auth(_)), "{err:?}");
    assert!(!err.to_string().contains("fpl init"), "{err}");
}

/// Every identity-bearing value in the fixtures is a placeholder.
#[test]
fn the_fixtures_hold_only_placeholder_identifiers() {
    let f = Fixtures::new();
    let number = f.detail["data"]["accountNumber"].as_str().unwrap();
    assert_eq!(number, "1234567890");
    let address = f.detail["data"]["serviceAddress"].as_str().unwrap();
    assert!(address.contains("Example Street"), "{address}");
    for raw in [
        include_str!("../../tests/fixtures/bill_history.json"),
        include_str!("../../tests/fixtures/account_history.json"),
    ] {
        assert!(!raw.contains('@'), "an email in a fixture");
    }
}
