# Rust TUI specifications

These public specifications describe the reusable terminal library and independent
gallery. Each component is developed through its complete behavior, implementation
layout, runnable scenarios and verification. Specification completeness, design
resolution, functionality, engineering verification and human acceptance are
independent checkpoints. Commits and
consumer rollout require their own authorization.
The [composer contract](components/composer.md) is the complete current implementation contract.
Other topics identify implemented foundations and their remaining requirements.

Start with [Product](product.md), then [Inventory](inventory.md). Each requirement
has one owning topic. Stable acceptance IDs are mapped to phases in [Delivery](roadmap.md).

| Topic | Owns |
| --- | --- |
| [Product and workflows](product.md) | Purpose, vocabulary, inline/full-screen journeys and consumer boundaries |
| [Inventory and gaps](inventory.md) | Complete experience inventory and dated implementation evidence |
| [Current architecture](architecture.md) | Implemented package/module map, composition and existing boundaries |
| [System design](system-design.md) | Architectural rules, dependency direction, public-contract conventions and proposed capability owners |
| [Components](components/README.md) | Reusable component contracts, checkpoints and host boundaries |
| [Tool Call Card](components/tool-call-card.md) | Tool lifecycle, textual previews, concurrency, selected actions and host acknowledgements |
| [Run Activity](components/run-activity.md) | Whole-run state, cancellation acknowledgement races and independent connection status |
| [Question Form](components/question-form.md) | Structured staged answers, review and identified host acceptance above Composer |
| [Permission Review](components/permission-review.md) | Deliberate choices, exact scopes, host acknowledgements, inspection and shared review queue |
| [Session Browser](components/session-browser.md) | Dedicated stable-ID navigation, identified discovery/actions and retained per-session Chat views |
| [Tree Browser](components/tree-browser.md) | Stable trees, lazy children, ancestor filtering, acknowledged actions and caller placement |
| [Code/Diff Inspection](components/code-diff-inspection.md) | Bounded code/patch views, loaded search, selection, host effects and caller return |
| [Status Panels & Notices](components/status-panels-and-notices.md) | Supplied facts, responsive panes, explicit expiry and persistent acknowledged notice actions |
| [Features](features/README.md) | Gallery, scenario coverage, input/navigation, activity/review, browsing and terminal hosting |
| [Chat and conversation content](components/chat.md) | Identified streaming, rich text/code/tables/diffs, anchors, loaded search and block selection |
| [Presentation architecture](presentation.md) | Shared semantic tokens, drawing primitives, profiles/fallbacks and presentation decisions |
| [Implementation plan](roadmap.md) | Dependency order, acceptance matrix, exit conditions and open decisions |
| [Verification](verification.md) | Independent checkpoint definitions, dated engineering summary and evidence boundaries |
| [Design alignment](design-alignment.md) | Current handoff implementation scope, remaining component work and open decisions |
| [Human testing gates](human-testing.md) | Smaller delivery slices, runnable acceptance steps, confirmation and commit workflow |
| [Code quality](code-quality.md) | Existing Rust tooling and engineering gates |

[Reader guides](../docs/README.md) describe implemented behavior. Baseline
inspections and dated executed results are qualified in [Verification](verification.md);
reorganizing specs is not a new test run. Tests present in source,
passing gates, native terminal acceptance and consumer rollout are distinct evidence.
Dependency declarations establish neither integration nor application coverage.

Editor/focus, rich picker, dialog foundations and commands have their recorded
[human gates](human-testing.md). Composer implements submission, queue, history,
stash, chips, completion, restart encoding and editor conflicts, including an
explicit foreground editor adapter and terminal handoff. Its complete review
steps are in [Composer acceptance](human-testing.md#gate-3a--submission-lifecycle).
Timed Dialog policies and rich Chat content are implemented. Inline hosting,
remaining design alignment and human visual acceptance remain separate work.

Product execution, protocol translation, authentication, storage, policy and
authority stay in consumers. No publication, distribution or product rollout is
authorized by these specifications.

## Specification ownership

Root topics own package-wide scope, architectural rules, inventory, delivery and
verification. `components/` owns reusable views; `features/` owns workflows that
compose views or serve multiple components. Shared styling and terminal ownership
stay explicit rather than becoming component-specific defaults.

A component follows the [contract format](components/README.md#contract-format-and-ownership).
A feature states its scope/status, workflow and participating components, host
boundaries, failure/recovery behavior, acceptance and remaining decisions. Link
owned requirements instead of copying them. Keep implementation design details
in public specs; do not link private design files.

The roadmap maps acceptance IDs to dependency order. Verification records executed
evidence; human testing records reported acceptance. None of those topics changes
the requirement owned by a component or feature. Current progress is summarized
in [Component checkpoints](components/README.md#completeness-checkpoints).
