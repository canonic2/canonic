# Composer and editing

Status: proposed target. The append-only implementation remains the
[baseline](inventory.md). `editor` owns generic text interaction; `composer`
adds draft/history/reference/submit behavior without an agent dependency.

## Editor workflow

Typing inserts at the caret or replaces a selection. Left/right and backspace/
delete move/remove whole graphemes. Word movement/deletion has a documented
Unicode-aware boundary policy; punctuation is not assumed to be ASCII words.
Up/down move by rendered visual line, retaining a preferred cell column; logical
line start/end and buffer start/end are distinct actions. A selection can extend
by grapheme, word, line or buffer; select-all and replacement are explicit.

Multiline insertion, cursor visibility and scrolling work at narrow sizes and
after resize. Resize changes layout, not draft text, undo stack or logical caret.
Zero-area rendering never panics or places a cursor outside the supplied area.
Only focused enabled editors request the terminal cursor.

Undo/redo reverses actual edit transactions, including replaced selections and
references whose ranges are affected. Paste is one transaction. Adjacent typing
may coalesce under explicit transaction boundaries; navigation, paste, selection
replacement and host replacement break coalescing. A new edit after undo clears
redo. Submission is not an edit undo that restores a sent message; draft history
handles recall separately. Bound undo memory and make discarded history observable
to tests. Text buffers contain valid UTF-8 and logical LF newlines.

## Paste and limits

Normalize CRLF then remaining CR to LF. Bracketed paste is an atomic input event,
never Enter submission, slash execution or shell execution. Pasted tab treatment
is explicit (proposed: retain tab in draft, expand to four-cell stops for display).
Sanitize display controls according to [Terminal](terminal.md); preserve ordinary
Unicode. Typing and paste respect byte limits without cutting UTF-8/graphemes.

Default initial draft budget remains 4 KiB for compatibility. Proposed configurable
limits must be validated, with visible truncation/refusal and no partially replaced
selection on a rejected edit. Prefer rejecting an oversized atomic replacement;
an explicitly chosen truncate policy returns retained/dropped counts. Large paste
summary is a later capability: full text remains in an explicit draft part within
host-approved limits and expands for inspection. A summary never hides what will
be submitted or loses content during copy/undo.

## Submission and asynchronous acknowledgement

Composer owns draft ID/revision, text, reference descriptors, cursor/selection,
local history position and pending intent. Host supplies can-submit/busy/disconnected
reason and whether accepted drafts queue. Enter submits only nonempty acceptable
content; attachment-only submission is allowed only when explicitly supported.
Commands and normal text have distinct host-declared modes and visible labels.

Submission snapshots one version and returns it once. Default proposed policy:
allow continued editing, but only one pending submission; further Enter shows
pending status without duplicate intent. On rejection, the snapshot remains
available and newer edits remain intact. Acceptance clears only if the current
revision still matches; otherwise keep later edits. Host acknowledges before UI
displays successful execution. Switching identity invalidates old acknowledgements.
No `!` or `/` prefix implicitly launches anything inside the library.

## History, stash and external editing

Host supplies accepted history and stash entries as opaque IDs/revisions; library
browses/replaces, host stores. At first/last visual line, explicit history commands
can recall previous/next entries; Up/down remain editing within other lines.
Recall preserves the unsent draft and restores it when leaving history. Stash
returns a snapshot intent; removal/restore waits for host response and cannot
silently overwrite a nonempty draft. Prompt undo differs from host conversation
undo, which may affect files and is outside this editor.

External editor intent includes current draft version. Host suspends terminal,
launches configured editor and returns replacement or error. If the draft changed
meanwhile, offer an explicit replace/keep choice; no late overwrite. Temporary
files/process ownership belongs to host. Missing editor, cancellation and invalid
returned content preserve the draft and restore the terminal.

## Completion and references

Completion asks host with draft ID/revision, query generation, trigger, logical
replace range and cursor. Host supplies stable candidates (label, detail, category,
enabled reason, insert text or reference ID), loading/error state and generation.
Only matching active results apply. Filtering/ranking of supplied data is local
and deterministic; filesystem/agent/skill discovery stays outside the library.

Visible completion owns arrows, Enter/Tab acceptance and Esc dismissal. Accept
inserts/replaces exactly the validated trigger range; it never also submits.
Dismissed results cannot reopen from a late response. Changing cursor or trigger
invalidates results. Empty/error lists leave the draft editable. References carry
opaque IDs, label/type/status and optional host-supplied path/range metadata.
Editing overlapping a token removes or updates it atomically according to an
explicit token policy; never retain an invisible context attachment accidentally.

File/image/directory/IDE references display summaries and validation states. Host
controls payload loading, model support and secrets. Gallery uses Acme descriptors
without reading files. No image decoder, editor watcher or directory traversal is
required by the reference contract.

## Acceptance criteria

- **E-01:** Cursor/word/selection/replacement/delete operate at valid Unicode
  boundaries, including combining marks, CJK and joined emoji; resize preserves text.
- **E-02:** Undo/redo reproduces edits and atomic paste with bounded memory;
  CR/CRLF/newline paste never submits or invokes commands.
- **E-03:** Limit refusal/truncation is visible and leaves valid text, cursor,
  selection and reference ranges; tiny rendering keeps cursor inside bounds.
- **E-04:** Repeated submission, rejection, delayed acceptance and newer edits
  neither duplicate an intent nor erase an unrelated draft revision.
- **E-05:** History/stash/editor flows preserve unsent content on cancel/error
  and resolve overwrite conflicts explicitly; no component performs persistence.
- **E-06:** Stale/empty/error completions cannot insert; acceptance replaces only
  the declared range, consumes input once and retains coherent reference identity.

## Open decisions

Review word-boundary policy, larger draft limits, typing coalescing and atomic
reference editing in gallery examples before stabilizing public APIs. IME behavior
depends on terminal event delivery and requires native evidence; do not claim
composition-event support from ordinary Unicode insertion tests.
