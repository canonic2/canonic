# Code and diff inspection

## Purpose

Read-only inspection of supplied code or a single-file unified patch, opened
from Chat, a Tool Call Card or a Tree Browser node. The component owns the
projection, logical cursor, folds, fields and identified effects. The host owns
loading, limits, editor/clipboard access and the retained caller.

## Current status

`inspection::Inspector` supplies the reusable state/input/view contract.
The gallery has 51 named live scenarios and eight presentation profiles. Chat
code/table/patch selection, patch tool details and the tree's file panel use it.
Human visual acceptance and actual consumer adapters remain separate.

## Behavior and state transitions

`Item` provides stable identity, revision, title, summary, language, origin,
optional initial source line and editor availability. `State` distinguishes
loading, ready, failed and unavailable content. Unavailable content can carry a
validated bounded preview; Enter explicitly reveals it with a partial-content
notice. Binary/generated/over-limit reasons come from the host. Empty content
is explicit. Deleted patches have old numbers only; rename facts are supplied
in the title/summary.

`Content::Code` starts at line one. `CodeAt` preserves an explicit excerpt's
first source line. `Diff` supplies a unified patch and optionally the complete
new file. The inspector never reconstructs a whole file from missing context.
Tab's whole-file view requires the supplied new file, whose loaded context and
additions must agree exactly with the patch.

| Key | Behavior |
| --- | --- |
| Up/Down | Logical lines, headers and fold rows |
| PageUp/PageDown | Screen with two rows of overlap; continue within long wrapped lines |
| Home/End | First/last loaded row |
| `]` / `[` | Next/previous hunk without wrapping; explicit boundary feedback |
| `:` | Isolated go-to-line field; new numbering, old fallback for deletion; past-end clamps |
| Left/Right / `w` | Horizontal cells / explicit wrapping |
| Tab | Diff/whole file at the same source line from Chat; return focus to tree origin |
| Enter | Expand a fold or explicitly show a supplied bounded part |
| `/`, `n`, `N` | Literal find and next/previous match |
| `v`, `y`, `c` | Select lines, copy patch/text, copy new-side code only |
| `r`, `e` | Retry failed load / request offered external editor |
| Esc | Cancel selection, clear find highlights, then return to caller |

Find is case-insensitive unless the query contains an uppercase character.
Matches map back to original Unicode grapheme ranges and include folded loaded
lines. Typing highlights without moving the cursor; Enter visits the first
match and keeps highlights. Next/previous matches reveal folds. Empty and
unmatched queries do not move the view. Single-line fields reuse Editor, reject
overflow as one insertion and isolate pasted commands. Paste outside a field
is inert. Load/copy/editor/fold/return effects require an unmodified press;
navigation can repeat, releases are inert.

More than eight supplied context lines fold their middle, keeping three at
each end. Expansion retains logical location. Selecting a fold includes its
hidden rows. Patch copying retains ASCII prefixes and selected headers/metadata;
code copying excludes removed rows, headers, signs and numbers. Copies are
LF-normalized selections, not a promise that an arbitrary selection forms an
independently applicable Git patch. Failure retains selection and host reason;
success clears the same selection, preserving a newer selection made meanwhile.

Under 50 columns diffs drop numbers and force wrapping; code keeps compact
numbers. Hunk headers reduce to the function label. Three rows show title,
cursor line and Escape; fewer show title only. Rectangles/focus are explicit.
Controls display visibly, never as terminal commands. RGB/indexed profiles use
row/word tints; ANSI 16 uses signs/color/underline, NO_COLOR signs/bold/underline/
reverse. ASCII changes decoration only. No animation or hidden clock is used.

## Implementation design

`src/inspection.rs` owns lifecycle and result validation; `inspection/model.rs`
owns host values and patch parsing; `input.rs` owns navigation/search/selection;
`view.rs` streams viewport glyphs into caller-owned frames. Theme owns semantic
diff colors. Gallery adapters compose without terminal leases or external I/O.

Validation rejects malformed ranges/counts, overlapping hunks, multiple-file
patches, mismatched whole-file content and overflow atomically. Limits: 4 MiB
combined patch/file source, 20,000 combined lines, 64 KiB per line, 128 bytes
per ID, 4 KiB per label/query and 64 KiB per host reason. Find retains at most
65,536 occurrences and explicitly asks for a refined query at that bound.
Rendering retains a viewport projection rather than all wrapped rows.

## Host boundary

The host can start with Loading while answering the caller's opening intent.
`new` issues no duplicate load. Authoritative `replace` requires the same ID
and a strictly newer revision; invalid replacements leave the view intact.
Retry, Copy and Editor return one exact `Request` with an opaque instance
ticket, item/revision and operation payload. `complete` accepts it once and
only for the matching result kind. Malformed loaded content remains pending.
Replacement, return and instance disposal invalidate old effects. Return
cancels no host work; late results cannot reopen a closed view. Reopening a
retained inspector keeps its logical position. Hosts bound multi-item caches
and restore retained Chat selection/anchor/draft/caret or tree focus/cursor.
Initial refusal is acknowledged on the caller without replacing it.

No editing, Git operation, discovery, clipboard write, editor launch,
permission grant or agent execution occurs in the library or gallery.

## Acceptance criteria

- **CI-01:** Bounded code/patches and explicit unavailable/partial/error states
  render safely without synthesizing missing source.
- **CI-02:** Lines/pages/hunks, folds, whole-file mapping and Unicode find retain
  logical source positions across sizes and profiles.
- **CI-03:** Exact single-use host requests isolate failure, replacement, return,
  foreign/duplicate results and semantic copy.
- **CI-04:** Chat/tool/tree entry and return preserve caller state; named
  scenarios and profiles are live and independently reachable.
- **CI-05:** Behavioral/render/property/native evidence accompanies Cargo gates
  and gallery-disabled/consumer compilation checks.

## Remaining gaps

Unified single-file inspection only. Split diff, editing, blame/history,
external discovery, durable caches and actual clipboard/editor adapters are
host work. Inline hosting, broader native profiles and human visual acceptance
are separate. See [verification](../verification.md) and
[human testing](../human-testing.md).
