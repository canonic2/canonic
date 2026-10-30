# Local MCP library checks. Run `make` to list targets.
CARGO ?= $(if $(wildcard $(HOME)/.cargo/bin/cargo),$(HOME)/.cargo/bin/cargo,$(shell command -v cargo 2>/dev/null))
CARGO_FLAGS ?=

# Development keeps Cargo's incremental artifacts in the existing target directory.
TEST_TARGET ?= --lib
.PHONY: dev test-focus

dev: ## Build incrementally without release optimization or installation
	"$(CARGO)" build --locked --all-features $(CARGO_FLAGS)

test-focus: ## Run matching library tests; FILTER is required, TEST_TARGET selects other targets
	$(if $(strip $(FILTER)),,$(error Set FILTER to a test name or module; optionally set TEST_TARGET='--test integration'))
	$(if $(filter -%,$(FILTER)),$(error FILTER must be a test-name substring, not a flag))
	"$(CARGO)" nextest run --locked --all-features $(TEST_TARGET) $(CARGO_FLAGS) "$(FILTER)"

.DEFAULT_GOAL := help
.PHONY: help build coverage

help: ## List local targets
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  %-24s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

build: ## Build the library with all features
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" build --release --locked --all-features $(CARGO_FLAGS)

.PHONY: format format-check lint types test doctest docs deps

format: ## Apply Rust formatting
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" fmt

format-check: ## Check Rust formatting without changing files
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" fmt --check

lint: ## Run strict Clippy
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" clippy --locked --all-targets --all-features $(CARGO_FLAGS) -- -D warnings

types: ## Type-check Rust targets
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" check --locked --all-targets --all-features $(CARGO_FLAGS)

test: ## Run Rust tests
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" nextest run --locked --all-features $(CARGO_FLAGS)

doctest: ## Run documentation examples
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" test --doc --locked --all-features $(CARGO_FLAGS)

docs: ## Generate local API reference HTML
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" doc --locked --all-features --no-deps --document-private-items $(CARGO_FLAGS)

deps: ## Audit dependency advisories, licenses and sources
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" deny check

coverage: ## Generate the LLVM coverage report in target/llvm-cov
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" llvm-cov nextest --locked --all-features --html $(CARGO_FLAGS)
