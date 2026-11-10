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
# The apply-cost regression may clone only immutable pack artifacts from this
# optional enrolled project into its test-owned fixture; the source is read-only.
APPLY_PROFILE_PROJECT ?=
PREVIOUS_BINARY ?=
.PHONY: dev test-focus

dev: ## Build incrementally without release optimization or installation
	"$(CARGO)" build --locked $(CARGO_FLAGS)

test-focus: ## Run matching library tests; FILTER is required, TEST_TARGET selects other targets
	$(if $(strip $(FILTER)),,$(error Set FILTER to a test name or module; optionally set TEST_TARGET='--test integration'))
	$(if $(filter -%,$(FILTER)),$(error FILTER must be a test-name substring, not a flag))
	$(if $(strip $(APPLY_PROFILE_PROJECT)),SHIELD_APPLY_PROFILE_PROJECT="$(APPLY_PROFILE_PROJECT)") $(if $(strip $(PREVIOUS_BINARY)),SHIELD_PREVIOUS_BINARY="$(PREVIOUS_BINARY)") "$(CARGO)" nextest run --locked $(TEST_TARGET) $(CARGO_FLAGS) "$(FILTER)"

.PHONY: test-adapter-focus
test-adapter-focus: dev ## Run one named Node adapter/SDK test against the current development binary
	$(if $(strip $(FILTER)),,$(error Set FILTER to a test-name pattern))
	$(if $(filter -%,$(FILTER)),$(error FILTER must be a test-name pattern, not a flag))
	$(if $(strip $(TEST_FILE)),,$(error Set TEST_FILE to an adapter test file))
	SHIELD_TEST_BINARY="$(CURDIR)/target/debug/shield" node --test --test-concurrency=1 --test-name-pattern="$(FILTER)" "$(TEST_FILE)"

.PHONY: test-native-fixture
test-native-fixture: ## Run local native-fixture process cleanup regressions; no agent/provider or Rust build
	node --test --test-concurrency=1 adapters/fixtures/native-command.test.mjs

.PHONY: native-acceptance-preflight test-native-acceptance test-native-acceptance-frozen
native-acceptance-preflight: ## Inspect one isolated installed native host; NATIVE_HOST is copilot or opencode
	$(if $(filter 1,$(words $(NATIVE_HOST))),,$(error Set NATIVE_HOST to one of copilot or opencode))
	$(if $(filter copilot opencode,$(NATIVE_HOST)),,$(error Set NATIVE_HOST to copilot or opencode))
	SHIELD_NATIVE_ACCEPTANCE_HOST="$(NATIVE_HOST)" node --test --test-concurrency=1 --test-name-pattern="native acceptance preflight" adapters/sdk-tests/generated-native.test.mjs

test-native-acceptance: dev ## No-cost generated callbacks on this Mac; requires one NATIVE_HOST and loopback/PTY/process-group scope
	$(if $(filter 1,$(words $(NATIVE_HOST))),,$(error Set NATIVE_HOST to one of copilot or opencode))
	$(if $(filter copilot opencode,$(NATIVE_HOST)),,$(error Set NATIVE_HOST to copilot or opencode))
	SHIELD_TEST_BINARY="$(CURDIR)/target/debug/shield" SHIELD_NATIVE_ACCEPTANCE_HOST="$(NATIVE_HOST)" node --test --test-concurrency=1 --test-name-pattern="generated native callback acceptance" adapters/sdk-tests/generated-native.test.mjs

test-native-acceptance-frozen: ## Same native fixtures against an explicit frozen NATIVE_BINARY, without compiling changed sources
	$(if $(filter 1,$(words $(NATIVE_HOST))),,$(error Set NATIVE_HOST to one of copilot or opencode))
	$(if $(filter copilot opencode,$(NATIVE_HOST)),,$(error Set NATIVE_HOST to copilot or opencode))
	$(if $(filter /%,$(NATIVE_BINARY)),,$(error Set NATIVE_BINARY to the absolute path of the retained executable))
	$(if $(filter -%,$(FILTER)),$(error FILTER must be a test-name pattern, not a flag))
	SHIELD_TEST_BINARY="$(NATIVE_BINARY)" SHIELD_NATIVE_ACCEPTANCE_HOST="$(NATIVE_HOST)" node --test --test-concurrency=1 --test-name-pattern="$(if $(strip $(FILTER)),$(FILTER),generated native callback acceptance)" adapters/sdk-tests/generated-native.test.mjs

HISTORY_VOLUME_COUNT ?= 500000
HISTORY_VOLUME_REPORT_DIR ?= $(CURDIR)/labs/history-volume
.PHONY: history-volume
history-volume: ## Measure the isolated shipping history collector; COUNT 500 calibrates, 500000 measures volume
	SHIELD_HISTORY_VOLUME_COUNT="$(HISTORY_VOLUME_COUNT)" SHIELD_HISTORY_VOLUME_REPORT_DIR="$(HISTORY_VOLUME_REPORT_DIR)" "$(CARGO)" nextest run --locked --lib $(CARGO_FLAGS) --profile history-volume --run-ignored only history_volume_campaign

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
