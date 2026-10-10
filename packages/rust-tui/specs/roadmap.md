# Implementation plan and acceptance

Status: phased implementation authorized on **2026-10-09**, with human testing
required between delivery slices. No phase is accepted by writing requirements
or passing automated checks alone. [Human testing gates](human-testing.md) owns
the delivery sequence, user steps, expected results and confirmation record.
The phase groups below retain their acceptance mapping; their smaller delivery
slices each require an independent human gate.

Current delivery: slice 1 was explicitly committed and advancement authorized.
Slice 2 is divided into 2a rich picker (accepted/committed), 2b dialogs
(2b.1 visual shell implemented, human gate pending; 2b.2 arrival/removal timing
unstarted) and 2c commands/bindings (unstarted).
This does not accept broader phase 2
criteria or unreported slice 1 redesign actions.

## Delivery scope and phase order

Keep local unpublished library/gallery and optional consumer dependencies. No
publication, installation, agent integration or product UI rollout is added by
this plan. Implement one bounded capability slice with its independent fixture,
composition and relevant checks before broadening. Dependency order matters more
than module file count. Maintain [foundation contracts](architecture.md) throughout.

| Phase | Deliverable and dependencies | Exit evidence |
| --- | --- | --- |
| 0 — Foundation acceptance | Reconfirm current picker/chat/runtime tests and gallery-off/consumer feature compatibility; record actual native baseline; agree first API/limit defaults | Existing gates and baseline evidence; do not label target gaps complete |
| 1 — Editor and focus | Extract generic editor; scoped input/focus; compose back into chat; editor fixtures and two-editor/modal demonstration; depends on foundation | E-01–E-03, I-01, I-05, A-01–A-02; preserves existing basic behavior |
| 2 — Draft and transient interaction | Versioned submit acknowledgement, history/stash, completion/reference contract; richer picker/dialog/palette/help; depends on 1 | E-04–E-06, D-01–D-03, I-02, A-03–A-04; simulated host errors/races |
| 3 — Transcript projection and content | Stable message/part IDs, targeted updates, logical viewport, bounded layout/cache, rich text/code, search/timeline/selection; depends on 1–2 | C-01–C-03, C-05–C-06, I-03–I-04; measured long stream/edit fixture |
| 4 — Activity and human review | Generic tools/progress/cancel acknowledgements; permission/question forms, queues/stale handling; depends on 2–3 | R-01–R-05, P-02; composed stream/edit/review/recovery acceptance |
| 5 — Browsing and inspection | Session/resource browsers, trees, status/notices, code/diff inspection; depends on 2–4 | D-04–D-06, C-04; composed content/sidebar/session scenarios |
| 6 — Inline and native runtime | Inline region adapter, explicit close errors, suspend/resume/editor handoff, native compatibility campaign; shares picker/review from 2/4 | T-01–T-03, T-06, P-01; normal-output preservation and native evidence |

Terminal sanitation and theme/capability work (**T-04–T-05**) advance alongside
phases 1–5 as affected rendering contracts are added; they cannot wait until the
runtime phase. Every phase adds gallery coverage (**G-01–G-04**, **P-03**) and
maintains side-effect/headless boundaries (**P-04**). Measurements (**G-05**)
start in phase 3 before rich rendering expands. Limited inline prototyping may
start earlier in temporary fixtures, but accepted reusable runtime depends on
shared interaction contracts. Do not defer the first slice behind all native hosts.

## First implementation slice

Implement a bounded multiline `editor` used by current `Chat`, with grapheme
cursor movement, selection replacement, backspace/delete, atomic normalized
paste and undo/redo. Preserve 4 KiB default limit and current send/newline/cancel
behavior through an explicit event adapter. Add minimal scoped focus so only the
active editor gets typing/paste and requests the cursor. Do not redesign the
whole transcript, introduce a provider registry or integrate Drone yet.

Gallery adds independent editor scenarios plus existing chat with editable draft
during deterministic streaming. A two-editor fixture and a simple modal body
prove focus boundaries. Relevant public behavioral tests, buffer/style/cursor
checks, Unicode properties and PTY paste/resize/restoration supplement existing
tests. Compatibility checks compile gallery-disabled library and all consumer
optional dependencies. This slice addresses E-01–E-03, I-01/I-05 and A-01/A-02;
full submission acknowledgement/history/completion waits for phase 2.

