# Using Agent Kit

For package tooling and test boundaries, see [Development and checks](development.md).

Add `canonic-agent-kit = { path = "../agent-kit" }` to a sibling Rust package.

For a product that inherits the agent host's current project directory,
`relocate_project(&context, &previous_root)` migrates an exactly owned
registration by removing its old absolute `cwd`. It preserves the launcher and
tool approvals, leaves the original copy's registration intact, and starts no
catalog process. Edited or unowned entries stay unchanged. Other launch changes
use the normal reviewed setup workflow. Reload the host after a migrated entry
is written.

## Review a registration

This example prints a proposed Codex registration without writing configuration,
prompting or starting a server. Add the dependency from the package README and
save this as your Rust application's `src/main.rs`:

```rust
use canonic_agent_kit::{Context, Options, Result, Tool, prepare};
use std::path::Path;

fn main() -> Result<()> {
    let project = std::env::args_os().nth(1).expect("pass a project directory");
    let context = Context::local(
        "acme",
        Path::new(&project),
        vec!["mcp".into(), "serve".into()],
    )?
    .with_tools(vec![Tool::new("acme_status", "Read service status", false)]);
    let options = Options {
        agents: vec!["codex".into()],
        dry_run: true,
        ..Options::default()
    };
    let plan = prepare(&context, &options)?;
    println!("{}", plan.review());
    Ok(())
}
```

Run `cargo run -- /path/to/acme-project`, selecting an existing project
directory. The JSON describes the project scope, configuration path, proposed
`acme mcp serve` entry and tool approval choices. Existing malformed or conflicting
configuration is refused. No configuration is applied by this example.

The example only prepares a review; it does not implement an MCP server.
For real installation, embed this flow in your installed server executable,
implement its declared launch arguments and catalog-check mode, and use `run`
for guided preflight and confirmation. If you compose `Plan::apply` yourself,
perform `check_catalog` and obtain authorization before applying; `prepare`
alone does neither. See the integration details below.

## Integrate your server

Construct `Context::local(product, root, arguments)` and supply the product's
tools through `with_tools`. `launch_command` can require a stable PATH name in
both scopes; otherwise project configuration uses the product name and user
configuration uses the installed executable path. `check_arguments` defaults
to `--check` and is appended to the trusted executable's launch arguments.

Set `Context.prompt_hook` to a native `UserPromptSubmit` group when your product
supplies prompt context, for example
`serde_json::json!({"hooks":[{"type":"command","command":"acme hook user-prompt","timeout":5}]})`.
Setup reviews and publishes it alongside the selected Claude Code and Codex
registrations. Claude hooks share its permission settings file; Codex hooks use
`hooks.json` beside its MCP configuration. Hub and Box definitions carry the equivalent
portable hook restricted to those two agents. Other hosts receive MCP only.
The consumer owns hook behavior, launch quoting, timeout and host compatibility.
Groups support `hooks` and an optional string `matcher`; handlers support
`type: "command"`, a nonempty `command`, an optional positive integer `timeout`
in seconds and an optional string `statusMessage`. Unsupported fields refuse
before publication rather than being dropped from a Hub or Box definition. Commands
cannot contain `{project}`, `{agent}`, `{event}` or `{command}`: Hub and Box expand
those tokens and native hosts do not, so the same group would run differently.
Reruns repair missing owned groups; removal preserves unrelated and pre-existing
equivalent groups. A matching command in a different group conflicts for review.
For Codex, an equal group in the same layer's inline `[[hooks.UserPromptSubmit]]`
in `config.toml` counts as pre-existing, so setup writes no second copy to
`hooks.json`. Removal leaves an emptied `UserPromptSubmit` list and a
`hooks.json` that setup created in place.
Writing a hook does not establish host approval or invocation.

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
authorization, optional project cwd on supported adapters, and exclusive install routes
(Hub alone, Box alone, or native agents alone). `Authorization::TerminalOnly` rejects `yes` and requires terminal
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
reason. The `hub` and `box` choices write a whole server definition to
`.hub/mcp/<product>.yml` or `.box/mcp/<product>.yml`; installation requires
that host's `config.yml` marker in the selected scope (a project's Hub
configuration may instead be in `.config/hub/`, which then holds the
definition), and removal can
withdraw an owned definition after the marker is gone. Where both markers
exist, the picker preselects Hub.
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
