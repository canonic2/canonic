# MCP Kit

Model Context Protocol servers in Rust.

`canonic-mcp-kit` lets a Rust program expose tools, resources and bundled workflows
to an MCP host. Your program supplies the content and decides what tools may
do. The library handles protocol messages, validation and request lifetimes,
with ready-to-use stdio and Streamable HTTP drivers or a core you can drive
from your own runtime.

## Installation

The library is built from source and has not been published to crates.io.
You need Rust 1.97 or later; the checkout pins the toolchain version.

From a sibling Rust package, add a local path dependency:

```toml
[dependencies]
canonic-mcp-kit = { path = "../mcp-kit" }
serde_json = "1.0"
```

Adjust the path to your checkout's `packages/mcp-kit` directory.

## Quick start

Run the bundled example from `packages/mcp-kit`:

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

## Highlights

- **Tools and content:** validated argument/result schemas, resources, templates,
  prompts, skills and optional MCP Apps.
- **User interaction:** confirmation, structured input and URL consent, with
  host capability checks and bounded interaction lifetimes.
- **Request handling:** worker lanes, ordering, cancellation, deadlines, progress
  and optional Tasks.
- **Implementation updates:** opt-in stable frontends with Process Kit cutover,
  native adapter hooks and explicit resource holds; see [worker replacement](docs/guides/worker-replacement.md).
- **Transport choice:** shared stdio and HTTP drivers, or an I/O-free core for
  your own transport and scheduler.
- **HTTP access control:** bearer-token validation hooks and an optional embedded
  authorization server. Without authentication, bind loopback.

## Resources and skills

Use resources for content a host can retrieve by URI, such as a guide or a
generated report. Templates describe addresses the host can fill in, such as
`acme://reports/{id}`. Your resolver supplies the bytes and checks access.
See [publishing resources](docs/guides/resources.md).

Skills add discoverable workflows and complete manifests over those resources.
Supply `SKILL.md` and its supporting files, then enable the skill catalog and
modern protocol support. The library validates their metadata, derives digests
from the exact bytes and serves the bundle. For content generated on request,
publish a dynamic skill: your handler serves its files, and the library checks
that each served `SKILL.md` matches the published metadata. See
[publishing skills](docs/guides/skills.md).

## Protocol and runtime support

The default configuration supports `2025-06-18` and `2025-11-25` through the
initialization handshake. Explicit modern configuration adds `2026-07-28`,
including discovery, per-request client context and cache hints. Set
`SessionOptions::legacy_protocols` to `false` with modern support enabled to
serve only `2026-07-28`; the HTTP driver selects this profile automatically.
Skills discovery uses the modern path; legacy clients can read the bundled
files as ordinary resources.

The shared transports are newline-delimited JSON-RPC over stdio and, with the
`http-server` feature, Streamable HTTP for `2026-07-28` only. For an
asynchronous runtime or another transport, drive the core directly and own
its scheduling and connection lifetime; the `http` exchange checks HTTP
requests for your own server. See [driving the session](docs/guides/session.md)
and [serving over HTTP](docs/guides/http.md).

Your program owns tool authority, resource access and continuation-state
validation. Hosts own skill activation, approval and presentation. Reading a
bundled file never executes it, and a confirmation response never grants
authority by itself.

## Documentation

- [Your first tool](docs/getting-started.md): build a server with a complete example.
- [Stdio](docs/guides/stdio.md) and [HTTP](docs/guides/http.md): choose a driver and
  configure request handling.
- [Authorization](docs/guides/authorization.md): protect HTTP access and choose a
  token verifier or [embedded authorization server](docs/guides/authorization-server.md).
- [Resources](docs/guides/resources.md), [skills](docs/guides/skills.md) and
  [MCP Apps](docs/guides/apps.md): publish content and workflows.
- [All guides](docs/README.md): schemas, pagination, subscriptions, prompts,
  completion, Tasks and user interaction.
- [API map](docs/reference/api.md) and [errors](docs/reference/errors.md):
  consult types, limits and recovery paths.

## Development

From the checkout root:

```sh
make -C packages/mcp-kit dev
make -C packages/mcp-kit test-focus FILTER='session::'
```

[Developing MCP](docs/contributing/development.md) covers individual checks
and test tooling.

## License

`canonic-mcp-kit` is subject to the [Canonic Proprietary License](LICENSE).
Third-party components keep their own licenses.
