# fpl-cli — task runner: build/test/lint/fmt/clean/install/deps, a cheap `smoke`
# check, and an aggregate `verify` gate. Thin wrappers over cargo so a green
# local run predicts a green CI run.

BIN := fpl
CARGO := cargo

.PHONY: all build release test lint fmt fmt-check clean install deps smoke verify audit dev

all: verify

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test --all

lint:
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clean:
	$(CARGO) clean

# `cargo install` ad-hoc signs, which gives the binary a *new* code identity
# every time. macOS scopes keychain "Always Allow" grants to that identity, so
# an unsigned reinstall silently revokes them and the next run prompts again.
# Re-signing with the stable shared identity keeps one grant valid across every
# future install.
install: SIGN_TARGET = $${CARGO_INSTALL_ROOT:-$$HOME/.cargo}/bin/$(BIN)
install:
	$(CARGO) install --path . --force
	@$(SIGN)

deps:
	$(CARGO) fetch

# Cheap sanity checks needing no config or network: version + top-level help.
smoke: release
	./target/release/$(BIN) --version
	./target/release/$(BIN) --help >/dev/null
	@for grp in init set-credential auth config accounts bills payments usage history profile meter alerts lookup outages api update; do \
		./target/release/$(BIN) $$grp --help >/dev/null || exit 1; \
	done
	@echo "smoke ok"

# Dependency license/advisory gate (matches CI). Needs cargo-deny installed.
audit:
	$(CARGO) deny check

# Aggregate pre-push gate: a green run here predicts green CI.
verify: fmt-check lint test smoke
	@echo "verify ok"

# Debug build re-signed with the same stable pk-cli-codesign identity, so the
# dev loop doesn't re-prompt either (see cli-common/scripts).
dev: SIGN_TARGET = target/debug/$(BIN)
dev:
	cargo build
	@$(SIGN)

# Shared re-signing step. No-ops with a note when the helper or identity is
# absent (CI, Linux, a fresh machine that hasn't run setup-dev-signing.sh).
define SIGN
if [ -x "$$HOME/Dev/cli-common/scripts/dev-sign.sh" ]; then \
	"$$HOME/Dev/cli-common/scripts/dev-sign.sh" "$(SIGN_TARGET)"; \
else echo "cli-common/scripts/dev-sign.sh not found — $(SIGN_TARGET) left ad-hoc signed"; fi
endef
