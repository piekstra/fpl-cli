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
  under the service name `piekstra.fpl` (a password stored by an older version
  under `fpl-cli` moves there on first read by a command). At runtime commands
  read the secret *only* from there; `fpl mcp` reads `$FPL_PASSWORD` first
  ([The MCP server](#the-mcp-server)). It is never written to a config file, log,
  or the command line.
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

## The MCP server

`fpl mcp` serves read-only tools to an MCP client (Claude Code, Claude Desktop) over
stdio, on this machine only; it opens no port unless run with `--http` (below). It
signs in to fpl.com with the stored password, as every command does, and keeps the
session in memory while it runs. A tool never prompts and never creates or changes
a keychain item: a password under the legacy `fpl-cli` service is read where it is,
and a missing or rejected password is an error asking the user to run `fpl auth
login` in a terminal. A password FPL has refused is not sent again by a running
server, so a stale one cannot lock the account; the next call that finds a different
password tries that.

The Claude Desktop extension (`.mcpb`) holds the username and password in Claude
Desktop's secure store (`user_config`, the password `sensitive`) and passes them to
its bundled binary as `$FPL_USERNAME` and `$FPL_PASSWORD`, so it reads no keychain
item for them.

`fpl mcp --http --port <PORT>` serves the same tools over HTTP for the user's own
Cloudflare Worker, which reaches it through the user's own Cloudflare Tunnel
(cli-common's `connector/`). It binds 127.0.0.1 only, with no flag for another
address, and refuses to start without a bearer token of 32 or more characters, from
`FPL_MCP_TOKEN` or the keychain item `mcp-http-token` (`piekstra.fpl`), which `fpl
mcp --new-token` makes from 256 random bits and replaces only with `--overwrite`.
**A copy of the token lives in your Cloudflare Worker**, as its `HOME_FPL_TOKEN`
secret. Every request must carry the token (compared in constant time) and a `Host`
of `127.0.0.1` or `localhost`, and must not carry an `Origin`; there are no CORS
headers. Only the read-only tools are served (all four are). The password stays on
your computer: the Worker sends tool calls and receives results, and never sees the
password or an FPL session.

**Tool results leave your computer on this path.** They hold your financial and
household data (balance, due date, bills, payments, electricity use, and your
service address where the account is named by it). They cross Cloudflare, through
your tunnel and your Worker, and reach Claude, which shows them in your
conversation. Who holds what, and what a leaked token can do, is in the connector's
[threat model](https://github.com/piekstra/cli-common/blob/main/connector/README.md#threat-model).

`fpl auth logout` removes the password and does not touch `mcp-http-token` or stop
`fpl mcp --http`. To revoke the connector's access, run `fpl mcp --new-token
--overwrite` and restart the server (`launchctl kickstart -k gui/$(id -u)/com.piekstra.fpl-mcp-http`);
the old token works until the restart. To stop serving, run `launchctl bootout
gui/$(id -u)/com.piekstra.fpl-mcp-http` and stop the tunnel.

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
