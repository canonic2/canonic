# Completion Menu

## Purpose

Suggest host-supplied commands, files and tokens above an editable draft. Selecting
a suggestion inserts text. Execution and submission belong to a later host Enter.

## Current status

Status: implemented as [Menu](../../src/completion.rs), with dedicated ranking and
view modules, Composer token routing and 30 interactive gallery states. Expanded
[engineering verification](../verification.md#completion-menu-alignment-engineering-record--2026-10-11) passed. Human
visual acceptance remains separate from engineering verification.

## Behavior and state transitions

### Composer integration

Typed / at byte zero opens commands; @ at the start or after whitespace opens
files. Composer inserts selected message files as atomic host references;
shell paths use file candidates and remain plain text. Paste never opens a menu. Escape closes
and keeps the draft; that token stays dismissed until a new trigger is typed.
Cursor navigation closes only when it leaves the active token. Deleting its
trigger or adding a separating space closes. Embedded slashes/emails stay text.

An empty command result reads `no command matches "/<query>"`; file and shell
routes read `no file matches "<query>"` and `no path matches "<query>"`.
Command no-match help points to `F1 lists every key`. No match never substitutes
a candidate or command execution for the literal Enter route.

Tab and Enter insert the selected candidate with a trailing space; neither runs
a command nor sends. Disabled rows are skipped. With no selectable match, Enter
passes to Composer to send literal text. A host-supplied closest file guess is
explicit and only Tab inserts it. Replacements exceeding the editor budget keep
both draft and menu. A later Enter on a known slash command emits its host command
intent; /stash opens the stash list then. Inserted message @paths are atomic
file references with host identity; shell paths remain editable text.

Up/Down and Ctrl-P/N skip disabled entries without wrapping. Page keys move by the
visible body height. Ranking is prefix, word-start, alias, then fuzzy subsequence;
recency breaks ties. Only names highlight matches. Stable IDs preserve selection
across streaming batches; additions append below existing results for that query.

`CompletionRequest` identifies an opening, draft revision, kind and query.
`complete` accepts only current results. Editing the query creates a new ticket;
closing/resetting invalidates it. Late/error/empty results cannot choose a stale
item. Error text stays in the menu; the draft survives. Candidate IDs must be
unique within each kind, and the source is bounded to 5,000 rows.

`Menu::take_request` debounces for 60 ms, loading appears after 150 ms, and five
seconds without results produces a retryable warning distinct from source errors:
`! no reply from <source> after 5 s`, followed by
`the source may be slow or stuck · tab retries`. Partial results remain
selectable. All-source failure shows an error; partial failure adds a nonselectable
warning naming sources and reasons. Tab inserts the selected result despite a
partial warning; with no selectable result, Tab retries with a new generation.
Alt-R explicitly retries failed/timed-out sources while retaining good results.
Ctrl-R remains Composer history search and closes the menu before that route.
Stale, foreign, closed and duplicate finished-source responses are rejected atomically.

## Implementation design

The dedicated state machine owns ranking, selection, source progress and injected
time. The body opens at up to eight rows (five at 40 columns and below) and never shrinks
while filtering. Rules display counts, remaining rows and source progress. The
fixed dock stays in place. Names use accent, descriptions dim, tags right; the
selected band uses selection fill and bold accent, with reverse without color.
Only the selected description expands, to at most three lines within the same
height; the 40-column view keeps descriptions to one shortened line. Tags drop
before descriptions; below 30 columns only names remain. Names
retain full data, clipping at grapheme boundaries when physically too wide.
Menu key help belongs to the activity row immediately above the draft. The keys
row below the dock retains Composer submission, queue or shell help; opening a
menu does not replace that help with insertion keys. A refused insertion takes
priority in activity until the editor notice expires, without closing the menu.
Optional command
argument help and faded placeholders are visual props, never inserted payload.

`Candidate` contains a stable ID, display name, aliases, insertion, description, tag,
disabled reason, hidden flag, recency and optional argument hints. `Menu::new` validates one to
64 source IDs and cached rows. `set_query` invalidates the opaque `Request`;
`Batch` replaces one source's accumulated rows atomically, with scanned/total
progress, a completion flag or error. IDs are unique across sources, with 5,000
retained candidates total. IDs/source names are at most 256 bytes, names/queries/
descriptions 4 KiB and insertions 16 KiB; display text is sanitized and insertion
controls rejected. Each candidate has at most sixteen aliases of 128 bytes each.
`tick` consumes explicit elapsed time. `update` returns one
`Action`: unhandled editor input, insertion data, close, literal Enter or retry.
The host closes only after successful insertion, retaining the menu on no-fit.
Partial warnings expose `failures`, `timed_out_sources` and `retry_failed` for host details/retry UI;
Tab with a selected result continues to insert that result. Retry retains good
sources and resets only failures. Rendering caches its page size and offset;
it never changes the host's caret.

Composer's legacy `CompletionItem`/`complete` API wraps a single `host` source.
Composer source insertions omit the leading `/` or `@`; Composer adds the trigger
and trailing space. Shell source insertions are complete path strings. Touched
file fixtures use `file · edited`; recent commands use `command · recent`.
Other source tags are host-supplied `kind · source` strings; built-ins may omit tags.
`set_completion_sources`, `complete_source`, `set_completion_guess` and
`retry_completion_sources` expose named sources. `completion_due` polls a
debounced request; `Intent::RetryCompletion` requests retry I/O. A subsequent
Enter emits `Command(id)` or `CommandWithArguments { id, arguments }`, preserving
typed arguments. Acceptance registers a source-provided command's stable identity
for that subsequent invocation, even when labels repeat. Removing/disabling that
selected identity before invocation preserves the draft with an unavailable
notice. That refusal marks the unchanged draft for literal submission: the next
explicit Enter sends text and cannot invoke a different command with the same
label. Editing or accepting a fresh suggestion clears the literal recovery state.
F1 closes completion and opens Composer help without changing the draft/caret.
Hosts still validate availability at execution.
[Command Palette](command-palette.md) uses invocation rather than
file insertion; its keyboard help shares host descriptors.

## Host boundary

Hosts own command availability, repository discovery and asynchronous queries.
[Composer](composer.md) owns draft revision, opening/closing the current route
and command intents. The standalone menu owns filtering, list navigation and
geometry. Hosts own discovery, recency, source scope, closest guesses and retry
I/O. Supplied results identify both request and source. No component reads the
filesystem or executes commands.

## Acceptance criteria

- **E-06:** Completion tickets reject stale results; accepting consumes once and
  never also sends or invokes. Verify ranking, disabled navigation, pages,
  debounce/loading/timeout, streaming selection, retry, guesses, cursor boundaries,
  paste, editor budgets, Unicode and all color profiles.
- **CM-02:** Prefix and word-start matches precede aliases and fuzzy matches;
  recency breaks ties, and streamed additions preserve the selected identity.
- **CM-03:** Timeout is a warning with source identity, distinct from an error;
  Tab still inserts after partial failure and Alt-R retries only failed sources.
- **CM-04:** Menu height remains fixed across filtering; narrow, wide-grapheme,
  ASCII/light/NO_COLOR frames preserve payload and keep menu help in activity.

Cross-component descriptor consistency is owned by
[Command Palette D-03](command-palette.md#acceptance-criteria).

Gallery coverage includes commands, files, loading, partial loading, both no-match
cases, error, partial failure, disabled, both inserted states, narrow, long
description, mixed sources and non-trigger text. Additional scenarios cover
timeout, under thirty columns, shell path/insertion, dismissed typing, fresh
trigger, leaving the token, pasted slash, refused replacement, removed command,
clipped wide names, NO_COLOR/ASCII/light profiles and aliases.

## Remaining gaps

Real discovery and execution are host responsibilities; gallery sources are local
simulations.

## Design decision — 2026-10-11

Ctrl-R retains Composer history search at the user's direction. Alt-R is the
separate completion retry key; Tab continues to insert a selected row and retries
only when nothing is selectable. This keeps history available without making a
source failure change the meaning of a successful insertion.