## Traceable acceptance matrix

All criteria are proposed/unaccepted except narrow related baseline behaviors.
“Continuous” means each implemented slice must supply evidence for its surface;
it does not require every future component to exist before accepting a slice.

| Criteria | Owning contract / observable exit | Phase |
| --- | --- | --- |
| P-01 | [Product](product.md): same safe interaction in inline/full-screen | 6 |
| P-02 | Product: editable stream/review/rejection/disconnection | 4 |
| P-03 | Product: independently reachable components/compositions | Continuous |
| P-04 | Product: imports/headless paths remain independent | Continuous |
| A-01 | [System design](system-design.md): instance state/generation isolation | 1, continuous |
| A-02 | System design: leaves use public events/frame without I/O/operation render | 1, continuous |
| A-03 | System design: IDs/revisions reject ambiguous routing | 2–3 |
| A-04 | System design: API adapters/docs and compilation boundaries | 2, continuous |
| I-01 | [Interaction](interaction.md): single event consumer and valid modal focus | 1–2 |
| I-02 | Interaction: event kinds/binding conflicts/leader timing | 2 |
| I-03 | Interaction: logical anchors and large scroll range | 3 |
| I-04 | Interaction: logical copy/selection, drag and clipboard failure | 3 |
| I-05 | Interaction: keyboard/hints/fallback/tiny-area access | 1, continuous |
| E-01 | [Composer](composer.md): Unicode movement/replacement/delete | 1 |
| E-02 | Composer: bounded undo/redo and atomic CR/CRLF paste | 1 |
| E-03 | Composer: limits, valid ranges and cursor bounds | 1 |
| E-04 | Composer: submit/ack/rejection/newer-version races | 2 |
| E-05 | Composer: history/stash/editor conflict preservation | 2; native handoff 6 |
| E-06 | Composer: generated completion/reference coherence | 2 |
| C-01 | [Conversation](conversation.md): interleaved/stale identified updates | 3 |
| C-02 | Conversation: provisional rich text/code/table/unknown fallback | 3 |
| C-03 | Conversation: follow/unread/reflow/paging/cache bounds | 3 |
| C-04 | Conversation: diffs and file/hunk identity through resize | 5 |
| C-05 | Conversation: search scope/timeline/stale result restoration | 3 |
| C-06 | Conversation: long projection measurements and explicit omissions | 3, continuous |
| R-01 | [Activity](activity.md): concurrent tool/run/connection states | 4 |
| R-02 | Activity: cancel request and host completion/interrupt races | 4 |
| R-03 | Activity: exact review identity/no implicit affirmative/expiry | 4 |
| R-04 | Activity: single/multi/typed question staged/reviewed response | 4 |
| R-05 | Activity: queued/stale review isolation and no authority effects | 4 |
| D-01 | [Dialogs](dialogs.md): topmost input/focus/generation/resize | 2 |
| D-02 | Dialogs: rich picker identity/loading/disabled/index adapter | 2 |
| D-03 | Dialogs: palette/help/completion consistency | 2 |
| D-04 | Dialogs: session search/actions/races without storage | 5 |
| D-05 | Dialogs: safe lazy tree navigation without host I/O | 5 |
| D-06 | Dialogs: responsive statuses/notices/unknown/clock | 5 |
| T-01 | [Terminal](terminal.md): lease/failure/close/drop/TTY cleanup | 6; foundation maintained |
| T-02 | Terminal: inline surrounding output and full-screen return | 6 |
| T-03 | Terminal: suspend/resume/editor error/fallback | 6 |
| T-04 | Terminal: complete/fragmented control sanitation and cell mapping | 1–3, continuous |
| T-05 | Terminal: semantic palette/no-color/default/ASCII/motion | 1–5, continuous |
| T-06 | Terminal: recorded native matrix and unclaimed profiles | 6 |
| G-01 | [Gallery](gallery.md): catalog/scenarios/no external effects | Continuous |
| G-02 | Gallery: reset/step/lifetime and observable intents | 1–2, continuous |
| G-03 | Gallery: meaningful behavioral/render/property/native evidence | Continuous |
| G-04 | Gallery: composed streaming/overlay/resize/failure isolation | 2–5 |
| G-05 | Gallery: approved cold/warm/resize/memory/fairness measurements | 3, continuous |

