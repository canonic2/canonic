CARGO ?= $(shell command -v cargo 2>/dev/null || printf '%s' "$(HOME)/.cargo/bin/cargo")
CARGO_FLAGS ?=

.DEFAULT_GOAL := help
.PHONY: help run build check coverage

help: ## List local targets
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  %-24s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Open the full-screen component gallery
	PATH="$(dir $(CARGO)):$$PATH" "$(CARGO)" run --locked --features gallery --bin tui-gallery $(CARGO_FLAGS)

build: ## Build the local library and release gallery
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
