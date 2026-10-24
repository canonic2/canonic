# canonic-mcp

A Rust library for Model Context Protocol servers. It has two layers:

- An I/O-free protocol core: strict JSON-RPC framing, the initialization
  lifecycle, a validated tool catalog, argument validation, tool results,
  cancellation and host confirmation requests. Any runtime can drive it.
- A synchronous stdio driver that runs a server's tools on worker lanes with
  deadlines, per-key ordering, progress, confirmation and end-of-input rules.

Servers depend on it through a local path dependency; it is not published.

```toml
[dependencies]
canonic-mcp = { path = "../mcp" }
```

A server supplies its catalog, identity and tool implementations:

```rust,ignore
use canonic_mcp::{stdio, Catalog, Client, Content, RpcError, ServerInfo, Tool, ToolResult};
use std::sync::Arc;

struct Acme;

impl stdio::Handler for Acme {
    fn initialize(&self, _client: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1.0.0").with_instructions("Use acme_status for service state."))
    }

    fn call(&self, _request: stdio::Request, _context: &stdio::Context) -> stdio::Outcome {
        stdio::Outcome::Tool(ToolResult::success(vec![Content::text("Acme is running.")]))
    }
}
```

The library decides framing, lifecycle, deadlines, cancellation and
disconnection. The server owns its tools, their authority, its instructions and
the wording of its results. A host confirmation reports what the host's user
chose; the library never grants authority.

- [Documentation](docs/README.md): guides and reference, organized by task.
- [Quick start](docs/getting-started.md): serve a first tool over stdio.
- [Development](docs/contributing/development.md): checks, tests and tools.
- [Specifications](specs/README.md): the contracts behind each module.

Select checks individually from this directory: `make format-check`, `make lint`,
`make types`, `make test`, `make doctest`, `make docs` or `make deps`.
See [development](docs/contributing/development.md) for their purposes and costs.
