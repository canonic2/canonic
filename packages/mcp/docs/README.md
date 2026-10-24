# MCP library documentation

`canonic-mcp` gives a Rust program a Model Context Protocol server: strict
framing, the initialization handshake, a validated tool catalog, argument
checks, bounded results, cancellation, deadlines, progress and host
confirmation. Your program supplies the tools and decides what they may do.

Start with the [quick start](getting-started.md).

## Guides

- [Quick start](getting-started.md) — dependency, a first tool and a stdio server.
- [Serve over stdio](guides/stdio.md) — lanes, keys, deadlines, progress, extension methods and disconnection.
- [Ask the user to confirm](guides/confirmation.md) — form confirmation and its continuation.
- [Drive the session yourself](guides/session.md) — use the protocol core from another runtime.

## Reference

- [API map](reference/api.md) — find the right type or function and build rustdoc.
- [Errors](reference/errors.md) — reply codes, refusals and what to do about them.

## Contributing

- [Development](contributing/development.md) — build, checks and test tooling.

These guides describe the supported public API. Generated rustdoc supplies
exact signatures. The [specifications](../specs/README.md) are the separate
internal contract reference, including every [limit](../specs/limits.md).
