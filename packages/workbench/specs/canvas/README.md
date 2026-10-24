# Canvas behavior

This folder owns how people select and arrange views. Shared address routing and
problem reporting remain in [core](../core.md); the [glossary](../glossary.md)
defines pages, scenarios, lenses and artboards.

| Topic | Owns |
| --- | --- |
| [Lenses](lenses.md) | Stable lens identity, display labels, defaults and selection precedence |
| [Sizes](sizes.md) | Size schema, fill axes, menus/editing, addresses and resolved dimensions used by review/export |
| [Interaction](interaction.md) | Camera/scroll behavior, keyboard focus, clipboard/editor routing and host differences |
| [Annotations](annotations.md) | Drawing/editing, per-artboard state, coordinates, review clearing and acceptance gaps |
| [Persistence](persistence.md) | Durable logical selections and preferences, first-page fallback, validation and restoration in browser/VS Code |
| [Multiple artboards](multiple-artboards.md) | Product workflows, instance isolation, declared composition, management proposals and acceptance |
| [Artboard system](artboard-system.md) | Implemented module ownership, state/transport design and remaining integration sequence |

Persistence saves destinations and viewer preferences. The pipeline's
[warm-runtime contract](../preview-pipeline/warm-runtime.md) owns live document
reuse and cleanup; retained DOM and component state are not restart persistence.
The [rendering boundary](../system/rendering-flow.md) owns one-shell/content-only
rules. Artboard design applies those rules without redefining them.

Artboard composition supplies the review target; [review](../review/README.md)
owns acquisition and export jobs, and [agent context](../integrations/agent-context.md)
owns delivery to agents. Layout persistence remains separate from the agreed
single-view restoration work.
