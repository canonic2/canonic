# Chat

## Purpose

Present a bounded rich conversation above one fixed Composer. Hosts retain
history and perform agent, storage, clipboard, opening and inspection operations.

## Current status

The reusable component and 51 interactive gallery scenarios are implemented.
The gallery acknowledges simulated host effects; it does not connect an agent
or use the OS clipboard. Engineering evidence is in
[Verification](../verification.md#chat-engineering-record--2026-10-10).
Human visual acceptance and consumer UI integration are separate checkpoints.

## Behavior and state transitions

### Turns and identified updates

Turn contains stable ID, role (User, Assistant, Note), display label, optional
timestamp/day/duration, end state and ordered Parts. Each part has its own ID,
revision and Content. Notes have no header. Immutable user submissions show dim
sending until the exact Composer ticket is accepted; refusal shows the reason
while Composer protects the independent draft.

Update addresses a Conversation token, message, part, expected revision and
next revision. Append/Replace are atomic; foreign conversations are ignored.
Missing targets and revision mismatches return a resync Request without changing
content. Resync must preserve parts/revisions and precede intervening progress.
Acceptance shows a dim reloaded notice. Duplicate/stale callbacks are inert.

Live assistant output has an accent marker at the last part's text edge. Done
shows a green check and supplied elapsed label. Stopped preserves partial content
with a dim cross/reason. Failed preserves content and offers Ctrl-X R when the
host enables retry; retry emits a request and the host starts a new reply.
Ended turns cannot accept part updates or a different outcome. All clock labels,
reasoning, execution outcomes and capabilities come from the host.

### Rich content and visual implementation

Body text is indented two cells and wraps to available width, capped at 100 prose
cells. User headers are bold terminal foreground; assistant headers accent bold;
metadata is dim. Day boundaries survive hiding timestamps. Blank rows separate
turns and blocks. Code/table/diff layouts use the available width.

CommonMark supports paragraphs, headings, emphasis, nested/ordered lists, quotes,
rules, links, inline/fenced code and tables. H1/H2 are accent bold; H3 and table
headers are bold. Quotes have a quiet vertical bar; rules cap at 20 cells.
Strikethrough stays literal. Raw HTML is muted literal text. Terminal controls,
including fragmented escape sequences, become visible text. Incomplete Markdown
is readable and only its changed part is reparsed.

Code has a quiet language/file/start-line rule, four-cell tab stops and a wrap
marker. Local lexical styling covers Rust, JavaScript, TypeScript, Python, shell
and JSON; unknown languages stay plain. Code copy preserves tabs/newlines;
Markdown copy preserves fences. Decorations never enter copy. Nothing executes
and no grammars are fetched.

Tables align columns, right-align numeric values and have no vertical borders.
Below 50 cells wide or when the table cannot fit, labeled records keep every
loaded column. O requests full loaded table inspection. Diffs preview the first
unified hunk with line numbers and addition/deletion signs, then remaining-hunk
count; O requests the full loaded patch. Search temporarily reveals hidden hunks.
Malformed/binary/no-hunk patches have a labeled raw fallback.

Generic tool parts show supplied verb/target/summary and folded output. Reasoning
is host-supplied and folded initially. Enter in block selection toggles folds.
Attachments show name/size/availability and optional open target. Unknown kinds
have a readable label. Opening rejects controls and unsupported schemes and is
always a host intent.

### Navigation, follow and paging

| Key | Behavior |
| --- | --- |
| Alt-PageUp / Alt-PageDown | Move one visible transcript page |
| Alt-Up / Alt-Down | Move three rows |
| Alt-Home | Oldest loaded row; again requests earlier history |
| Alt-End | Resume following; request latest if newer history is unloaded |
| Ctrl-F | Search loaded content |
| Ctrl-X V | Select conversation blocks |
| Ctrl-X T | Toggle timestamps |
| Ctrl-X R | Request retry when available |

Plain editing/navigation remains Composer-owned. Manual navigation saves a
message/part/source-offset anchor across resize, prepend and updates. Eviction
shows loss explicitly. The unread row is reserved above activity, keeping the
dock fixed: live output shows new output; ended messages show a count.
Sending or Alt-End resumes follow and clears unread.

Earlier/Latest requests are opaque, instance-scoped and single-use. One paging
request can be pending; failure retains loaded content and allows retry. Hosts
supply remaining-history counts and authoritative windows; Chat loads no storage.

### Search and selection

Ctrl-F focuses a separate Editor in the activity row, dims Composer and preserves
its draft/caret/selection/undo. Search is literal, Unicode case-insensitive and
loaded-only. Formatting markers, line numbers and generated chrome do not match.
Up/Down and Ctrl-P/N move through matches and wrap with a notice. The current
match uses selection/accent; others underline. Matching folds/hunks temporarily
open with a notice. Esc restores the old anchor; Enter keeps the match position.
Both restore Composer and refold temporary expansions.

Ctrl-X V starts at the newest block. Up/Down move by block; Shift extends;
M selects a whole message; Y requests original Markdown; C requests code source;
O opens a supplied link/file/attachment or requests table/diff inspection.
A margin marker identifies the range without changing source. Enter toggles
folds. Esc resumes following only when selection began while following.

Copy/Open/Inspect wait for explicit host acknowledgement. Success shows a dim
notice with copy kind/line count; failure keeps selection with a retryable reason.
Foreign, duplicate and stale completions are ignored. There are no OSC clipboard
or hyperlink writes and no mouse capture; native terminal selection remains usable.

### Welcome, narrow layouts and profiles

Welcome uses host title/context/recent summaries/suggestions and disappears with
the first turn. Session browsing is host-owned. Tiny areas clamp; hints give way
to content. ANSI16 code avoids filled backgrounds; inline code uses underline
where no surface exists. NO_COLOR uses reverse selection and signs/labels.
ASCII replaces generated decoration only and preserves Unicode source.
Reduced motion keeps the same content and navigation.

### Bounds and performance

| Resource | Limit and visible behavior |
| --- | --- |
| Loaded turns | 100; evicted history counts remain explicit |
| Content per turn | 64 KiB after sanitation; omitted bytes/lines labeled |
| Parts per turn | 128; extra parts labeled |
| Parsed blocks per turn | 1,024; extra blocks labeled |
| Tables | 64 columns, 1,024 rows, 8,192 cells per turn; omissions labeled |
| Display rows | 2,048 per part; 32,768 cached total; omissions labeled |
| Wrapped-row allocations | Estimated 8 MiB; source/parse storage separate |
| Search matches | 4,096; limited result scope explicit |

Allocation prioritizes the manual anchor or newest output while following.
Source/parse storage is bounded by the turn/part/content limits. Metrics expose
parses, layouts, cached rows and estimated wrapped bytes. Unchanged frames reuse
parse/geometry; part updates invalidate that part, resize invalidates wrapping,
and color changes require no parsing. Measurement is evidence, not a portable
frame-time guarantee.

## Implementation design

[Chat](../../src/chat.rs) owns records, opaque requests and cache state. Submodules
separate content parsing, lexical styling, color-neutral layout, transcript
chrome, interaction and rendering. Blocks retain original Markdown separately
from semantic search text and generated runs. Rows carry source anchors and
optional display-to-source maps.

render_with frames the host; render_docked_with renders the fixed dock. Both
consume supplied Frame, Rect and Presentation without acquiring a terminal.
update returns compatible Action::Submit/Cancel; take_intents drains Chat work.
Hosts separately drain composer_mut().take_intents().

Compatibility push/push_turn/append remain. Rich adapters use push_record, apply,
resynchronize, finish_turn, complete_earlier/complete_latest and acknowledge.
reset_transcript invalidates Chat tickets/view state while keeping Composer.

## Host boundary

Hosts supply authoritative IDs/revisions, formatted time, content/capabilities
and history windows. They execute retry, cancellation, clipboard, opening,
inspection, saving and external-editor handoff. Composer owns its one draft,
submission, history, queue and stash. Actions availability is a display label
and grants no authority. The gallery's actions-off review choices are a local
host projection; reusable permission reviews belong to Tool Call Card.

## Acceptance criteria

- **C-01:** Interleaved updates/resync reject stale/foreign results without changing
  immutable submissions or newer progress.
- **C-02:** Rich/provisional content, narrow tables and safe fallbacks are readable;
  Markdown/code copy retains source rather than decoration.
- **C-03:** Follow/unread/reflow/paging/error/eviction preserve position or show loss;
  the dock and instance caches remain bounded and stable.
- **C-04:** Unified previews, hidden-hunk search and full-inspection intents retain
  supplied patch content without Git effects; malformed patches remain readable.
- **C-05:** Loaded semantic search/block navigation expose scope, restore draft/
  anchor and require generation-safe host acknowledgements.
- **C-06:** Long-history measurements record cold/warm work and cache bounds;
  omissions are explicit and full-history/export remains host-owned.

## Remaining gaps

Human acceptance, consumer integration and broader native coverage are separate.
Unloaded-history search, session browsers, split diff viewers, real host adapters
and reusable permission reviews belong to their own components/hosts.
