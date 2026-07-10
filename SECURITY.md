# Security policy

## Reporting a vulnerability

Please report security issues **privately** — do not open a public issue.

- Preferred: open a [GitHub private security advisory](https://docs.github.com/en/code-security/security-advisories)
  on this repo ("Security" tab → "Report a vulnerability").
- Or contact the maintainer (see `.github/CODEOWNERS`).

We aim to acknowledge within a few days and coordinate a fix/disclosure with you.

## How this tool handles your credentials

`fpl-cli` authenticates to your real FPL account, so credential handling is the
core security concern:

- **Password storage.** Your FPL password lives only in the OS-native secret
  store — macOS Keychain, Windows Credential Manager, or Linux Secret Service —
  under the service name `fpl-cli`. At runtime the CLI reads the secret *only*
  from there. It is never written to a config file, log, or the command line.
- **Ingress.** Getting the password into the keychain is a setup-time step:
  `fpl init` / `fpl set-credential` read it from stdin (`--stdin` /
  `--password-stdin`) or a named env var (`--from-env` / `--password-from-env`).
  There is deliberately **no `--password=<value>` flag** — flag values leak into
  `ps`, shell history, and pasted transcripts.
- **In memory.** Secrets are wrapped in a `Secret` type that redacts itself in
  `Debug`/`Display` output and is zeroized on drop. It is read only at the point
  of use.
- **On disk.** `~/.config/fpl-cli/config.json` holds only non-secret preferences
  (default username and active account number). No password, token, or cookie is
  persisted there.
- **Over the network.** All requests go to `https://www.fpl.com` (and the public
  `fplmaps.com` outage feed) over TLS. The session JWT and cookies from login
  live in memory for the duration of a single command and are not saved.

## No personal data in the repo

This repository must never contain a real account number, service address,
email, cookie, or credential — in code, tests, fixtures, or commits. Use the
placeholder account number `1234567890`. CI runs `gitleaks` to help enforce this.

## Payments

`fpl payments create` moves real money. It refuses to submit without an
interactive `[y/N]` confirmation or an explicit `--force`, and never stores
payment instrument details.

## Dependencies

CI runs `cargo audit` and `cargo deny` on every push and pull request.

## Supported versions

Pre-1.0: only the latest release receives fixes.
