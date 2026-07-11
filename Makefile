# fpl-cli — task runner: build/test/lint/fmt/clean/install/deps, a cheap `smoke`
# check, and an aggregate `verify` gate. Thin wrappers over cargo so a green
# local run predicts a green CI run.

BIN := fpl
CARGO := cargo

.PHONY: all build release test lint fmt fmt-check clean install deps smoke verify audit

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

install:
	$(CARGO) install --path . --force

deps:
	$(CARGO) fetch

# Cheap sanity checks needing no config or network: version + top-level help.
smoke: release
	./target/release/$(BIN) --version
	./target/release/$(BIN) --help >/dev/null
	@for grp in init set-credential auth accounts bills payments usage history profile meter alerts lookup outages api update; do \
		./target/release/$(BIN) $$grp --help >/dev/null || exit 1; \
	done
	@echo "smoke ok"

# Dependency license/advisory gate (matches CI). Needs cargo-deny installed.
audit:
	$(CARGO) deny check

# Aggregate pre-push gate: a green run here predicts green CI.
verify: fmt-check lint test smoke
	@echo "verify ok"
