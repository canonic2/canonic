# Conversation, content and inspection

Status: proposed target. Plain bounded role/text rendering and last-message
append are implemented; structured content and stable navigation are gaps.

## Transcript behavior

Display user, assistant and host/system entries, each with stable message ID,
revision, role label, optional timestamp/turn/run metadata and ordered part IDs.
Parts include plain/rich text, code, supplied reasoning/summary, tool reference,
attachment summary, diff and boundary/status marker. Unknown parts render a
labeled generic representation. UI cannot fabricate reasoning, token usage or
tool success; host determines available content and metadata.

Completed and active entries coexist. A live part can be appended/replaced without
rebuilding its neighbors' state. Streaming updates identify conversation, message,
part and revision/generation. Host adapter owns transport order/deduplication;
UI validates snapshot/update revision. An invalid/out-of-order patch yields a
resync outcome without corrupting rendered content. Interleaved tools and text
update their own targets. Append to the last message alone is insufficient.

Streaming incomplete Markdown is rendered as readable provisional text/block
content. Completing a fence, list or table may reflow that part, but keeps manual
viewport/selection at a logical anchor. Render is deterministic at a given
snapshot/time. Streaming never hijacks focus or submits typed input.

Follow shows latest output until explicit manual navigation. New activity while
inspection is paused increments an unread indicator; follow action clears it.
Expansion/detail visibility is local presentation state by part ID, independent
of whether a backend supplies those details. Timestamps, role names, reasoning
and tool detail visibility are explicit view options, not persisted globals.

## Rich content contract

Plain text remains the universal fallback. Markdown supports paragraphs, headings,
emphasis, lists, quotations, rules, links, inline code, fenced code and bounded
tables. Unsupported/invalid constructs show readable text without terminal side
effects. Raw HTML is text or omitted with a marker; no browser interpretation.
Terminal width constrains wrapping. Wide tables offer a scrollable inspection
view or labeled fallback rather than silently hiding columns.

Code blocks show an optional supplied language, lines and copy action. Syntax
styling is a presentation concern with plain fallback on missing language/parser.
It must not run code, fetch grammars during render or require a compiler. Wrapped
display retains original lines for copy. Tabs have deterministic cell stops.
Line numbers/decorations never enter copy payload by accident.

Diff viewer accepts host-supplied files/hunks/old-new ranges and line kinds. Unified
mode works at narrow width; split mode is optional above a measured readable
threshold. Width shrink switches to unified while preserving file/hunk anchor.
Added/deleted labels/signs work without color. No-final-newline, binary/unavailable
patches, malformed input and long paths have explicit states. File/hunk navigation
returns stable IDs. Applying/reverting/generating patches and choosing Git bases
are host operations. No Git subprocess exists inside `content`.

Tool bodies use the same code/diff/text contracts through [Activity](activity.md).
Attachments show label/type/availability and optional open intent. Links use
sanitized visible labels and explicit host opening; untrusted OSC hyperlinks are
not replayed. Concealment and decoration remain reversible presentation options.

## Bounded projection and performance

Host retains authoritative history. Initial compatibility defaults remain 100
messages/64 KiB each; target projection also bounds total bytes, part count,
layout cache and visible-row work. Changing limits is explicit and shows omitted
content markers. Truncation is never portrayed as the complete message.

Older-history paging returns a request tied to anchor/generation. Prepending
retains viewport; paging failure provides retry without losing loaded rows.
Requesting full export is host-owned and distinct from copying the loaded view.
Caches invalidate affected part on update and widths on resize. Do not rewrap
every retained message per animation frame. Proposed benchmarks and measurement
conditions are in [Gallery](gallery.md); no speed claim is accepted yet.

## Search and timeline

Loaded-transcript search matches sanitized semantic text, not borders. Default
proposal: case-insensitive literal search with deterministic match ordering;
regex is deferred. Expose loaded-only scope, query, count and current match.
Next/previous match wrap with visible indication and jump by message/part/range.
Collapsed matches can reveal their block; closing search restores prior viewport
unless user explicitly retains the jump. Revision updates recompute affected
matches and keep logical match identity where possible.

Host-supplied unloaded-history search has request generations/loading/error
states and explicit scope; absence must not imply all history was searched.
Timeline is a browser of supplied message/turn summaries, including latest-user,
previous/next and direct jumps. Fork/undo/retry from a message are host intents
with host capability/confirmation, not modifications to library history.

## Acceptance criteria

- **C-01:** Interleaved identified updates reach the correct part; invalid/stale
  revisions request resync and cannot change another conversation.
- **C-02:** Provisional/final rich text, code, tables and unknown parts remain
  readable at narrow sizes; safe plain fallback and semantic copy are verified.
- **C-03:** Follow/unread, expansion, reflow, paging/prepend/error/eviction maintain
  logical position or visible loss; update caches remain instance-scoped/bounded.
- **C-04:** Unified/split diffs preserve file/hunk anchor and line identity through
  resize; malformed/binary/no-newline cases stay inspectable without Git effects.
- **C-05:** Search/timeline jumps use stable IDs, expose scope/no-match/loading,
  and reject stale results; close restores the documented prior position.
- **C-06:** Long-history measurements meet approved budgets with bounded projection
  and visible truncation; copy-loaded and host full-export are distinguished.

## Open decisions

Select Markdown/syntax dependencies under existing license/audit rules after a
bounded prototype. Set table behavior, split-width threshold, projection budgets
and search normalization using realistic gallery evidence. Rich rendering does
not require semantic duplication of host session/storage models.
