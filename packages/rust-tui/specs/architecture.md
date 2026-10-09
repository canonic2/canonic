# Shared terminal components and gallery — foundation

Status: foundation contract and inspected baseline. The proposed expansion and
module decomposition are in [System design](system-design.md). [Inventory](inventory.md)
records current evidence; expanded behavior is specified separately by topic.

The requested basis is a reusable Rust library at packages/rust-tui with an
interactive gallery for isolated development. Drone, Gate and Shield consume it
through local path dependencies without publication. The library does not own
agent sessions, Git state, policy, approvals or product persistence.

## Contracts

Components accept input and explicit state, render into caller-owned frames,
and return product-independent actions. A picker selection never authorizes
execution by itself. Hosts choose defaults, execute/reject actions and own
application state. Importing the library has no terminal side effects.

The runtime permits one full-screen terminal owner per process. Acquisition
checks interactive stdin/stdout, enters raw mode/alternate screen and enables
bracketed paste. Partial-startup cleanup and drop restore owned modes. Nested
views use the same runtime; they never start a second event loop.

The gallery defaults to a component selector. It composes the public picker and
chat views with local Acme fixtures. It simulates activity and fixture failures
without accounts, networks, product CLIs or settings changes. Input stays usable
while streamed fixture output advances. Esc returns to the selector or exits
there; Ctrl-C exits; Ctrl-T toggles themes; F2 changes fixtures.

Cargo package canonic-rust-tui has publish=false. The gallery is feature-gated;
consumer dependencies are optional behind each product's tui feature. Product
headless paths retain their default graph and interfaces. No root workspace,
dynamic plugin mechanism, registry publication or product UI rollout is added.

## Implemented components and limits

- Picker: search, arrow navigation, safe caller defaults, original-index results,
  explicit confirmation, cancellation and empty results. Search/labels are bounded
  to 4 KiB and terminal control characters are stripped.
- Chat: bounded history, status, transcript, automatic following/manual scrolling,
  composer, pasted newlines, grapheme backspace, submission and cancellation actions.
  Retains 100 messages of up to 64 KiB each and a 4 KiB composer. Visible text is
  sanitized and wraps by grapheme cell width. Extremely narrow wide glyphs use a
  replacement character. Hosts retain authoritative histories outside the view.
- Dark/light semantic themes; tiny-terminal layouts are clipped by Ratatui.

The composer is append-only. Cursor navigation, selection, undo, command history,
Markdown, structured tool cards, mouse support and inline runtime helpers are
future increments. The shared library does not connect Drone to its gallery.
Forced signals, cross-process terminal ownership and Windows acceptance remain
unverified; the process-local guard does not claim those guarantees.

## Acceptance

Verify state/actions and safety of defaults through public component tests;
render multiple dimensions into TestBackend; review gallery snapshots with
behavioral assertions. Exercise real PTYs for navigation, submission, resize and
terminal restoration. Check each consumer with its tui feature and locked local
dependency. Passing rendering tests alone does not prove real-terminal behavior.
