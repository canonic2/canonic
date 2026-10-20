# rustup's proxy first: only it honours rust-toolchain.toml's pinned nightly.
CARGO ?= $(shell test -x "$(HOME)/.cargo/bin/cargo" && printf '%s' "$(HOME)/.cargo/bin/cargo" || command -v cargo)
CARGO_FLAGS ?=

.DEFAULT_GOAL := help
.PHONY: help build check coverage

help: ## List local targets
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  %-24s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

build: ## Build the local release executable
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" build --release --locked --all-features $(CARGO_FLAGS)

check: ## Run formatting, strict Clippy, nextest, doctests, docs and dependency checks
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" fmt --check
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" clippy --locked --all-targets --all-features $(CARGO_FLAGS) -- -D warnings
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" nextest run --locked --all-features $(CARGO_FLAGS)
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" test --doc --locked --all-features $(CARGO_FLAGS)
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" doc --locked --all-features --no-deps --document-private-items $(CARGO_FLAGS)
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" deny check

coverage: ## Generate the LLVM coverage report in target/llvm-cov
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" llvm-cov nextest --locked --all-features --html $(CARGO_FLAGS)
