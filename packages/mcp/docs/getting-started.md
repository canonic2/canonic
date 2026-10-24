# Quick start

## Add the dependency

The library is not published. Depend on it by path from a sibling package:

```toml
[dependencies]
canonic-mcp = { path = "../mcp" }
serde_json = "1.0"
```

It needs Rust 1.99 or later and has no features.

## Publish a tool

A `Catalog` is your validated tool list. Each tool has a name, one sentence on
what it returns or changes, and an object schema for its arguments:

```rust,ignore
use canonic_mcp::{Annotations, Catalog, Tool};
use serde_json::json;

let catalog = Catalog::new(vec![
    Tool::new(
        "acme_status",
        "Return the state of one Acme service.",
        json!({
            "type": "object",
            "properties": {"service": {"type": "string", "minLength": 1}},
            "required": ["service"],
            "additionalProperties": false
        }),
    )
    .with_annotations(Annotations {
        read_only_hint: Some(true),
        ..Annotations::default()
    }),
])?;
```

`Catalog::new` refuses invalid names, duplicates and schema keywords the
library does not enforce, such as `pattern`. Arguments are validated against
the schema before your code runs.

A `$ref` may have annotation siblings such as `description`, but assertion
siblings such as `maxLength` or `type` are refused. Put constraints in the
referenced schema, or combine the reference and constraints with `allOf`.

## Implement the handler

```rust,ignore
use canonic_mcp::{
    stdio::{Context, Handler, Outcome, Request},
    Client, Content, RpcError, ServerInfo, ToolResult,
};

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _client: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", env!("CARGO_PKG_VERSION"))
            .with_instructions("Acme reports service state. Call acme_status with a service name."))
    }

    fn call(&self, request: Request, _context: &Context) -> Outcome {
        let Request::Tool(call) = request else {
            return Outcome::Done;
        };
        let service = call.arguments["service"].as_str().unwrap_or_default();
        Outcome::Tool(ToolResult::success(vec![Content::text(format!(
            "{service} is running."
        ))]))
    }
}
```

## Serve

```rust,ignore
use canonic_mcp::stdio::{self, Options};
use std::{io, sync::Arc};

let result = stdio::serve(
    Arc::new(Acme),
    Arc::new(catalog),
    io::BufReader::new(io::stdin()),
    io::stdout().lock(),
    &Options::default(),
);
```

`serve` returns when the host closes stdin and admitted work has settled, or
with a `ServeError` for a broken connection. Write diagnostics to stderr or a
file, never to stdout.

Next: [serve over stdio](guides/stdio.md) explains lanes, deadlines and
disconnection.
