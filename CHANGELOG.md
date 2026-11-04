# Changelog

## v0.5.2 — 2026-11-03

- `auth logout --forget` removes the connector token `mcp-http-token`, and says what that does
  not end: a running `fpl mcp --http` keeps the token it started with. Every step of a forget is
  attempted, and every failure reported.
- The connector example moves to port 8740. `rhd` documents 8732, so the two could not run side
  by side. Nobody has deployed; a port you already chose keeps working.
- On Windows, a session too long for one credential is split and read back as one.
- cli-common v0.27.0, up from v0.24.0.

## v0.5.1 — 2026-11-03

- `fpl api` refuses a plain `http://` URL, except to this machine (exit 2). It sent the
  session JWT and cookies in the clear. An `https` URL to any host still works, since you
  named it.
- A redirect that leaves `https`, or goes to another host while the session JWT is in
  use, is now an error rather than followed, since reqwest keeps FPL's `jwttoken` header
  on every hop. `fpl outages` reads a public feed with no JWT, so only the `https` rule
  applies there.
- A network failure's message names the host, not the whole URL.
- cli-common v0.24.0, up from v0.21.0.

## v0.5.0 — 2026-11-03

- `fpl mcp`: read-only tools for Claude, the `utility/v1` set from cli-common's
  `pk-cli-utility`: `fpl_balance`, `fpl_bills`, `fpl_payments` and `fpl_usage`. They
  run over stdio for Claude Code and Claude Desktop, and with `--http` on 127.0.0.1
  behind a bearer token for Claude on the web and mobile through your own Cloudflare
  Worker and tunnel.
- Releases attach `fpl-<version>.mcpb`, a Claude Desktop extension, and a Windows
  x86_64 tarball.
- `fpl mcp` reads `$FPL_PASSWORD` before the keychain, so an extension can pass the
  password from its settings. Every other command still reads only the keychain.
- `fpl mcp` reads the config on each call, not at start.
- `self-update` says to restart a running `fpl mcp --http` and to install the new
  `.mcpb`.
- Sign-in: only a 401 from FPL's login is now exit 3. Any other failing status (a 5xx,
  a 429, a 403 from FPL's edge) is exit 5, since it says nothing about the credentials.
- cli-common v0.21.0, up from v0.5.0. Rust 1.89 or later.
