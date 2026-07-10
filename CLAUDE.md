# CLAUDE.md — fpl-cli

Guidance for AI agents working in this repo.

## What this is

A single-binary Rust CLI (`fpl`) over Florida Power & Light's **undocumented**
`www.fpl.com` JSON services — account info, billing, usage, payments, outages.
No official API exists. Endpoint mapping and the auth flow are documented in
[`docs/api.md`](docs/api.md); read it before touching `src/client.rs`.

## Hard rules

- **This repo is public. Never commit personal data.** No real account numbers,
  service addresses, emails, cookies, JWTs, or passwords — in code, tests,
  fixtures, comments, or commit messages. Use the placeholder account
  `1234567890`. CI runs `gitleaks`.
- **Secrets only in the keychain.** Passwords go through `secrets::Secret` and
  the OS keychain via `secrets::CredentialStore`. Never log, print, serialize,
  or write a credential to disk. `~/.config/fpl-cli/config.json` holds
  non-secret preferences only.
- **Payments require confirmation.** `fpl pay make` must never submit without an
  interactive `[y/N]` or an explicit `--yes`. Don't weaken that.
- **Best-effort parsing.** FPL response shapes vary by account type and drift.
  Don't `unwrap()` on response fields; degrade to `--json` passthrough.

## Layout

| File | Responsibility |
|------|----------------|
| `src/main.rs` | Arg wiring, command handlers, credential/account resolution |
| `src/cli.rs` | `clap` command tree |
| `src/client.rs` | `Fpl` HTTP client: login, JWT/cookie session, endpoint methods, raw `request` |
| `src/secrets.rs` | `Secret` (redacting, zeroizing) + `CredentialStore` (keychain→env→prompt) |
| `src/config.rs` | `~/.config/fpl-cli/config.json` (non-secret prefs) |
| `src/output.rs` | Human + `--json` rendering |
| `src/dates.rs` | Minimal date math (no calendar-crate dependency) |
| `src/error.rs` | `AppError` + stable exit codes |

## Conventions

- Every command supports `--json` (stdout = data, stderr = diagnostics).
- Exit codes are a contract: `0` ok, `1` other, `2` usage, `3` auth, `4` not
  found, `5` network. See `error.rs` and the README table.
- New endpoints are added in `client.rs`, mapped from FPL's own service registry
  (`/data/serviceconfparameters.js`). Add a unit test for any pure helper.

## Local checks (must pass before pushing)

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

## Testing without an FPL account

The `outages` command hits the public feed and needs no login — use it to smoke
test the binary end-to-end. Authenticated paths can't be exercised in CI; keep
their logic covered by unit tests on the pure helpers (date/premise/field
extraction) and lean on `fpl api` for manual verification against a real account.
