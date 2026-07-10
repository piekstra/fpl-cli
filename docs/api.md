# FPL API notes

FPL has no official public API. This documents the undocumented `www.fpl.com`
JSON services that `fpl-cli` talks to, so the mapping is auditable and the next
person doesn't have to re-derive it.

## How this was mapped

- FPL's account-management web app is an AMD/Dojo bundle. Its **service
  registry** lives at `https://www.fpl.com/data/serviceconfparameters.js` — a
  plain object of `{ name: { url, target } }` entries. That file is the source of
  truth for most paths below.
- The `serviceconfig` module composes account-scoped URLs as
  roughly `{url}/account/{account}{target}`.
- Auth flow and a few request bodies were cross-checked against the community
  Home Assistant integration ([`dotKrad/hass-fpl`](https://github.com/dotKrad/hass-fpl)).
- Reachability was confirmed by probing each path unauthenticated: a `401`
  (rather than `404`) means the route exists and requires a session.

Nothing here is guaranteed stable. FPL can change these at any time.

## Authentication (main region / "FL01")

1. **Login** — `GET /cs/customer/v1/registration/loginAndUseMigration?migrationToggle=Y&view=LoginMini`
   with HTTP **Basic** auth (`username:password`). On success (`200`), the
   response carries a **`jwttoken`** header and sets session cookies. On `401`,
   the JSON body has a `messageCode` — `NOTVALIDUSER` or `FAILEDPASSWORD`.
2. **Authenticated calls** — send the `jwttoken` header and reuse the cookie jar.
3. **Logout** — `GET /cs/customer/v1/registration/logout`.

Former **Gulf Power** accounts (Northwest Florida, "FL02") use an AWS Cognito
SRP login against `cognito-idp.us-east-1.amazonaws.com` instead, and a separate
`/cs/gulf/ssp/…` service tree. Not implemented here.

## Endpoints used by fpl-cli

All paths are under `https://www.fpl.com` and require the session unless noted.
`{account}` is the FPL account number; `{premise}` is the 9-digit zero-padded
premise number from account detail.

| Command | Method | Path |
|---------|--------|------|
| `accounts list` | GET | `/cs/customer/v1/resources/account?sortBy=status&count=10&start=1` |
| `accounts list` (fallback) | POST | `/cs/customer/v1/accountservices/resources/loginNew?mediaChannel=IOS` |
| `accounts get` | GET | `/cs/customer/v1/accountservices/resources/account/{account}/select?view=account-lander` |
| `accounts balance` | POST | `/cs/customer/v1/accountservices/resources/loginNew?mediaChannel=IOS` (balance fields per account) |
| `bills list` | GET | `/cs/customer/v1/sumbillaccount/resources/account/{account}/bill-history` |
| `bills projected` | GET | `/cs/customer/v1/accountservices/resources/account/{account}/projectedBill?premiseNumber={premise}&lastBilledDate={MMDDYYYY}` |
| `bills budget` | GET | `/cs/customer/v1/budgetbillingapi/resources/account/{account}/budgetBillingGraph` |
| `bills get` / `usage get` | POST | `/cs/customer/v1/energydashboard/resources/energy-usage/account/{account}/mobile-energy-service` |
| `usage hourly` | POST | `/cs/customer/v1/energydashboard/resources/energy-usage/account/{account}/mobile-hourly-usage` |
| `usage appliances` | POST | `/cs/customer/v1/energyanalyzer/resources/{account}/getDisaggResp` |
| `payments methods` | GET | `/cs/customer/v1/paymentservices/resources/account/{account}/payment-option` |
| `payments create` | POST | `/cs/customer/v1/paymentservices/resources/account/{account}/payment` |
| `payments list` / `history list --type account` | GET | `/cs/customer/v1/accounthistory/resources/account/{account}/account-history?count=25&start=1&sortBy=date` |
| `history list --type deposit` | GET | `/cs/customer/v1/accounthistory/resources/account/{account}/deposit-history?count=25&start=1&sortBy=date` |
| `outages list` | GET | `https://www.fplmaps.com/customer/outage/CountyOutages.json` *(public, no auth)* |

The `account-history` / `deposit-history` endpoints require the `count`, `start`,
and `sortBy` query parameters together — omit any and they return `400`. The
dedicated `/balance` endpoint has similar pagination requirements and is
inconsistent, so `accounts balance` reads the per-account balance fields
(`balance`, `actualBalance`, `dueDateVal`) off the `loginNew` list instead.
**Document retrieval is not yet mapped.** Both document history
(`/documentretrieval/…/document-history`, every path variant returns `404`) and
the bill-PDF download (`/documentretrieval/…/download`, returns `555 "No file
path available in DB"` for every `billDate` format) appear to need a document
reference the web app derives elsewhere. Until that's mapped, there's no
`history --type document` or `bills download`; `bills list` still gives every
bill's amounts, dates, and usage as text.

### Request-body notes

- **`mobile-energy-service`**: `{ status: "2", accountType: "RESIDENTIAL",
  premiseNumber, lastBilledDate: "MMDDYYYY", amrFlag: "Y", revCode: "1",
  meterNo }`. `status` `"2"` = active account. `lastBilledDate` and `meterNo`
  come from account detail (`currentBillDate`, `meterNo`).
- **`mobile-hourly-usage`**: `{ premiseNumber, startDate: "MM-DD-YYYY" }` (note
  the dashed date here, versus the undashed `MMDDYYYY` elsewhere).
- **`getDisaggResp`**: `{ premiseId, accountNumber }`.
- **`payment`**: `fpl-cli` sends `{ paymentAmount, paymentDate: "MM-DD-YYYY",
  paymentMethod? }`. This body is **best-effort** — the exact schema FPL expects
  hasn't been confirmed against a live submission. Verify with a small amount, or
  use `fpl api` with a payload you've captured, before relying on it.

## Other known endpoints (not yet wrapped)

The service registry exposes much more than `fpl-cli` wraps today — automatic
bill pay (`/cs/customer/v1/api/automaticbillpayment/resources/program`),
paperless/eBill enrollment (`/cs/customer/v1/programenrollment/resources/ebill`),
Budget Billing enrollment (`/cs/customer/v1/budgetbillingapi/resources/programEnrollment`),
multi-account management (`/cs/customer/v1/multiaccount/…`), preferences,
outage reporting (`/cs/customer/v1/wors/public/…`), and more. Reach any of them
with `fpl api <METHOD> <PATH>` until a first-class command exists.

## Adding a first-class command

New endpoints go in `src/client.rs` as a method returning `serde_json::Value`,
wired to a verb in `src/cli.rs` and a handler in `src/commands/<group>.rs`. Reads
render through `output::render` (text); don't add `--json` to a read — `fpl api`
is the raw-JSON path. Once you can confirm a response shape from a real
(redacted) payload, add a purpose-built renderer in `src/output.rs`.
