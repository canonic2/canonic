# Agent Kit

Register your application's MCP server in coding agents' native configuration.

`canonic-agent-kit` gives Rust applications a shared setup flow: choose agents,
review configuration changes, and install, repair or remove the entries your
application owns. It preserves unrelated settings and keeps ownership records
for later repair and removal.

## Add the library

Agent Kit is unpublished and used from source. Consumers require Rust 1.97 or
newer; this checkout pins its own development toolchain. From a sibling Rust
package, add this to `Cargo.toml`:

```toml
[dependencies]
canonic-agent-kit = { path = "../agent-kit" }
```

Adjust the path to your checkout's `packages/agent-kit` directory.

## Set up your server

Start with the [review-only example](docs/README.md#review-a-registration) to
print a proposed Codex registration without changing configuration. For an
existing server, connect its launch arguments and tool defaults to the setup flow:

1. Construct a `Context` with your product name, project root and server arguments.
2. Supply the tools the setup screen should offer for approval.
3. Use `run` for guided agent selection and confirmation, or `prepare` to obtain
   a reviewable plan for your own interface.

The result is a reviewed native configuration entry for the selected agents.
Project scope is the default; user scope is explicit. When your CLI exposes the
shared options, `--agent codex --dry-run --json` returns a review without writes
or prompts. Applying requires confirmation or an explicit `--yes` under the
consumer's setup policy.

Guided install and repair preflight your trusted executable with the configured
catalog-check arguments. Your executable must support that check without
starting a long-lived server. Agent trust and reload steps still apply after
configuration is written; publication alone does not establish a live connection.

## Choose the integration level

- **Guided setup:** reuse TUI Kit's selection, review and confirmation flow.
- **Your own interface:** prepare a plan, present its review and apply it through
  your application's authorization flow.
- **Configuration primitives:** use the document codecs and storage APIs when
  your application owns the complete workflow.

The [usage guide](docs/README.md) covers supported agents and scopes, tool
permission reset, formatting behavior, ownership and conflict recovery.

## Development

From the repository root, `make -C packages/agent-kit` lists available targets.
[Development and checks](docs/development.md) explains focused verification and
temporary configuration fixtures.

## License

Agent Kit is subject to the [Canonic Proprietary License](../../LICENSE).
Third-party components retain their own licenses and notices.
