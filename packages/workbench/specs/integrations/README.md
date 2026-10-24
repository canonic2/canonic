# Host and agent integrations

This folder owns how Workbench participates in an editor and how external
agents receive canvas context. Shared product behavior stays independent of
these integrations.

| Topic | Owns |
| --- | --- |
| [VS Code extension](vscode-extension.md) | Activation/trust, spaces, commands, sidebar/panel messages, restored-tab lifecycle and host-owned services |
| [Hosts and updates](hosting.md) | Editor/browser/standalone/portable capabilities, platform prerequisites, remote limits and update/shutdown semantics |
| [Agent context](agent-context.md) | Published view identity, server discovery, hook/MCP delivery, editor proposals and host acceptance |

[Persistence](../canvas/persistence.md) owns what is saved, stable identity,
startup precedence and storage-failure behavior. VS Code owns its workspaceState
adapter and editor lifecycle; it does not redefine restoration rules. Browser
IndexedDB remains specified with persistence, with no editor dependency.

[Server](../system/server.md) owns stdio/HTTP and subprocess boundaries.
[Themes](../ui/themes.md) owns theme values and snapshot semantics.
[Review](../review/README.md) owns deliberate screenshots/handoffs, separate
from automatic reference context.
