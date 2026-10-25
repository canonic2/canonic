# Workbench interface contracts

This folder owns presentation primitives and themes shared by the shell,
sidebar and design examples. It concerns Workbench's interface; project content
keeps its own framework, styles and theme.

| Topic | Owns |
| --- | --- |
| [Web Components](web-components.md) | Properties/events, Shadow DOM, registration, connection lifecycle, accessibility, design pages and migration |
| [Themes](themes.md) | Semantic tokens, standalone defaults, host theme snapshots, live changes, accessibility and project-style isolation |

Components present supplied data and emit intent. Capability controllers own
navigation, persistence, rendering and I/O. In the agreed
[target layout](../system/ui-port.md), application
state follows Lattice's state management (`@canonic2/lattice`): capability
stores hold it and bindings connect it to components. [Modules](../system/modules.md)
owns that dependency direction; [rendering flow](../system/rendering-flow.md)
owns shell/content separation. The [VS Code integration](../integrations/vscode-extension.md)
applies the theme transport contract without defining a second token model.
Behavior-specific controls stay specified with their capability, such as
[sizes](../canvas/sizes.md), rather than becoming UI-wide policy.
