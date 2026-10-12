# System design and module contracts

This contract owns architectural rules, public-contract conventions and target
capability boundaries. [Architecture](architecture.md) owns the current module
map; [Presentation](presentation.md) owns shared styling/primitives. Component
and feature contracts own their behaviors and acceptance criteria.

Implemented boundaries and proposed owners are distinguished below. Proposed
names do not imply existing exports or approved exact Rust signatures.

## Dependency direction

```text
consumer / gallery host
  -> chat, inline composition, dialogs, browsers, command palette
    -> composer, conversation, activity/review, picker, tree, status
      -> editor, content, viewport, selection
        -> text, theme, input values
host -> runtime -> Crossterm / Ratatui terminal
components -> caller-supplied events and Ratatui frame/area
```

Pure component state/input/rendering never depends on terminal acquisition,
gallery, products or agent adapters. Explicit host adapters such as foreground
editor execution are separate from those pure paths. Runtime
knows terminal capabilities/leases, not conversation semantics. Host polls external
activity and routes events. No global theme, focus tree, clipboard, session or
clock exists. The existing process-local lease guard only prevents competing owners.

Keep native Rust layout: self-named modules under `src/`, sibling directories when
a capability grows. No empty layers or unrelated migration. `lib.rs` exposes APIs,
`main.rs` parses/reports, feature-gated `gallery.rs` composes fixtures. Existing
entry points need an explicit compatibility plan when contracts change.

## Independent owners

| Capability owner | Public inputs and purpose | Outputs and owned state | Current implementation |
| --- | --- | --- | --- |
| `input` | Scoped bindings, normalized events, explicit elapsed time | Consumed/unhandled plus action; pending sequence in dispatcher | Bindings/event values implemented under commands; separate input module proposed |
| `focus` | Stable targets, visible/enabled state, modal scopes | Transitions; active target and return stack per composition | Implemented |
| `text` | Untrusted strings, byte/cell budgets, width/policy | Bounded sanitized text, truncation and source mapping; pure logic | Implemented private module; richer source mapping proposed |
| `theme` | Semantic palette and terminal profile | Immutable styles/fallbacks; no settings lookup | Implemented |
| `viewport` | Logical rows/anchors, rectangle, scroll/jump | Visible range, follow/unread, paging intent; position/cache | Shared module proposed; current components own local offsets |
| `selection` | Logical text ranges and hit mapping | Sanitized copy payload; anchor/extent | Shared module proposed; Editor selection implemented |
| `editor` | Text, limits, edit commands, paste, dimensions | Text/cursor/selection changes; undo/redo, visual cursor goal | Implemented |
| `composer` | Editor, history/stash/references, completion, capabilities | Submit/query/stash/editor intents; draft and pending metadata | Implemented |
| `completion` | Host source snapshots, query generations and elapsed time | Ranked suggestions, insert/close/retry actions, fixed-height view | Implemented |
| `content` | Plain/Markdown/code/diff blocks and policy | Rows, hit/source mapping, link intent; per-instance derived cache | Proposed |
| `conversation` | Stable message/part projection and revisions | Search/jump/copy/page intents; viewport/expansion/search/selection | Proposed; bounded plain projection in Chat |
| `tool_call_card` | Identified calls/groups, lifecycle/output/capabilities and clock tick | Bounded previews, expansion and acknowledged copy/details/retry; no execution | Implemented standalone and composed by Chat |
| `activity` | Identified run/progress projection, clock tick and independent connection | Run cancellation acknowledgements; no actual tasks | Run Activity implemented standalone and composed by Chat |
| `review` | Versioned permission/confirmation/question and acknowledgements | Respond/dismiss intent; staged answers/pending/error state | Proposed; reusable Dialog/Picker foundations exist |
| `picker` | Stable choices, groups, disabled reasons/default/filter | Choice/cancel/query intents; query/highlight/viewport | Implemented |
| `dialog` | Caller bodies, stack/size/close policy and injected time | Close/result/focus, arrival/guard/notices and attention intents | Reusable stack, timing and caution implemented; inline hosting separate |
| `commands` | Host descriptors, enabled reasons and bindings | Invocation ID; palette/filter/help; no execution | Implemented |
| `browser` | Session/timeline/resource rows and loading state | Select/query/rename/delete/fork intents; local navigation | Proposed |
| `tree_browser` | Normalized host forest and child/page metadata | Ticketed load/action results; local focus/folds/filter/cursor | Implemented reusable scope; see [Tree Browser](components/tree-browser.md) |
| `inspection` | Bounded code/patch and optional whole new file | Exact load/copy/editor results and caller return | [Implemented reusable scope](components/code-diff-inspection.md) |
| `status_panels` | Supplied sections/notices and explicit host time | Stable focus/folds, expiry, persistent errors and exact action requests | Reusable component and responsive gallery composition implemented |
| `chat` | Conversation/composer/activity/review/status and capabilities | Screen intents; layout/focus/screen-scoped presentation | Rich transcript, Composer, cards, Run Activity, review queue and opt-in status composition implemented |
| `runtime` | Explicit inline/full-screen mode/capabilities | Lease, frames/events, suspend/resume/close result; owned modes | Full-screen lease/handoff implemented; inline hosting pending |
| `gallery` | Fixture descriptions/scripted updates | Simulated results/evidence; clock/event log/fixture lifetime | Implemented; expanded fixtures/measurements pending |

