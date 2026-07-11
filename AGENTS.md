# AGENTS.md — fpl-cli

Canonical agent entrypoint for this repo. `CLAUDE.md` is a one-line pointer here.

## What this is

A single-binary CLI (`fpl`) over Florida Power & Light's **undocumented**
`www.fpl.com` JSON services — account info, billing, usage, payments, outages.
No official API exists. Design ergonomics: a verb command surface, keychain-only
runtime secrets with stdin/env ingress, text-primary output, and stable exit
codes.

The endpoint map and auth flow are in [`docs/api.md`](docs/api.md) — read it
before touching `src/client.rs`.

## Local map

| Path | Responsibility |
|------|----------------|
| `src/main.rs` | thin entrypoint; parses args, dispatches |
| `src/cli.rs` | `clap` command tree (verbs + args) |
| `src/commands/*.rs` | one handler module per resource group (accounts, bills, payments, usage, history, profile, meter, alerts, lookup, outages, …) |
| `src/client.rs` | `Fpl` HTTP client: login, session, endpoints, raw `request` |
| `src/secrets.rs` | `Secret` (redacting/zeroizing) + `CredentialStore` + ingress |
| `src/config.rs` | `~/.config/fpl-cli/config.json` (non-secret prefs) |
| `src/output.rs` | text rendering + control-plane JSON |
| `src/error.rs` | `AppError` + exit codes |
| `src/commands/update.rs` | `fpl update`: self-update from GitHub Releases |
| `src/dates.rs` | minimal date math (no calendar crate) |
| `build.rs` | bakes the target triple into `FPL_TARGET` for `update` |

## Durable conventions (do not drift)

- **Verb language.** Resource groups take fixed verbs: `list`, `get`, `use`,
  `create`. Domain reads that name a precise FPL concept are allowed
  (`bills projected`, `accounts balance`). Don't coin a verb where a table verb
  fits; don't collapse `accounts list` to bare `accounts`.
- **Secrets: keychain-only at runtime.** The password is read only from the OS
  keychain. It gets there via `fpl init` / `fpl set-credential`, which ingest
  from `--stdin` or `--from-env <VAR>` — **never a `--value`/`--password` flag**
  (leaks to `ps`, history, transcripts). Credential replacement uses
  `--overwrite`. Wrap secrets in `secrets::Secret`; never log or serialize one.
- **Mutation safety.** `payments create` moves money — confirm by default, skip
  with `--force` (NOT `--yes`). A non-TTY run without `--force` fails with a hint.
- **Output: text is primary.** Resource reads render `Key: value` blocks and
  pipe-delimited (`ALL_CAPS`) tables. JSON is control-plane only
  (`init`/`set-credential` results, `auth status --json`, and the raw `api`
  payload). Do **not** add `--json` to resource reads. Data → stdout,
  diagnostics/confirmations → stderr.
- **Exit codes are a contract:** `0` ok, `1` other/keychain, `2` usage, `3`
  auth, `4` not found, `5` network. See `error.rs`.
- **Best-effort parsing.** FPL shapes vary by account type and drift. Never
  `unwrap()` on a response field; `output::render` flattens unknown shapes.

## This repo is public

Never commit personal data — no real account number, service address, email,
cookie, JWT, or password, in code, tests, fixtures, comments, or commit
messages. Use the placeholder account `1234567890`. CI runs `gitleaks`.

## Local checks (must pass before pushing)

```sh
make verify        # fmt-check + clippy -D warnings + test + smoke
make audit         # cargo-deny (licenses/advisories), matches CI
```

`make smoke` and `fpl outages list` need no login and exercise the binary
end-to-end. Authenticated paths can't run in CI; keep their logic covered by
unit tests on the pure helpers and verify manually with `fpl api`.
