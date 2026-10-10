# Completion Menu

## Purpose

Offer host-supplied command or file candidates for typed prefixes, preserve the
draft during loading/failure and accept one result without also sending.

## Current status

Status: slash and file completion are implemented inside Composer through
[completion routing](../../src/composer/completion.rs) and the shared option picker.
A standalone public completion component and gallery component entry remain
pending. This contract records the existing integrated behavior; standalone
extraction needs its own design, API and acceptance.

## Behavior and state transitions

### Composer integration

Typing / at byte zero opens host command candidates. Enter emits a stable
command ID; Tab completes its text without executing. The built-in stash command
opens the stash picker. Typing @ at a word boundary opens file candidates;
Enter/Tab inserts `@path ` as plain editable text and never also sends. @ inside
an email/word is ordinary text. Supplied candidates use the shared fuzzy picker; touched files win equal-score
ties, and disabled rows cannot be confirmed. Hosts supply repository scope.
Inserted @path tokens use the information color while remaining ordinary text.

`CompletionRequest` identifies an opening, draft revision, kind and query.
`complete` accepts only current results. Editing the query creates a new ticket;
closing/resetting invalidates it. Late/error/empty results cannot choose a stale
item. Error text stays in the picker; the draft survives. Candidate IDs must be
unique within each kind, and the source is bounded to 5,000 rows.

Word deletion, undo/redo, selection and navigation bindings close completion
before editing the box, so the picker query cannot drift from its visible text.

## Implementation design

The menu grows above the fixed Composer dock to at most nine rows. Opening
height remains stable while filtering, clamped to available space. Disabled,
empty, loading and error rows follow the option picker contract. Escape closes
the menu before other Composer cancellation; paste cannot open it.
[Command Palette](command-palette.md) uses invocation rather than
file insertion; its keyboard help shares host descriptors.

## Host boundary

Hosts own command availability, repository discovery and asynchronous queries.
[Composer](composer.md) owns draft revision, opening/closing the current route
and command intents. [Option Picker](option-picker.md) owns filtering, list
navigation and geometry. Supplied completion results must identify their request.

## Acceptance criteria

- **E-06:** Completion tickets reject stale results; accepting consumes once and
  never also sends a message or invokes a file.

Cross-component descriptor consistency is owned by
[Command Palette D-03](command-palette.md#acceptance-criteria).

Composer integration and gallery tests provide evidence for the current route.
They do not establish a standalone component API or standalone gallery entry.

## Remaining gaps

A standalone public component API, its complete design and independent gallery
entry remain pending. Current evidence applies to the integrated Composer route.
