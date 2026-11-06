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
repeated agent IDs, removal, dry run and explicit `yes`. `prepare` returns a
`Plan` with `review` and `apply` when the product owns the interaction instead.

The host catalog covers Claude Code, Codex, Copilot in VS Code and its CLI,
Cursor, Gemini, Kilo, Kiro, Kimi, OpenCode, Qwen, Amp, Droid, Windsurf,
Cline, Augment and Goose. Unsupported automatic scopes return a manual-setup
reason. Box definitions are available when the selected scope has a Box config.
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
