# Gallery scenario coverage

The flow is **component picker → scenario picker → component scenario**. The
scenario picker is searchable with `main` first. Esc walks back through these
levels after component-specific dismissal. Choosing a scenario resets only that
component. F10 opens the active component's scenario picker and F10 again returns
to the current scenario without resetting its draft or focus. The picker pauses
simulated updates. CLI `--component` opens scenarios, and `--scenario` opens a
specific fixture directly. These use the same registry as F10 and the headless
`--list` inventory. The component picker and each component's scenario picker
retain search text, selected item and viewport for the current gallery lifetime.
Esc and F10 reopen that retained state rather than constructing a new picker.
At the same terminal size the prior view is restored; after resize the viewport
adjusts to keep the selected item visible.

Contract review: 2026-10-10, including the composer's updated Up-key behavior.
The registry is `src/gallery/scenarios.rs`. It contains 149 named entries, plus
four main-view presentation profiles per component. This is a contract coverage map,
not a claim that every specified state is implemented or visually accepted.

| Component / public contract | Interactive scenarios | States needing a dedicated implementation or fixture |
| --- | --- | --- |
| [Option picker](../components/option-picker.md#picker-contract) | main/default, details, disabled, danger, filter, no-match, scrolled, multi, groups, loading, narrow, done; supplemental empty/error; inline placement, centered dialog, full-screen host | Native inline terminal ownership; inline placement currently runs inside the gallery's alternate screen |
| [Editor](../components/editor.md#editor-workflow) | main/focused, blurred, empty, wrap, selection, unicode, scrolled, pasted, undo, near-limit, limit, paste-cut; supplemental modal focus isolation | Dedicated composer-host and growing prompt-host fixtures. Composer input is already available through the composer main view, and dialog prompts through dialog. Foreground editor host adapter is verified separately |
| [Dialog](../components/dialog.md#official-dialog-design) | main/prompt, multi-line, confirm, notice, nested/nest, replace, remove, scroll; supplemental locked, outside, removed-return, error, rename; late-result rejection | Timed waiting presentation. `late` exercises stale ticket rejection but does not reproduce the required live busy activity |
| [Commands](../components/command-palette.md#commands-completion-and-help) | main/palette, filter, filter-key, unavailable, no-match, keys, keys-custom, conflict, pending, pending-hints, pending-miss; supplemental empty | Native human acceptance remains separate |
| [Completion Menu](../components/completion-menu.md) | main, files, loading, still-loading, no-match-command, no-match-file, error, partly-failed, disabled, command-inserted, file-inserted, narrow, long-description, mixed-sources, doesnt-open | Real discovery/execution belongs to hosts; human visual acceptance remains separate |
| [Composer](../components/composer.md) | 59 runnable entries: submission/refusal, four box modes, queue/edit/remove/pause/restart, history/search/earlier sessions, stash/swap/drop, editor/return/error/conflict, paste/image/hidden controls, shell/output, commands/files/email, clearing/rewind/quit and keys | Real agent, clipboard and durable-storage integration belongs to consumer hosts; native human visual acceptance stays separate |
| [Chat](../components/chat.md), [activity](activity.md), [browsing](browsing.md) and [presentation](../presentation.md) | main, empty, streaming, error, disconnected, long | actions-off, handoff, sessions, pane, anchored-dock, answer, welcome. Current transcript fixtures do not establish rich content, activity, browser, lens or transcript-anchor acceptance |

Every component offers `narrow-40`, `ansi16`, `no-color`, and `ascii` main-view
profiles. These cover narrow and color variants without duplicating
component state. Picker also has its existing narrower picker fixture. Ctrl-T
selects dark/light tokens and F4 cycles all color modes. Actual terminal resize
remains necessary to test a small whole screen; the 40-column preview only caps
the component's area. Zero/tiny-area rendering is covered by automated checks.

Pending entries are selectable descriptions of their intended
interaction. They render no imitation of unsupported component state. Gallery
controls and notices are harness UI; they are excluded when assessing the
component against its design. Composer scenarios exercise the complete component and its simulated host ports.
The foreground editor adapter and terminal handoff have separate native tests.

## Review steps

1. Run `make -C packages/tui run`, choose a component and a scenario. Search
   by scenario name, open it, then return and open `main`.
2. In Composer main, type an unsent draft, open F10 and return with F10. The draft
   and cursor stay intact. Open `sending`: the box is empty and the fixed turn
   has dim sending status. F5 accepts it. Open `refused` and `refused-kept` to
   compare the returned draft with a preserved newer draft.
3. Open Composer `history` or `box-history`: Up/Down browse twelve entries;
   Escape or Down past newest restores the exact draft and cursor. Enter resends
   a copy; F5 accepts it. `up-key` exercises editor-first recall; `edit-queued` saves/removes held entries.
4. Open Dialog `replace`, `late` and `remove`. Check replacement text, rejected
   stale result and absence of the removed panel. Open Picker `dialog` and
   `full-screen` to compare its host placements.
5. Open each presentation profile and resize the terminal. Confirm readable
   bounded content and visible exit controls. Use Ctrl-C twice on an empty idle Composer box to exit; other components use Ctrl-C.

Engineering evidence and human approval remain separate. No commit or slice
advancement is implied by opening these fixtures.

Engineering verification includes strict Clippy, formatting, behavioral and owned
Unix PTY tests, doctests, API documentation and dependency checks. TestBackend
checks exercise every listed scenario at zero, tiny and standard sizes, verify
draft/cursor preservation, remembered picker state and presentation profiles.
The complete composer acceptance record is in [Human testing](../human-testing.md#gate-3a--submission-lifecycle).
Native human visual acceptance and consumer rollout remain separate evidence.
