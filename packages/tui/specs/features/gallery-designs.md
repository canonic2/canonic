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
The registry is `src/gallery/scenarios.rs`. It contains 502 named entries, plus
eight main-view presentation/width profiles per component. This is a contract coverage map,
not a claim that every specified state is implemented or visually accepted.

| Component / public contract | Interactive scenarios | States needing a dedicated implementation or fixture |
| --- | --- | --- |
| [Option picker](../components/option-picker.md#picker-contract) | main/default, details, disabled, danger, filter, no-match, scrolled, multi, groups, loading, narrow, done, empty, error, maximum, minimum; inline placement, centered dialog, full-screen host | Native inline terminal ownership; inline placement currently runs inside the gallery's alternate screen |
| [Editor](../components/editor.md#editor-workflow) | main/focused, blurred, empty, wrap, selection, unicode, scrolled, pasted, undo, near-limit, limit, paste-cut, kill-ring, word-parts, line-kills, typed-refused, blurred-scrolled, blurred-limit, tiny-3/2/1, paste-refused-whole, chip-cap, control-characters; modal focus isolation | Dedicated composer-host and growing prompt-host fixtures. Composer input is already available through the composer main view, and dialog prompts through dialog. Foreground editor host adapter is verified separately |
| [Dialog](../components/dialog.md#official-dialog-design) | 36 named scenarios: panels/stack, timed arrival/guard/attention, locked/outside/error/filter handling, clear/removal, tiny/tall/profile states and irreversible/exact-name confirmation; `late` remains stale ticket rejection | Human visual acceptance and native inline-screen transitions |
| [Commands](../components/command-palette.md#commands-completion-and-help) | 25 interactive frames: palette/filter/key/unavailable/empty, help/custom/conflicts, pending/hints/miss, empty scopes, help miss, host refusal, alias/slash search, short/40-column and palette/help/hints profiles | Slash insertion belongs to Composer; native human acceptance remains separate |
| [Completion Menu](../components/completion-menu.md) | 30 interactive frames covering source progress/error/timeout/retry, insertion, token dismissal/reopening, refused/removed payloads, aliases, narrow/wide names and menu profiles | Real discovery/execution belongs to hosts; human visual acceptance remains separate |
| [Composer](../components/composer.md) | 99 runnable entries: submission/refusal, four box modes, queue capacity/held/blank edit/pause/resume/restart, history/search, stash capacity/swap/drop, editor configuration/read/size/late return/conflict, typed paste/image/file payloads, shell folding/queue/background/history, commands, leaders, selection priority, clearing/rewind/quit and profiles | Real agent, clipboard and durable-storage integration belongs to consumer hosts; expanded engineering and human visual acceptance are tracked independently |
| [Chat](../components/chat.md), [activity](activity.md), [browsing](browsing.md) and [presentation](../presentation.md) | 76 interactive entries: rich identified turns, streaming/delivery/stop/resync, bounded content, loaded search and selection, delayed copy/link/inspection acknowledgements, earlier/newer loading/failure/recovery, hidden timestamps, quiet notes, host suggestions and fallback profiles | Reusable review, sessions and standalone inspection belong to their own components. Host effects are simulated; human acceptance is separate |
| [Tool Call Card](../components/tool-call-card.md) | 79 interactive entries: lifecycle/output, measured progress, child tasks/artifacts, redaction/controls, host reorder, selection/actions, concurrency/caps, profiles and twenty whole-run states, including reduced motion with advancing host seconds and generic input waiting | Human visual acceptance and real execution/clipboard/permission adapters stay separate |

## Permission Review coverage

| Component / public contract | Interactive scenarios | Remaining acceptance |
| --- | --- | --- |
| [Question Form](../components/question-form.md) | 52 live answer/review/validation/lifetime/queue states plus eight profiles | Human visual/platform acceptance and real host delivery |
| [Permission Review](../components/permission-review.md) | 56 live inspection/confirmation/lifetime/mixed-queue states plus eight profiles | Human visual/platform acceptance and real host authority |

The 56 live entries map opening/defaults, numeric highlighting, details/search/
dropped paste, exact-scope confirmation/Back/sending/session receipts, covered
later operations, destructive arm/disarm/expiry, acknowledged outcomes, R retry,
no/delayed/foreign/stale acknowledgements, material changes, expiry/withdrawal,
mixed order/reorder/withdrawal, streaming, restored draft/anchor and linked tool
cards. Long resources/choices/details/scopes and narrow/short/tiny profiles remain
interactive. `src/gallery/permissions.rs` drives public components and simulated
host acknowledgements. F5/F6/F7 apply/fail/refuse, F8 expires, F9 replaces and F12
supplies a stale result. [Human steps](../human-testing.md#permission-review) keep
visual acceptance separate from engineering verification.


## Question Form coverage

The 52 named entries exercise single/multiple/typed answers, Something else,
required/min/max/character/resource limits, free navigation and review editing,
safe cancellation and offered partial submission, immutable pending snapshots,
failed/refused/delayed/stale/foreign acknowledgements, expiry/copy/replacement,
queue reorder/withdrawal, streaming, caret return and long/narrow/tiny profiles.
`src/gallery/reviews.rs` drives the reusable form and Chat through public events.
F5 acknowledges, F6 fails, F7 refuses, F8 expires, F9 replaces and F12 supplies
a stale acknowledgement. Copy and host delivery are deterministic simulations.
See [human review](../human-testing.md#question-form-review).

## Tool Call Card coverage


| Contract area | Runnable scenarios |
| --- | --- |
| Anatomy, verbs and host placement | main, unknown, completed |
| Lifecycle and bodies | queued, running, streaming, waiting, waiting-lock, failed, cancelled, text, code, diff, large-output, not-shown, empty-output |
| Selected navigation and effects | selected, expanded, failed-selected, retried, details, copied, actions-off, copy-failed, details-failed, retry-refused, no-ack, stale-ack, delayed-ack |
| Concurrent and growing output | three-at-once, out-of-order, all-done, all-succeeded, many-at-once, streaming-large, expanded-capped |
| Responsive and safe text | narrow, narrow-diff, narrow-failed, palette-16, without-color, without-color-diff, unsafe-output, large-line; narrow-40/ansi16/no-color/ascii profiles |

F5 steps queued/waiting → running → completed; F6 fails active calls; F8 appends
eight output lines. Streaming scenarios grow on injected ticks. F7 completes
pending effects; stale-ack first advances the snapshot and rejects the old ticket.
Details uses `/` literal search, `n` next, `y` simulated copy-all and Escape return.
All host effects stay inside the fixture and create no execution or grant.

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
