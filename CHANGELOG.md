# Changelog

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
- `self-update` says to restart a running `fpl mcp --http` and to install the new
  `.mcpb`.
- Sign-in: only a 401 from FPL's login is now exit 3. Any other failing status (a 5xx,
  a 429, a 403 from FPL's edge) is exit 5, since it says nothing about the credentials.
- cli-common v0.21.0, up from v0.5.0. Rust 1.89 or later.
