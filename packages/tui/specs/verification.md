# Verification and acceptance

This topic owns evidence interpretation and the current engineering summary.
[Code quality](code-quality.md) owns executable package checks;
[Human testing](human-testing.md) owns runnable review steps, human acceptance
and historical gate/commit records. [Roadmap](roadmap.md) owns delivery order.
Component/feature specs own the acceptance requirements; neither evidence nor
delivery tables redefine them.

## Independent checkpoints

Track each checkpoint for a named scope and revision:

| Checkpoint | Evidence required | Does not establish |
| --- | --- | --- |
| Specification complete | Behavior, states, keys, implementation design, host boundary, acceptance and gaps reviewed | Implementation or accepted visuals |
| Design resolved | Layout, feedback, responsive/fallback behavior and interactions settled in the public contract | Engineering or human acceptance |
| Functionality complete | All in-scope behavior implemented; remaining host duties explicit | Passing checks or consumer rollout |
| Engineering verified | Dated commands/results and applicable state/render/property/native/consumer checks | Human acceptance or untested platform coverage |
| Human accepted | Explicit reported pass for demonstrated steps on the reported terminal | Acceptance of later material changes or other hosts |

Use partial or pending when any in-scope requirement lacks evidence. A checkpoint
commit is not human acceptance. Tests found in source are not a new test run.
These records were carried forward during the specification reorganization;
Markdown changes do not rerun or refresh Rust engineering evidence.

## Completion Menu engineering record — 2026-10-10

`make -C packages/tui check` passed formatting, strict Clippy, all 132 tests,
two doctests, API documentation and dependency auditing. Dedicated behavioral
and property checks cover matching/ranking, disabled navigation, generation
rejection, streaming order/selection, debounce/loading/timeout, retry, guesses,
insertion budgets and editor token boundaries. The native macOS PTY trace covers
insert-then-invoke, argument delivery, source retry, resize, quit, cooked input and
exact terminal restoration. Gallery checks render all 15 states at tiny and
standard sizes and assert a fixed dock, display-only argument hints and back
navigation. Main, narrow, long-description and partial-failure layouts have
reviewed render snapshots; the semantic-color main view was inspected visually.

`make -C packages/tui build` passed the release library/gallery build.
Gallery-disabled nextest passed all 104 tests. Gate, Shield and Drone passed
offline compilation checks with their optional `tui` feature using disposable
manifests; their workspace lockfiles remained byte-for-byte unchanged.
Human visual acceptance, native Linux/Windows acceptance and real consumer source
discovery/execution remain separate from these local engineering results.

## Composer engineering record — 2026-10-10

Engineering verification on 2026-10-10: `make check` passed all 114 tests,
formatting, strict Clippy, one doctest, API documentation and dependency auditing.
The native Unix tests cover acknowledgement/recall/resize/quit, cooked foreground
handoff and private editor-file ownership. Rendered screens were reviewed for
queue edits, history, stash, editor conflicts, file insertion and paste chips.
`make build` passed the release library/gallery build. Gallery-disabled nextest
passed all 88 tests. These checks establish the reusable component and explicit
host adapters; gallery host work remains simulated.
Gate, Shield and Drone also passed offline checks with their optional `tui`
feature. Their original lockfiles were preserved byte-for-byte.

The evidence applies to the implemented reusable workflows and explicit host
adapters. Standalone Completion Menu, rich Chat targets and real consumer
execution/storage/clipboard integration are outside that verified scope.
Human acceptance remains in [Composer review](human-testing.md#gate-3a--submission-lifecycle).

## Chat engineering record — 2026-10-10

Scope: the rich reusable Chat component, fixed Composer composition and 51
interactive gallery scenarios. Tests cover identified interleaving/resync,
stale/foreign callbacks, loaded semantic search, folded content, original-source
copy, full-inspection intents, reflow/paging anchors, unread/dock geometry,
safe raw content, Unicode/ASCII and pathological cache bounds. Six snapshots
cover answer, wide-table fallback, folded search, selected code, actions-off and
the pane; Completion Menu snapshots reflect Chat's indentation/wrapping and
reserved unread row.

The macOS Unix PTY workflow covers code copy acknowledgement, loaded search,
draft-caret restoration, picker return, resize, Ctrl-C, unchanged native mouse
capture and exact terminal-attribute restoration. Physical keyboard/emulator
acceptance and native Windows coverage remain separate.

`make -C packages/tui check` passed formatting, strict all-target Clippy,
157 all-feature tests, three doctests, private-item API documentation and
dependency auditing. Gallery-disabled nextest passed 126 tests. The release
library/gallery build passed. Gate, Shield and Drone optional TUI compilation
passed offline using disposable manifests/lockfiles; workspace lockfiles stayed
unchanged. This is dependency compilation, not consumer UI rollout.

Recorded release measurement: macOS 15.7.9, x86_64 Intel Xeon W-2140B at 3.20 GHz,
Rust 1.99.0, ANSI16 TestBackend at 100×40, 100 turns containing 550,000 bytes of
Unicode prose. Cold construction plus first draw took 279.25 ms. Mean warm
draw time over 25 frames was 1.22 ms with 19,900 cached rows and no additional
parses/layouts. This includes TestBackend construction/rendering, excludes
physical terminal writes, and is not a p95/input-latency or portable guarantee.
The broader performance campaign remains in [Gallery](features/gallery.md#performance-measurements--proposed-targets).

Public Markdown links/anchors and private-design separation passed; all 36 Chat
design links resolve. Public/private TUI diffs passed whitespace checks.
Human visual acceptance remains pending;
the [Chat review](human-testing.md#chat-review--complete-local-conversation)
provides the complete component workflow. Clipboard/open/agent/history effects
in the gallery are simulated, not consumer rollout evidence.

## Foundation engineering record — 2026-10-09

Foundation engineering checks passed during the scoped package commits
`33fe70a` (private) and `2a6bc0f` (public): formatting, strict Clippy, ten
all-feature nextest tests including the Unix PTY test, seven gallery-disabled
tests, doctest, API docs, dependency audit, release build, consumer optional
feature compilation and Markdown/diff checks. Dependency auditing reported
allowed duplicate-version warnings. Foundation human acceptance passed on
2026-10-09; the user deferred visual refinement. These results do not accept
expanded capability contracts or establish visual design acceptance.

## Evidence and review locations

- [Human acceptance record](human-testing.md#acceptance-record) distinguishes the
  Editor/Picker functional gates, their visual follow-ups and pending Dialog,
  Command Palette and Composer reviews.
- [Component checkpoints](components/README.md#completeness-checkpoints) summarize
  current scopes without upgrading partial work to complete.
- [Inventory](inventory.md) links source/test evidence and identifies gaps.
- [Gallery coverage](features/gallery-designs.md) records executable scenarios
  and pending coverage; harness controls are outside product layout acceptance.

For behavior changes, run the owning package checks and record observed results
with scope, date and platform. Use real terminal evidence for terminal contracts.
For Markdown-only changes, check local links/anchors, private-design separation
and each affected Gate repository's diff. Do not add tests that assert prose.
