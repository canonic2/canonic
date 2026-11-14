# Local Agent Kit checks. Run `make` to list targets.
CARGO ?= $(if $(wildcard $(HOME)/.cargo/bin/cargo),$(HOME)/.cargo/bin/cargo,$(shell command -v cargo 2>/dev/null))
CARGO_FLAGS ?=
FILTER ?=
TEST_TARGET ?= --lib
.DEFAULT_GOAL := help
.PHONY: help lock dev build format format-check test-focus test types lint doctest docs deps coverage prune

# Cargo never deletes old build output. Run prune on its own, while no build or
# test run uses target/: a running test may still launch an old test binary.
PRUNE_DAYS ?= 3
prune: ## Delete incremental caches and test binaries unused for more than PRUNE_DAYS days
	@test ! -d target || find target -mindepth 3 -maxdepth 4 -type d -path '*/incremental/*' -prune -mtime +$(PRUNE_DAYS) -exec rm -rf {} +
	@test ! -d target || find target -mindepth 3 -maxdepth 4 -type f -path '*/deps/*' -perm -u+x ! -name '*.*' -mtime +$(PRUNE_DAYS) -delete

help:
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  %-24s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

lock: ## Resolve the dependency lockfile from the local cache
	"$(CARGO)" generate-lockfile --offline

dev: ## Build incrementally without release optimization or installation
	"$(CARGO)" build --locked --all-features $(CARGO_FLAGS)

build: ## Build the library with release optimization
	"$(CARGO)" build --release --locked --all-features $(CARGO_FLAGS)

format: ## Apply Rust formatting
	"$(CARGO)" fmt

format-check: ## Check Rust formatting
	"$(CARGO)" fmt --check

test-focus: ## Run matching tests; FILTER is required, TEST_TARGET defaults to --lib
	$(if $(strip $(FILTER)),,$(error Set FILTER to a test-name substring))
	$(if $(filter -%,$(FILTER)),$(error FILTER must be a test-name substring, not a flag))
	"$(CARGO)" nextest run --locked --all-features $(TEST_TARGET) $(CARGO_FLAGS) "$(FILTER)"

test: ## Run Rust tests
	"$(CARGO)" nextest run --locked --all-features $(CARGO_FLAGS)

types: ## Check all library targets
	"$(CARGO)" check --locked --all-targets --all-features $(CARGO_FLAGS)

lint: ## Run strict Clippy
	"$(CARGO)" clippy --locked --all-targets --all-features $(CARGO_FLAGS) -- -D warnings

doctest: ## Run documentation examples
	"$(CARGO)" test --doc --locked --all-features $(CARGO_FLAGS)

docs: ## Generate local API reference HTML
	"$(CARGO)" doc --locked --all-features --no-deps --document-private-items $(CARGO_FLAGS)

deps: ## Audit dependency advisories, licenses and sources
	"$(CARGO)" deny check

coverage: ## Generate the LLVM coverage report in target/llvm-cov
	"$(CARGO)" llvm-cov nextest --locked --all-features --html $(CARGO_FLAGS)
