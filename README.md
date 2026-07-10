# fpl-cli

Manage your **Florida Power & Light** account from the command line: check your
balance, view and download bills, inspect energy usage, make a payment, and
watch outages — all from a terminal, with a `--json` mode on every command so a
person *or* an agent can script it.

FPL publishes no official public API. This is a polite client over the same
`www.fpl.com` JSON services that FPL's own website and mobile app call. Endpoint
paths were mapped from FPL's public service registry and cross-checked against
the community Home Assistant integration — see [`docs/api.md`](docs/api.md).

> **Status: work in progress.** The public **outage** feed needs no login and is
> fully verified. Authenticated commands target real, confirmed-reachable
> endpoints, but FPL's response *shapes* vary by account type and aren't all
> pinned down yet, so most authenticated commands print the raw JSON payload for
> now. Use `--json` (or `fpl api`) to see exactly what came back, and please open
> an issue with a **redacted** snippet if a command's output looks off.

## Install

```sh
cargo build --release          # binary at ./target/release/fpl
# or
cargo install --path .         # installs the `fpl` binary onto your PATH
```

The crate/repo is `fpl-cli`; the binary is **`fpl`**. Requires a recent stable
Rust toolchain. macOS, Windows, and Linux are supported (each uses its native
secret store).

## Getting started

```sh
# Store your fpl.com credentials in the OS keychain and verify them.
# Prompts (no echo) for the password; remembers your username + default account.
$ fpl login
FPL username (email): you@example.com
FPL password for you@example.com: ********
logged in as you@example.com; default account 1234567890

# What's configured? (never prints the password)
$ fpl status
username:  you@example.com
account:   1234567890
password:  stored in keychain
```

Your **password lives only in the OS keychain** (macOS Keychain / Windows
Credential Manager / Linux Secret Service). Your username and default account
number are cached in `~/.config/fpl-cli/config.json` (no secrets). Nothing
authenticates over the network except the login call itself.

## Commands

### Account

```sh
fpl accounts          # list the accounts on your login
fpl account           # service address, meter, bill cycle, enrolled programs
fpl balance           # current balance and due date
```

### Billing

```sh
fpl bill current      # this period: projected bill, bill-to-date, daily average
fpl bill projected    # projected end-of-cycle bill
fpl bill history      # prior bills (amounts, dates, usage)
fpl bill budget       # Budget Billing plan status + monthly graph
fpl bill download --out my-bill.pdf   # download your latest bill PDF
```

### Payments

```sh
fpl pay methods       # saved payment methods / options
fpl pay history       # payments from the account ledger
fpl pay make --amount 123.45              # make a payment (asks to confirm)
fpl pay make --amount 123.45 --date 08-01-2026 --method <id> --yes
```

`fpl pay make` **will not submit without confirmation** — it prompts `[y/N]`, or
requires `--yes` in a non-interactive shell. Money movement is hard to reverse,
so treat `--yes` as the explicit go-ahead.

### Usage

```sh
fpl usage summary     # current-period kWh, projected cost, daily average
fpl usage hourly --date 07-04-2026    # hourly kWh for one day (default: yesterday)
fpl usage appliances  # appliance-level (disaggregated) breakdown
```

### History

```sh
fpl history account   # transactions: charges, payments, adjustments
fpl history deposit   # deposit history
fpl history documents # documents available to download
```

### Outages (no login required)

```sh
$ fpl outages --county broward
County                        Out           Served
-------------------- ------------ ----------------
Broward                        70          853,654
-------------------- ------------ ----------------
TOTAL OUT                      70

fpl outages                   # all counties FPL serves
fpl outages --county miami --json
```

### Raw API escape hatch

Anything not yet wrapped in a subcommand — hit it directly with your session:

```sh
fpl api GET /cs/customer/v1/resources/header
fpl api POST /cs/customer/v1/accountservices/resources/loginNew?mediaChannel=IOS --data '{}'
```

Paths are relative to `https://www.fpl.com` (or pass a full URL). Output is
always JSON.

## JSON & scripting

Every command takes `--json` (machine output on stdout; diagnostics on stderr):

```sh
# Total customers out across all FPL counties:
fpl outages --json | jq '[.[] | (.["Customers Out"] | gsub(",";"") | tonumber)] | add'

# Your current balance amount:
fpl balance --json | jq -r '.data.amount // .data.balance'
```

### Global flags

| Flag | Meaning |
|------|---------|
| `--json` | Machine-readable JSON on stdout |
| `-v, --verbose` | Extra diagnostics on stderr (never secrets) |
| `-q, --quiet` | Suppress non-error stderr output |
| `--username <email>` | Override login (else config → `$FPL_USERNAME` → prompt) |
| `--account <number>` | Override account (else config → first account) |

### Environment variables

| Var | Purpose |
|-----|---------|
| `FPL_USERNAME` | Default login email |
| `FPL_PASSWORD` | Password (skips the keychain/prompt — handy for CI) |
| `FPL_ACCOUNT` | Default account number |

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Unexpected / keychain error |
| `2` | Usage error (bad input, unconfirmed payment) |
| `3` | Authentication required or rejected |
| `4` | Not found (unknown account, empty result) |
| `5` | Network / upstream error |

## Regional support

Works for standard FPL (**"main region"**) accounts. Former **Gulf Power**
accounts in Northwest Florida use a different AWS Cognito login flow and aren't
supported yet — contributions welcome.

## Disclaimer

Unofficial and not affiliated with, endorsed by, or supported by Florida Power &
Light. It uses undocumented endpoints that can change or break at any time. Use
it with your own account, at personal scale, and within FPL's Terms of Service.
No warranty — see the license.

## License

Dual-licensed under **MIT OR Apache-2.0**. See [`LICENSE-MIT`](LICENSE-MIT) and
[`LICENSE-APACHE`](LICENSE-APACHE).
