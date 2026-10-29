# Local Shield build and installation. Run `make` to list targets.
CARGO ?= $(if $(wildcard $(HOME)/.cargo/bin/cargo),$(HOME)/.cargo/bin/cargo,$(shell command -v cargo 2>/dev/null))
CARGO_FLAGS ?=
.PHONY: format format-check lint types test doctest docs deps

format: ## Apply Rust formatting; FORMAT_FILES selects files and their child modules
	$(if $(strip $(FORMAT_FILES)),"$(dir $(CARGO))rustfmt" --edition 2024 $(FORMAT_FILES),PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" fmt)

format-check: ## Check Rust formatting without changing files
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" fmt --check

lint: ## Run strict Clippy
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" clippy --locked --all-targets $(CARGO_FLAGS) -- -D warnings

types: ## Type-check Rust targets
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" check --locked --all-targets $(CARGO_FLAGS)

test: ## Run Rust tests
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" nextest run --locked $(CARGO_FLAGS)

doctest: ## Run documentation examples
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" test --doc --locked $(CARGO_FLAGS)

docs: ## Generate local API reference HTML
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" doc --locked --no-deps --document-private-items $(CARGO_FLAGS)

deps: ## Audit dependency advisories, licenses and sources
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" deny check

# Development keeps Cargo's incremental artifacts in the existing target directory.
TEST_TARGET ?= --lib
.PHONY: dev test-focus

dev: ## Build incrementally without release optimization or installation
	"$(CARGO)" build --locked $(CARGO_FLAGS)

test-focus: ## Run matching library tests; FILTER is required, TEST_TARGET selects other targets
	$(if $(strip $(FILTER)),,$(error Set FILTER to a test name or module; optionally set TEST_TARGET='--test integration'))
	$(if $(filter -%,$(FILTER)),$(error FILTER must be a test-name substring, not a flag))
	"$(CARGO)" nextest run --locked $(TEST_TARGET) $(CARGO_FLAGS) "$(FILTER)"
PREFIX ?= $(HOME)/.local

.DEFAULT_GOAL := help
.PHONY: help build build-debug install-local install-local-debug

help: ## List the targets
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  %-24s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

build: ## Build Shield's local release binary
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" build --target-dir target --release --locked $(CARGO_FLAGS)

build-debug: ## Build with debug symbols and automatic diagnostic logging
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" build --profile diagnostic --target-dir target --locked $(CARGO_FLAGS)

install-local-debug: ## Install a debug build with automatic diagnostic logging
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" install --profile diagnostic --path . --target-dir target --locked --force --root "$(PREFIX)" $(CARGO_FLAGS)
	@if [ "$$(uname -s)" = Darwin ]; then codesign --sign - --force "$(PREFIX)/bin/shield"; fi

install-local: ## Build and install Shield into PREFIX/bin (default: ~/.local/bin)
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" install --path . --target-dir target --locked --force --root "$(PREFIX)" $(CARGO_FLAGS)
	@if [ "$$(uname -s)" = Darwin ]; then codesign --sign - --force "$(PREFIX)/bin/shield"; fi
