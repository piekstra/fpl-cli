<!-- Keep PRs focused. Link the issue: Closes #___ -->

## What & why

## Type

- [ ] Feature
- [ ] Bug fix
- [ ] Docs
- [ ] Refactor / chore

## Checks

- [ ] `cargo fmt --all` clean
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo test` passes
- [ ] If RedSky parsing changed: added/updated a unit test in `src/product.rs`
- [ ] README / `--help` updated if behavior changed

## Security

- [ ] No secrets, tokens, or real account data added (code, tests, commits)
- [ ] Still credential-free and personal-scale / ToS-respecting

## Family / cli-common

- [ ] No shared/reusable behavior copied in that belongs in [cli-common](https://github.com/piekstra/cli-common) (`pk-cli-*`)
- [ ] Surface, DTO, or exit-code changes reflected in cli-common `DESIGN.md` / `conformance.md`
