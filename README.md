# Canonic

Canonic builds tools for visual interface review, mobile and browser testing,
and coding-agent permissions, alongside Rust and JavaScript libraries for
application developers.

This public repository contains Shield, Testrun, the Rust kits and Lattice, plus
Workbench's documentation and releases. Workbench's implementation is private.

[Review interfaces](#workbench) ·
[Set agent permissions](#shield) ·
[Test mobile apps and browsers](#testrun) ·
[Build with the libraries](#libraries) ·
[Website](https://canonic.sh/)

## Libraries

These libraries are available from source and are not yet published to package
registries. The Rust kits use local Cargo path dependencies; Lattice runs as
native JavaScript. Each package's README explains setup and requirements.

| Package | Use it to | Start here |
| --- | --- | --- |
| [Agent Kit](packages/agent-kit/README.md) | Register and manage an MCP server in coding agents' native configuration | [Usage and ownership](packages/agent-kit/docs/README.md) |
| [MCP Kit](packages/mcp-kit/README.md) | Build MCP servers with tools, resources and user interaction over stdio or HTTP | [Get started](packages/mcp-kit/docs/getting-started.md) |
| [Process Kit](packages/process-kit/README.md) | Run and communicate with a shared local background manager | [Get started](packages/process-kit/docs/getting-started.md) |
| [Registry Kit](packages/registry-kit/README.md) | Acquire verified packs from signed registries and resolve their dependencies | [Quick start](packages/registry-kit/docs/getting-started/quick-start.md) |
| [TUI Kit](packages/tui-kit/README.md) | Build terminal interfaces and guided setup flows from reusable components | [Usage and gallery](packages/tui-kit/docs/usage.md) |
| [Lattice](packages/lattice/README.md) | Manage JavaScript state, keyed collections and Web Component bindings | [Guide and examples](packages/lattice/docs/README.md) |

## Workbench

Review HTML pages, component previews, Storybook stories and running apps on a
canvas in VS Code. Compare designs with implementations at different device
widths, then turn annotations and screenshots into a prompt for a coding agent.

[Install from the Marketplace](https://marketplace.visualstudio.com/items?itemName=canonic.canonic-workbench) ·
[Download a VSIX](https://github.com/canonic2/canonic/releases) ·
[Get started](packages/workbench/docs/getting-started.md) ·
[Documentation](https://canonic.sh/workbench/docs/)

## Shield

Set project permissions for coding agents. Shield checks commands, file reads
and web requests against rules stored with your project. Each request gets
**Allow**, **Ask** or **Deny**, with the rules behind the decision.

Install signed permission packs for common tools and choose a protection
profile. Agents can propose changes; you review and confirm them.

[Build and install](packages/shield/README.md#installation) ·
[Set up a project](packages/shield/docs/getting-started.md) ·
[Documentation](packages/shield/docs/README.md) ·
[Permission packs](https://github.com/canonic2/shield-registry)

## Testrun

Inspect and control iOS, Android and browser targets from a CLI or coding agent.
Read screens and UI elements, send input, capture evidence and investigate failures.

[Downloads](https://canonic.sh/testrun/#install) ·
[Build and install](packages/testrun/README.md#install) ·
[Get started](packages/testrun/docs/getting-started.md) ·
[Changelog](packages/testrun/CHANGELOG.md)

## Development

Each package owns its dependencies and checks. Start with its README. Rust
packages use Cargo from their own folder, for example:

```sh
cd packages/agent-kit
cargo nextest run --locked --all-features
```

## License

Canonic-owned software, documentation and assets are subject to the
[Canonic Proprietary License](LICENSE). Third-party components retain their own
licenses and notices.