The capability table does not mandate one module per row. Related
row browsers stay together until their behavior warrants extraction. No widget
inheritance tree, plugin registry or universal reactive state store is required.

## Public contract conventions

Each component accepts data/update events, caller input and frame/area/theme.
Document handled/unhandled input separately from operation intent. Host stops
dispatch on consumption: an event cannot both submit a form and invoke a
background command. Rendering can update derived layout/scroll cache but cannot
emit operations or change authoritative data.

Opaque IDs identify messages, parts, runs, requests, commands, nodes and drafts.
Reordering never changes identity. Presentation state belongs to one instance,
reset or deliberately transferred on identity change. Revisions/generations
reject stale asynchronous results. Duplicate IDs, invalid references, excessive
input and inconsistent updates return observable validation outcomes, not panics.

Projections may be borrowed snapshots or bounded owned data; choose concrete Rust
representations within each slice. Host retains full transcript/artifact data.
Wrapping caches are disposable, keyed by content revision, width and render policy.
No component retains an agent session object or owns detached asynchronous work.
Host-owned work posts validated completion events.

Illustrative exchange, not approved signatures:

```text
Submit(draft-id, v7, text, references)
Rejected(draft-id, v7, reason) OR Accepted(draft-id, v7)
Respond(request-id, r3, choice-id)
Pending / Applied / Failed / Expired(request-id, r3)
```

Acknowledging v7 must not erase v8 edits. Typed errors serve callers that branch
on failure kind; application diagnostics retain causes. Explicit time avoids
hidden timers and makes the gallery deterministic.

## Migration boundary

Chat composes the extracted Editor through Composer; original-index picker
compatibility remains available. Future capability migrations introduce stable
IDs before concurrent parts/targeted streams and preserve or explicitly adapt
existing consumer contracts. Do not make callers supply full
application state. Every changed public API has docs/examples, gallery-off compile
checks and each consumer's optional-feature check.

## Acceptance criteria

- **A-01:** Two screens isolate drafts, focus, themes, searches and generations;
  leaving one cancels only its work.
- **A-02:** Leaves work through public events/rendering without a lease/product
  service; rendering emits no operations.
- **A-03:** Reordering/removal/duplicates/stale updates cannot route an intent to
  another object; validation failures are observable.
- **A-04:** API migration preserves documented behavior or supplies explicit
  adaptations; gallery-off and consumer feature checks pass.
