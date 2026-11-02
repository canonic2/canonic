# MCP library documentation

`canonic-mcp-kit` gives a Rust program a Model Context Protocol server: strict
framing, the initialization handshake, a validated tool catalog, argument
checks, bounded results, cancellation, deadlines, progress and host
confirmation. Your program supplies the tools and decides what they may do.

Start with the [quick start](getting-started.md).

## Guides

- [Quick start](getting-started.md) — dependency, a first tool and a stdio server.
- [Serve over stdio](guides/stdio.md) — lanes, keys, deadlines, progress, extension methods and disconnection.
- [List tools in pages](guides/pagination.md) — opaque cursors, revision-specific visibility and large catalog discovery.
- [Notify hosts about changes](guides/subscriptions.md) — opt-in modern streams, filters, catalog replacement and resource updates.
- [Ask the user to confirm](guides/confirmation.md) — form confirmation and its continuation.
- [Publish resources and templates](guides/resources.md) — guides, reports and binary artifacts through validated URIs, tool-result links and embedded contents.
- [Publish reusable prompts](guides/prompts.md) — optional catalogs, string arguments and typed conversation messages.
- [Suggest argument values](guides/completion.md) — optional contextual choices for prompt arguments and resource-template variables.
- [Return audio and content hints](guides/audio.md) — inline recordings, audience, priority and modification hints, and task persistence.
- [Validate arguments and results](guides/schemas.md) — prepared JSON Schema 2020-12, bundled references, resource limits and output contracts.
- [Publish bundled skills](guides/skills.md) — offline workflows, verified manifests, modern request context and legacy resource fallback.
- [Bundle your guides as documentation tools](guides/documentation.md) — build-time guide bundles and the list, search and read tools.
- [Return a task for long-running work](guides/tasks.md) — optional handles, durable consumer state, polling, input and cancellation.
- [Collect choices, fields and external consent](guides/elicitation.md) — rich forms and bounded legacy/modern interactions.
- [Drive the session yourself](guides/session.md) — use the protocol core from another runtime.

## Reference

- [API map](reference/api.md) — find the right type or function and build rustdoc.
- [Errors](reference/errors.md) — reply codes, refusals and what to do about them.

## Contributing

- [Development](contributing/development.md) — build, checks and test tooling.

These guides describe the supported public API. Generated rustdoc supplies
exact signatures; the resource and Skills guides describe their publication
and content limits.
