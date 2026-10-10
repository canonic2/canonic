# Rust TUI specifications

These internal specs describe a reusable terminal library and independent gallery.
Implementation is authorized in bounded slices with **human testing before each
commit and next slice**, requested on **2026-10-09**. Foundation contracts remain
requirements. Expanded behavior and module contracts are a target pending
implementation and the roadmap's decisions; they are not shipped support or
approved API signatures. Start delivery with [Human testing gates](human-testing.md).

Start with [Product](product.md), then [Inventory](inventory.md). Each requirement
has one owning topic. Stable acceptance IDs are mapped to phases in [Delivery](roadmap.md).

| Topic | Owns |
| --- | --- |
| [Product and workflows](product.md) | Purpose, vocabulary, inline/full-screen journeys and consumer boundaries |
| [Inventory and gaps](inventory.md) | Complete experience inventory and dated implementation evidence |
| [Foundation architecture](architecture.md) | Existing ownership, runtime, fixtures, limits and acceptance |
| [System design](system-design.md) | Proposed modules, public contracts, composition, dependencies and state ownership |
| [Input and navigation](interaction.md) | Focus, routing, bindings, pointer behavior, scrolling, selection and copy |
| [Composer and editing](composer.md) | Editor, draft lifecycle, history, stash, completion and references |
| [Conversation and content](conversation.md) | Transcript, streaming, rich text, code, diffs, search and timeline |
| [Activity and review](activity.md) | Tool/run states, permissions, questions and cancellation |
| [Dialogs and browsing](dialogs.md) | Overlays, pickers, commands, sessions, trees, panels and notices |
| [Terminal and presentation](terminal.md) | Lease, inline hosting, cleanup, compatibility, themes and accessibility |
| [Gallery and verification](gallery.md) | Isolated fixtures, composed screens and test evidence |
| [Implementation plan](roadmap.md) | Dependency order, acceptance matrix, exit conditions and open decisions |
| [Human testing gates](human-testing.md) | Smaller delivery slices, runnable acceptance steps, confirmation and commit workflow |
| [Code quality](code-quality.md) | Existing Rust tooling and engineering gates |

[Reader guides](../docs/README.md) describe implemented behavior. The baseline
comes from source/test inspection, not a new test run. Tests present in source,
passing gates, native terminal acceptance and consumer rollout are distinct evidence.
Optional dependency declarations establish neither integration nor application coverage.

Product execution, protocol translation, authentication, storage, policy and
authority stay in consumers. No publication, distribution or product rollout is
authorized by these specifications.