## Acceptance procedure

For every delivery slice: implement only that slice, run its engineering checks,
provide the exact command and numbered human test steps, then stop. The user
confirms whether the tested behavior works. A failed or ambiguous result keeps
the slice open; investigate and fix that slice, rerun affected checks and present
the revised test steps. Do not commit or begin another slice while its human
gate is pending. After explicit confirmation, use the commit skill to commit
only the reviewed Rust TUI scope in both owning repositories, then continue to
the next slice. Do not push. User-reported platform limits remain visible rather
than becoming assumed acceptance for other hosts.

Foundation engineering checks passed during the scoped package commits
`33fe70a` (private) and `2a6bc0f` (public): formatting, strict Clippy, ten
all-feature nextest tests including the Unix PTY test, seven gallery-disabled
tests, doctest, API docs, dependency audit, release build, consumer optional
feature compilation and Markdown/diff checks. Dependency auditing reported
allowed duplicate-version warnings. Foundation human acceptance passed on
2026-10-09; the user deferred visual refinement. These results do not accept
expanded capability contracts or establish visual design acceptance.

Each slice records implemented criteria, named fixtures, commands/results and
remaining limits. Run owning Rust gates from [Code quality](code-quality.md),
library without gallery, and affected consumer compilation. Request host access
for PTY/signals and run heavy suites separately. A TestBackend snapshot does not
accept terminal restoration; consumer compilation does not accept product UI.
User guides change only when behavior ships. Final Markdown link/diff review is
required, but does not substitute for behavioral evidence.

Consumer rollout is separately authorized work: Drone maps its adapter events,
capabilities, requests and drafts into views while retaining process/session
ownership. Gate/Shield choose explicit inline entry points and preserve headless
interfaces. Their own specs/tests must accept policy/execution/output behavior;
library completion alone does not establish it.

## Unresolved design decisions

| Decision | Proposed starting point / evidence needed | Needed before |
| --- | --- | --- |
| Public signatures and compatibility | Concrete value/update/action types, adapters for existing APIs; compile examples | Each affected slice |
| Word boundaries and undo coalescing | Unicode-aware words, grapheme safety, explicit transactions; editable Unicode fixtures | Phase 1 API stabilization |
| Projection/draft/cache/undo limits | Keep existing defaults first; add total/part/cache bounds; measure pathological content | Phases 1 and 3 |
| Input map/leader/mouse defaults | Scoped defaults with traditional newline fallback; gallery usability and native traces | Phase 2 / pointer acceptance |
| Reference edit semantics | Atomic token removal/replacement and explicit payload summaries | Phase 2 |
| Rich renderer/syntax dependencies | Bounded prototype, readable fallback, license/deny review; no network in render | Phase 3 |
| Search normalization/table/split width | Literal loaded-only default; Unicode/table/diff fixtures before broader policy | Phases 3 and 5 |
| Review panel versus overlay | Preserve draft and inspect details at 40×12; host-supplied scope/close/queue policy | Phase 4 |
| Inline anchoring/output result | Short bounded region, safe fallback on unsupported cursor behavior; PTY surrounding-text evidence | Phase 6 |
| Native Windows harness/IME/assistive matrix | Native runner/emulator and explicit testing conditions; separate unsupported states | Phase 6 claims |
| Performance targets | Proposed gallery budgets require measured baseline and approval, not assumed guarantees | Phase 3 acceptance |

These decisions do not block writing the contract or authorize silent product
policy choices. Deferred capabilities include image protocols, terminal emulation,
audio/OS notifications, dynamic plugins, authentication, session storage and Git
undo/apply. Generic views may expose supplied descriptors/intents for them.

## Remaining gap summary

Current library has picker/chat, reusable multiline editing/focus, semantic
dark/light styling and a lease. Slice 1 implementation awaits its human gate.
The largest gaps are identified transcript updates and
anchors, rich content, acknowledged draft/run/review lifecycle, browsers/overlays,
inline mode and broad native acceptance. Gallery and tests need to grow with each
capability. The editor/focus slice performs no new product integration.
