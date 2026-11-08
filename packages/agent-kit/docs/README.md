# Using Agent Kit

For package tooling and test boundaries, see [Development and checks](development.md).

Add `canonic-agent-kit = { path = "../agent-kit" }` to a sibling Rust package.

Construct `Context::local(product, root, arguments)` and supply the product's
tools through `with_tools`. `launch_command` can require a stable PATH name in
both scopes; otherwise project configuration uses the product name and user
configuration uses the installed executable path. `check_arguments` defaults
to `--check` and is appended to the trusted executable's launch arguments.

`run(context, options, machine)` handles selection, review and confirmation.
Project scope is the default; user scope is explicit. `Options` supports
repeated agent IDs, removal, dry run and explicit `yes`. `--reset-tool-permissions`
starts from the consumer's current tool defaults instead of configured approvals.
It supports dry-run review, conflicts with removal and retains the normal
publication authorization. The permissions page labels saved selections.
Without explicit agents, reset skips the action picker and starts agent selection.
Explicit `Options.approved`, including an empty vector, can revise reset defaults.
`prepare` returns a `Plan` with `review` and `apply` when the product owns the
interaction instead.

Use `Context::local_at_root` when the consumer already selected an explicit root;
it canonicalizes that root without replacing it with an enclosing checkout.
`run_with_policy` and `prepare_with_policy` accept `SetupPolicy` for terminal-only
authorization, optional project cwd on supported adapters, and exclusive Box/native
install routes. `Authorization::TerminalOnly` rejects `yes` and requires terminal
acceptance for every applying action; the default `run` policy remains unchanged.
The optional catalog observer receives only the executable path and returns a
guard held through the shared bounded check, for tracing. It cannot replace
execution or turn an invalid catalog result into success.

Custom inspection can use `PreparationSession::new(context, scope, policy)`:
call `inspect` for typed host status and `prepare` with the same scope to share
file snapshots and prepared codecs. Drop the session after building the plan;
apply still reads fresh bytes. `catalog` supplies stable IDs/discovery names and
`discovery_defaults` accepts an explicit PATH snapshot without executing agents.

The host catalog covers Claude Code, Codex, Copilot in VS Code and its CLI,
Cursor, Gemini, Kilo, Kiro, Kimi, OpenCode, Qwen, Amp, Droid, Windsurf,
Cline, Augment and Goose. Unsupported automatic scopes return a manual-setup
reason. Box installation requires a configuration marker in the selected scope;
removal can withdraw an owned definition after that marker is gone.
Install/repair checks the trusted executable's catalog before publication. The
child has a 30-second deadline and a 16 MiB stdout limit; timeout kills and reaps
the direct child and reports an error without changing agent configuration.
Catalog stderr is discarded. The check must not start detached workers; this
does not provide process-tree containment.
Agent trust and reload requirements still apply after publication; writing
configuration does not prove a live MCP connection.

`document` edits JSON/JSONC, TOML and YAML entries and approval/hook lists.
JSON/JSONC and TOML preserve unrelated formatting. YAML is normalized to
equivalent JSON and the review discloses that conversion. Malformed or
ambiguous documents fail before publication.

`storage` provides bounded no-follow reads, path-identity locks and durable
staged replacement. Records retain the previous and desired entry for repair
and removal. Application rechecks reviewed bytes before publication and refuses
externally changed owned entries. Limits apply to complete host files (16 MiB).
Keep product-specific authority and host capability policy in the consumer.
