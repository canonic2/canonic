# MCP

Model Context Protocol servers in Rust.

`canonic-mcp` lets a Rust program expose tools, resources and bundled workflows
to an MCP host. Your program supplies the content and decides what tools may
do. The library handles protocol messages, validation and request lifetimes,
with a ready-to-use stdio driver or a core you can drive from your own runtime.

## Highlights

- **Validated tools.** Publish tool descriptions and argument schemas. Calls
  are checked before your handler runs; results support text, images and
  structured data.
- **Resources and templates.** Serve guides, reports and binary artifacts by
  URI, with paginated catalogs and parameterized addresses.
- **Bundled skills.** Publish workflows alongside their tools, with parsed
  metadata, complete file manifests and SHA-256 digests. Files are prepared
  once and served from memory.
- **Managed stdio requests.** Worker lanes, per-key ordering, queue limits,
  deadlines, cancellation and progress keep request handling bounded.
- **User confirmation.** Ask a yes/no question through the host, using legacy
  continuations or modern input-required replies and retries.
- **Your runtime.** The protocol core performs no I/O and starts no threads.
  Use the shared driver or bring your own transport and scheduler.

## Installation

The library is built from source and has not been published to crates.io.
You need Rust 1.99 or later; the checkout pins the toolchain version.

From a sibling Rust package, add a local path dependency:

```toml
[dependencies]
canonic-mcp = { path = "../mcp" }
serde_json = "1.0"
```

Adjust the path to your checkout's `packages/mcp` directory.

## Quick start

Run the bundled example from `packages/mcp`:

```sh
cargo run --locked --example skills
```

It serves an `acme_status` tool and a status workflow with supporting files.
Paste this JSON-RPC request as one line on stdin:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"acme_status","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

The server replies with a complete tool result containing the text `running`.
Close stdin to stop it. After dependencies are acquired, the example needs no
network access; its workflow files are embedded in the executable.

See the [example source](examples/skills.rs), or follow the
[first-tool guide](docs/getting-started.md) to build your own server.

## Resources and skills

Use resources for content a host can retrieve by URI, such as a guide or a
generated report. Templates describe addresses the host can fill in, such as
`acme://reports/{id}`. Your resolver supplies the bytes and checks access.
See [publishing resources](docs/guides/resources.md).

Skills add discoverable workflows and complete manifests over those resources.
Supply `SKILL.md` and its supporting files, then enable the skill catalog and
modern protocol support. The library validates their metadata, derives digests
from the exact bytes and serves the bundle. See
[publishing skills](docs/guides/skills.md).

## Protocol and runtime support

The default configuration supports `2025-06-18` and `2025-11-25` through the
initialization handshake. Explicit modern configuration adds `2026-07-28`,
including discovery, per-request client context and cache hints. Skills
discovery uses the modern path; legacy clients can read the bundled files as
ordinary resources.

The shared transport is newline-delimited JSON-RPC over stdio. For an
asynchronous runtime or another transport, drive the core directly and own
its scheduling and connection lifetime. See
[driving the session](docs/guides/session.md).

Your program owns tool authority, resource access and continuation-state
validation. Hosts own skill activation, approval and presentation. Reading a
bundled file never executes it, and a confirmation response never grants
authority by itself.

## Documentation

- [Your first tool](docs/getting-started.md)
- [Lanes, deadlines and cancellation](docs/guides/stdio.md)
- [User confirmation](docs/guides/confirmation.md)
- [Resources and templates](docs/guides/resources.md)
- [Bundled skills](docs/guides/skills.md)
- [API map](docs/reference/api.md)
- [Errors and recovery](docs/reference/errors.md)
- [All guides](docs/README.md)

## Development

From the checkout root:

```sh
make -C packages/mcp dev
make -C packages/mcp test-focus FILTER='session::'
```

[Developing MCP](docs/contributing/development.md) covers individual checks
and test tooling.

## License

`canonic-mcp` is subject to the [Canonic Proprietary License](LICENSE).
Third-party components keep their own licenses.
