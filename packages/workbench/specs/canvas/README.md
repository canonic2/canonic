# Canvas behavior

This folder owns how people select and arrange views. Shared address routing and
problem reporting remain in [core](../core.md); the [glossary](../glossary.md)
defines pages, scenarios, lenses and artboards.

| Topic | Owns |
| --- | --- |
| [Lenses](lenses.md) | Stable lens identity, display labels, defaults and selection precedence |
| [Sizes](sizes.md) | Size schema, fill axes, menus/editing, addresses, multiple-artboard size policy and resolved dimensions used by review/export |
| [Navigation](navigation.md) | Requested versus displayed destination, the 200 ms grace period, pending/loading/failed presentation, settling per slot, group publication, capture/handoff/context while pending |
| [Navigation store](navigation-store.md) | Agreed Lattice store design for navigation: state, phases, events, identity across documents and invariants |
| [Interaction](interaction.md) | Camera and zoom, scroll behavior, keyboard focus, clipboard/editor routing and host differences |
| [Annotations](annotations.md) | Drawing/editing, per-artboard state, coordinates, review clearing and acceptance gaps |
| [Persistence](persistence.md) | Durable logical selections and preferences, opening precedence, first-page fallback, validation and restoration in browser/VS Code |
| [Multiple artboards](multiple-artboards.md) | Built artboard instances and declared groups: isolation, limits, declared-group parking, review/context requirements and acceptance |
| [Artboard system](artboard-system.md) | Module ownership, state and runtime/transport contracts and the capability map |
| [Artboard management](artboard-management.md) | Proposal, not agreed: add/duplicate/close, Fit operations, arrangement, its persistence, open questions and the delivery plan |

State owners: navigation state belongs to [navigation](navigation.md); camera
and zoom to [interaction](interaction.md); retained live sessions to the
pipeline's [retained sessions](../preview-pipeline/warm-runtime.md#retained-sessions),
which owns live document reuse and cleanup. Persistence saves destinations and
viewer preferences; retained DOM and component state are not restart persistence.
The [rendering boundary](../system/rendering-flow.md) owns one-shell/content-only
rules. Artboard design applies those rules without redefining them.

Artboard composition supplies the review target; [review](../review/README.md)
owns acquisition and export jobs, and [agent context](../integrations/agent-context.md)
owns delivery to agents. Layout persistence remains separate from the agreed
single-view restoration work.
