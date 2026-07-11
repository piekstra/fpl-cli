# Contributing to fpl-cli

Thanks for helping out! This is a small, focused CLI over the same
`www.fpl.com` JSON services the FPL website and mobile app use. FPL publishes
no official API, so endpoint behavior can change without notice.

## Before you start

- Open or comment on an issue describing the change.
- Branch from `main` (`feat/…`, `fix/…`, `docs/…`); don't push to `main` directly.
- Use [Conventional Commits](https://www.conventionalcommits.org/).

## Local checks (must pass)

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

## Ground rules

- **No secrets, tokens, account numbers, addresses, or other personal data** in
  code, tests, fixtures, commits, or issue attachments. The example account
  number `1234567890` is a placeholder — use it. CI runs `gitleaks`.
- Passwords belong in the OS keychain only. Never print, log, or serialize a
  credential; wrap sensitive strings in `secrets::Secret`.
- Keep parsing **best-effort**. FPL's response shapes drift and vary by account
  type; a missing field should degrade gracefully (fall back to `--json`), not
  panic.
- Keep it personal-scale. No features whose purpose is bulk collection or
  hammering FPL's endpoints.
- Be kind — see [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Adding or fixing an endpoint

New endpoints go in `src/client.rs`, mapped from FPL's own service registry
(`/data/serviceconfparameters.js`) where possible — see [`docs/api.md`](docs/api.md).
When you can confirm a response shape from a real (redacted) payload, add a
human-readable renderer in `src/output.rs`; otherwise leave it as pretty-JSON
passthrough and note it as unverified.

## Pull requests

Use the PR template and fill in the checklist. A maintainer (see
`.github/CODEOWNERS`) will review.

## License of contributions

By contributing, you agree your work is dual-licensed under **MIT OR Apache-2.0**.

## The CLI family & cli-common

This repo is part of a family of CLIs (fpl, xfin, lrfl, tojfl, …) that share a
surface spec and library crates: [piekstra/cli-common](https://github.com/piekstra/cli-common)
(**piekstra-cli/1**). Before adding anything reusable — output rendering,
secret handling, config storage, self-update, DTO shapes — check whether it
belongs in cli-common's `pk-cli-*` crates instead. Contributions of shared,
reusable pieces to cli-common are encouraged and preferred over per-repo
copies; consume them here as tag-pinned git dependencies.

Surface changes (new standard commands/flags, DTO fields, exit codes) start as
a spec change in cli-common's `DESIGN.md`.

On macOS, run cli-common's `scripts/setup-dev-signing.sh` once and build with
`make dev` so keychain "Always Allow" grants survive rebuilds.
