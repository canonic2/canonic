# Agent Kit

`canonic-agent-kit` owns native MCP registration for coding agents: selection,
review, install/repair/removal, formatting-preserving configuration edits and
recoverable ownership records. It is an unpublished Rust library, used through
local path dependencies.

Products supply their executable, launch arguments, tool defaults, project root
and trusted catalog-check arguments. The library never starts the registered
MCP server as part of editing configuration. Its preflight runs only the
consumer-supplied executable and catalog-check arguments.

The generic setup workflow uses TUI Kit's guided prompts. Explicit `--agent`
with `--dry-run --json` produces a review without writes or prompts; `--yes`
allows an explicitly selected change to apply without terminal confirmation.
Products may instead compose the public document and storage APIs with their
own policy and workflow.

- [Usage and ownership](docs/README.md)
- [Development and checks](docs/development.md)

The implementation and guides belong to the public repository. Internal specs,
agent instructions and labs stay private.

From the repository root, use individual Make targets:

```sh
make -C packages/agent-kit lock
make -C packages/agent-kit format
make -C packages/agent-kit test-focus FILTER=tests::
```

The toolchain is pinned to nightly-2026-04-16 (Rust 1.97). Tests use temporary
roots and do not edit the developer's agent configuration.
