# fpl-cli

Manage your **Florida Power & Light** account from the command line: check your
balance, view and download bills, inspect energy usage, make a payment, and
watch outages — from a terminal, with stable exit codes and text output a person
*or* an agent can parse.

FPL publishes no official public API. This is a polite client over the same
`www.fpl.com` JSON services that FPL's own website and mobile app call. It
follows a small set of ergonomics: a fixed **verb** command surface,
**keychain-only** runtime secrets with stdin/env ingress, **text-first** output,
and a stable exit-code contract. Endpoint mapping is in
[`docs/api.md`](docs/api.md).

> **Status: work in progress.** The public **outage** feed needs no login and is
> fully verified. Authenticated commands target real, confirmed-reachable
> endpoints, but FPL's response *shapes* vary by account type and aren't all
> pinned down yet — reads render a best-effort flattened view of whatever comes
> back. For the raw payload (and to help map a shape), use `fpl api`.

## Install

```sh
make install          # cargo install --path . → the `fpl` binary on your PATH
# or
cargo build --release # binary at ./target/release/fpl
```

The crate/repo is `fpl-cli`; the binary is **`fpl`**. Requires a recent stable
Rust toolchain. macOS, Windows, and Linux are supported (each uses its native
secret store).

## Getting started

Set up once. Your password is read from stdin or an env var and stored only in
the OS keychain — there is deliberately no password flag.

```sh
# Interactive first-time setup (prompts, no echo):
$ fpl init --username you@example.com
FPL password for you@example.com: ********
stored credentials for you@example.com; active account 1234567890

# Or fully scriptable (headless / CI / 1Password):
$ op read 'op://Private/FPL/password' | fpl init --username you@example.com --password-stdin
$ FPL_PW=... fpl init --username you@example.com --password-from-env FPL_PW

# What's configured? (never prints the password)
$ fpl auth status
username: you@example.com
account:  1234567890
password: stored in keychain
```

Rotate a stored password without re-running full setup:

```sh
op read 'op://Private/FPL/password' | fpl set-credential --stdin --overwrite
```

## Commands

The surface is resource groups + fixed verbs (`list`, `get`, `use`, `create`).
Most commands take an optional `[account-id]`; omit it to use the active account
(set with `accounts use`, or the first account on your login).

### Accounts

```sh
fpl accounts list                 # all accounts on your login
fpl accounts get [account-id]     # service address, meter, bill cycle, programs
fpl accounts use <account-id>     # set the active account for later commands
fpl accounts balance [account-id] # current balance and due date
```

### Bills

```sh
fpl bills get [account-id]        # this period: projected bill, bill-to-date, daily avg
fpl bills projected [account-id]  # projected end-of-cycle bill
fpl bills list [account-id]       # prior bills (amounts, dates, usage)
fpl bills budget [account-id]     # Budget Billing plan status + monthly graph
```

### Payments

```sh
fpl payments list [account-id]    # payments from the account ledger
fpl payments methods [account-id] # saved payment methods / options
fpl payments create --amount 123.45              # make a payment (asks to confirm)
fpl payments create --amount 123.45 --date 08-01-2026 --method <id> --force
```

`payments create` **will not submit without confirmation** — it prompts `[y/N]`,
or requires `--force` in a non-interactive shell. Money movement is hard to
reverse, so `--force` is the explicit go-ahead.

### Usage

```sh
fpl usage get [account-id]                 # current-period kWh, projected cost, daily avg
fpl usage hourly [account-id] --date 07-04-2026   # hourly kWh (default: yesterday)
fpl usage appliances [account-id]          # appliance-level (disaggregated) breakdown
```

### History

```sh
fpl history list [account-id]                 # account ledger (default --type account)
fpl history list [account-id] --type deposit  # deposit history
fpl history types                             # valid --type values
```

### Outages (no login required)

```sh
$ fpl outages list --name broward
COUNTY | OUT | SERVED
Broward | 70 | 853,654
TOTAL OUT | 70

fpl outages list                  # all counties FPL serves
```

### Raw API escape hatch

Anything not yet wrapped in a subcommand — or the raw JSON of one — with your
session:

```sh
fpl api GET /cs/customer/v1/resources/header
fpl api POST /cs/customer/v1/accountservices/resources/loginNew?mediaChannel=IOS --data '{}'
```

Paths are relative to `https://www.fpl.com` (or pass a full URL). Output is
always JSON.

### Updating

```sh
fpl update --check    # is a newer release available?
fpl update            # download the latest release for your platform and replace the binary
```

`fpl update` pulls the matching `fpl-<target>.tar.gz` from this repo's GitHub
Releases and swaps the running binary in place. (If you installed with
`cargo install`, `git pull && make install` also works.)

## Output & scripting

**Text is the default.** Resource reads render `Key: value` blocks and
pipe-delimited tables (`ALL_CAPS` headers) — token-dense and parseable without
JSON. JSON is reserved for control-plane signals: `init`/`set-credential`
results (`--json`), `auth status --json`, and `fpl api` (always JSON). For the
raw structure of a resource read, pipe it through `fpl api`.

Data goes to stdout; diagnostics and confirmations go to stderr.

```sh
# Total customers out across all FPL counties:
fpl api GET https://www.fplmaps.com/customer/outage/CountyOutages.json \
  | jq '[.outages[] | (.["Customers Out"] | gsub(",";"") | tonumber)] | add'
```

### Global flags

| Flag | Meaning |
|------|---------|
| `-a, --account <number>` | Account to act on (else active account → first account) |
| `--username <email>` | Login (else config → `$FPL_USERNAME`) |
| `-v, --verbose` | Extra diagnostics on stderr (never secrets) |
| `-q, --quiet` | Suppress non-error stderr output |

### Environment variables

| Var | Purpose |
|-----|---------|
| `FPL_USERNAME` | Default login email |
| `FPL_ACCOUNT` | Default account number |

Secrets never enter through an env var the CLI reads at runtime — pass them to
`init`/`set-credential` via `--password-from-env` / `--from-env` at setup time.

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
