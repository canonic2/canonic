# Implementation plan and acceptance

Status: delivery now completes each component's specifications, implementation,
gallery scenarios and relevant engineering checks before moving to the next
component, as requested on 2026-10-10. Human visual acceptance and explicit commit
authorization remain separate from passing tests.

Editor/focus and rich picker have recorded acceptance. Dialog foundations and
commands are implemented; dialog timing remains separate. Composer implements
its full local workflow: submission, queue, history, stash, chips, completion,
saved-state codec, external-editor conflicts and foreground terminal handoff.
Consumers own actual execution, durable storage and clipboard integration.
Completion Menu implements reusable source-aware queries, insertion, streaming,
retry and its dedicated gallery. The next component is **Chat (conversation
view)**: rich identified transcript content, streaming, navigation and inspection.

## Delivery scope and phase order

Keep local unpublished library/gallery and optional consumer dependencies. No
publication, installation, agent integration or product UI rollout is added by
this plan. Complete one component with its owned state, host boundaries, scenarios and
relevant checks before broadening. Dependency order matters more
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

## Traceable acceptance matrix

Each acceptance ID has one owning contract below. This table maps requirements
to dependency phases; it is not a pass/fail record. Implemented scope and current
checkpoints are in [Components](components/README.md#completeness-checkpoints);
executed evidence and human acceptance are in [Verification](verification.md).
“Continuous” means each implemented scope supplies applicable evidence.

| Criteria | Owning contract / observable exit | Phase |
| --- | --- | --- |
| P-01 | [Product](product.md#acceptance-criteria): same safe interaction in inline/full-screen | 6 |
| P-02 | [Product](product.md#acceptance-criteria): editable stream/review/rejection/disconnection | 4 |
| P-03 | [Product](product.md#acceptance-criteria): independently reachable components/compositions | Continuous |
| P-04 | [Product](product.md#acceptance-criteria): imports/headless paths remain independent | Continuous |
| A-01 | [System design](system-design.md#acceptance-criteria): instance state/generation isolation | 1, continuous |
| A-02 | [System design](system-design.md#acceptance-criteria): leaves use public events/frame without I/O/operation render | 1, continuous |
| A-03 | [System design](system-design.md#acceptance-criteria): IDs/revisions reject ambiguous routing | 2–3 |
| A-04 | [System design](system-design.md#acceptance-criteria): API adapters/docs and compilation boundaries | 2, continuous |
| I-01 | [Interaction](features/interaction.md#acceptance-criteria): single event consumer and valid modal focus | 1–2 |
| I-02 | [Interaction](features/interaction.md#acceptance-criteria): event kinds/binding conflicts/leader timing | 2 |
| I-03 | [Interaction](features/interaction.md#acceptance-criteria): logical anchors and large scroll range | 3 |
| I-04 | [Interaction](features/interaction.md#acceptance-criteria): logical copy/selection, drag and clipboard failure | 3 |
| I-05 | [Interaction](features/interaction.md#acceptance-criteria): keyboard/hints/fallback/tiny-area access | 1, continuous |
| E-01 | [Editor](components/editor.md): Unicode movement/replacement/delete | 1 |
| E-02 | [Editor](components/editor.md): bounded undo/redo and atomic CR/CRLF paste | 1 |
| E-03 | [Editor](components/editor.md): limits, valid ranges and cursor bounds | 1 |
| E-04 | [Composer](components/composer.md): submit/ack/rejection/newer-version races | 2 |
| E-05 | [Composer](components/composer.md): history/stash/editor conflict preservation | 2; native handoff 6 |
| E-06 | [Completion Menu](components/completion-menu.md): generated completion/reference coherence | 2 |
| C-01 | [Chat](components/chat.md#acceptance-criteria): interleaved/stale identified updates | 3 |
| C-02 | [Chat](components/chat.md#acceptance-criteria): provisional rich text/code/table/unknown fallback | 3 |
| C-03 | [Chat](components/chat.md#acceptance-criteria): follow/unread/reflow/paging/cache bounds | 3 |
| C-04 | [Chat](components/chat.md#acceptance-criteria): diffs and file/hunk identity through resize | 5 |
| C-05 | [Chat](components/chat.md#acceptance-criteria): search scope/timeline/stale result restoration | 3 |
| C-06 | [Chat](components/chat.md#acceptance-criteria): long projection measurements and explicit omissions | 3, continuous |
| R-01 | [Activity](features/activity.md#acceptance-criteria): concurrent tool/run/connection states | 4 |
| R-02 | [Activity](features/activity.md#acceptance-criteria): cancel request and host completion/interrupt races | 4 |
| R-03 | [Activity](features/activity.md#acceptance-criteria): exact review identity/no implicit affirmative/expiry | 4 |
| R-04 | [Activity](features/activity.md#acceptance-criteria): single/multi/typed question staged/reviewed response | 4 |
| R-05 | [Activity](features/activity.md#acceptance-criteria): queued/stale review isolation and no authority effects | 4 |
| D-01 | [Dialog](components/dialog.md): topmost input/focus/generation/resize | 2 |
| D-02 | [Option Picker](components/option-picker.md): identity/loading/disabled/index adapter | 2 |
| D-03 | [Command Palette](components/command-palette.md): palette/help/completion consistency | 2 |
| D-04 | [Browsing](features/browsing.md): session search/actions/races without storage | 5 |
| D-05 | [Browsing](features/browsing.md#gallery-fixtures-and-acceptance): safe lazy tree navigation without host I/O | 5 |
| D-06 | [Browsing](features/browsing.md#gallery-fixtures-and-acceptance): responsive statuses/notices/unknown/clock | 5 |
| T-01 | [Terminal](features/terminal.md#acceptance-criteria): lease/failure/close/drop/TTY cleanup | 6; foundation maintained |
| T-02 | [Terminal](features/terminal.md#acceptance-criteria): inline surrounding output and full-screen return | 6 |
| T-03 | [Terminal](features/terminal.md#acceptance-criteria): suspend/resume/editor error/fallback | 6 |
| T-04 | [Terminal](features/terminal.md#acceptance-criteria): complete/fragmented control sanitation and cell mapping | 1–3, continuous |
| T-05 | [Terminal](features/terminal.md#acceptance-criteria): semantic palette/no-color/default/ASCII/motion | 1–5, continuous |
| T-06 | [Terminal](features/terminal.md#acceptance-criteria): recorded native matrix and unclaimed profiles | 6 |
| G-01 | [Gallery](features/gallery.md#acceptance-criteria): catalog/scenarios/no external effects | Continuous |
| G-02 | [Gallery](features/gallery.md#acceptance-criteria): reset/step/lifetime and observable intents | 1–2, continuous |
| G-03 | [Gallery](features/gallery.md#acceptance-criteria): meaningful behavioral/render/property/native evidence | Continuous |
| G-04 | [Gallery](features/gallery.md#acceptance-criteria): composed streaming/overlay/resize/failure isolation | 2–5 |
| G-05 | [Gallery](features/gallery.md#acceptance-criteria): approved cold/warm/resize/memory/fairness measurements | 3, continuous |

## Verification and consumer rollout

The [verification contract](verification.md) defines independent checkpoints and
records engineering evidence. [Human testing](human-testing.md#delivery-contract)
defines the review/confirmation procedure. This roadmap does not grant commit,
push or rollout authorization.

Consumer rollout is separately authorized work. Consumers map their own events,
capabilities and storage into reusable views while retaining execution/session
ownership. Their specs/tests accept policy, output and product UI behavior;
library completion or dependency compilation alone does not establish that.

## Unresolved design decisions

| Decision | Proposed starting point / evidence needed | Needed before |
| --- | --- | --- |
| Public signatures and compatibility | Concrete value/update/action types, adapters for existing APIs; compile examples | Each affected slice |
| Rich projection/cache limits | Current component limits are specified; approve total/part/cache bounds using pathological transcript measurements | Phase 3 |
| Pointer defaults and native key encodings | Scoped keyboard/leader rules are specified; establish pointer defaults and unverified terminal encodings | Pointer/native acceptance |
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

The next standalone component is Completion Menu. Dialog arrival/removal timing,
rich identified Chat content and anchors, structured activity/review, browsers,
inline hosting and broad native acceptance remain separate work. Composer's
submission, history/stash, queue and editor handoff are implemented local scope.
Current per-component gaps belong in the [component contracts](components/README.md),
and feature gaps in the [feature contracts](features/README.md).
