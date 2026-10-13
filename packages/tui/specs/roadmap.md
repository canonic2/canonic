# Implementation plan and acceptance

Status: delivery now completes each component's specifications, implementation,
gallery scenarios and relevant engineering checks before moving to the next
component, as requested on 2026-10-10. Human visual acceptance and explicit commit
authorization remain separate from passing tests.

Editor/focus and rich picker have recorded acceptance. Dialog foundations and
commands are implemented; Dialog supplies timed arrival and caution policies. Composer implements
its full local workflow: submission, queue, history, stash, chips, completion,
saved-state codec, external-editor conflicts and foreground terminal handoff.
Consumers own actual execution, durable storage and clipboard integration.
Completion Menu implements reusable source-aware queries, insertion, streaming,
retry and its dedicated gallery. Chat implements identified rich content,
streaming/resync, anchors/paging, search, selection/copy and 76 scenarios.
Tool Call Card implements identified activity/output, ordered groups and selected
copy/details/retry with host-owned execution. Its 79 gallery scenarios include
progress, child tasks/artifacts, host reorder and whole-run cancellation races.
Question Form implements staged answers, review, cancellation/partial delivery,
host acknowledgement races and above-dock Chat with 52 live scenarios.
Permission Review implements explicit choices, scope/destructive confirmation,
inspection, host outcomes and the mixed queue with 56 live scenarios.
Session Browser implements dedicated navigation/search/paging and acknowledged
row actions with retained Chat instances and 72 live scenarios. Permission policy
and broader native acceptance remain separate. The bounded inline host and
composition fixture are implemented in phase 6.
Tree Browser supplies lazy files/tasks, stable folds, filtering with ancestors,
acknowledged effects and 66 live scenarios; full engineering gates passed.
Code/Diff Inspection supplies bounded code/unified patches, navigation, folds,
loaded find, selection, identified copy/editor/load results and 51 live scenarios.

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
| 5 — Browsing and inspection | Session/resource browsers, trees, status/notices, code/diff inspection; depends on 2–4 | D-04–D-06, composed content/sidebar/session scenarios |
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
| C-04 | [Chat](components/chat.md#acceptance-criteria): unified patch preview, hidden-hunk search and full-inspection intent | 3 |
| C-05 | [Chat](components/chat.md#acceptance-criteria): loaded search/block navigation and stale acknowledgement restoration | 3 |
| C-06 | [Chat](components/chat.md#acceptance-criteria): long projection measurements and explicit omissions | 3, continuous |
| R-01 | [Activity](features/activity.md#acceptance-criteria): concurrent tool/run/connection states | 4 |
| TC-01–TC-05 | [Tool Call Card](components/tool-call-card.md#acceptance-criteria): lifecycle, concurrency/output, focus/effects, gallery/native and engineering evidence | 4 |
| RUN-1–RUN-7 | [Run Activity](components/run-activity.md#acceptance-criteria): host snapshots, cancellation races, retry, connectivity, motion and options | 4 |
| R-02 | [Activity](features/activity.md#acceptance-criteria): cancel request and host completion/interrupt races | 4 |
| R-03 | [Activity](features/activity.md#acceptance-criteria): exact review identity/no implicit affirmative/expiry | 4 |
| R-04 | [Activity](features/activity.md#acceptance-criteria): single/multi/typed question staged/reviewed response | 4 |
| R-05 | [Activity](features/activity.md#acceptance-criteria): queued/stale review isolation and no authority effects | 4 |
| QF-01–QF-07 | [Question Form](components/question-form.md#acceptance-criteria): staged/typed/review/cancellation, identified host delivery, queue and verification | 4 |
| PR-01–PR-06 | [Permission Review](components/permission-review.md#acceptance-criteria): deliberate/exact-scope confirmation, lifecycle races, mixed queue and inspection | 4 |
| D-01 | [Dialog](components/dialog.md): topmost input/focus/generation/resize | 2 |
| DG-02–DG-05 | [Dialog](components/dialog.md#acceptance-criteria): timed arrival/guard, app closure, notices/attention and caution qualification | 2 |
| D-02 | [Option Picker](components/option-picker.md): identity/loading/disabled/index adapter | 2 |
| D-03 | [Command Palette](components/command-palette.md): palette/help/completion consistency | 2 |
| CP-02–CP-04 | [Command Palette](components/command-palette.md#acceptance-criteria): ranking/recency, captured height, unavailable/alias feedback and effective scoped help | 2 |
| CM-02–CM-04 | [Completion Menu](components/completion-menu.md#acceptance-criteria): alias ranking, source warning/retry, fixed height and dock/status separation | 2 |
| D-04 / SB-01–SB-06 | [Session Browser](components/session-browser.md#implementation-ownership-and-acceptance): stable rows, search/actions/paging/races and Chat return without storage | 5 |
| D-05 / TB-01–TB-05 | [Tree Browser](components/tree-browser.md#ownership-and-acceptance): lazy normalized trees, identity/folds/filter, host outcomes and responsive placement | 5 |
| CI-01–CI-05 | [Code/Diff Inspection](components/code-diff-inspection.md#acceptance-criteria): bounded views, logical navigation/search/copy, exact effects and caller return | 5 |
| SP-01–SP-05 | [Status Panels & Notices](components/status-panels-and-notices.md#acceptance-criteria): supplied states/facts, exact effects, expiry/persistence, responsive focus/profiles and native restoration | 5 |
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
| Broader projection/cache policy | Chat's bounded source/part/block/row/cache contracts and pathological tests are implemented; full export remains host-owned | Broader content/inspection scope |
| Pointer defaults and native key encodings | Scoped keyboard/leader rules are specified; establish pointer defaults and unverified terminal encodings | Pointer/native acceptance |
| Standalone inspection dependencies | Read-only code/unified diff uses existing Unicode, Editor, theme and frame contracts; no additional dependency | Phase 5 |
| Unloaded search and split diff policy | Inspection uses loaded Unicode literal smart-case search and unified single-file views; missing context and split diff remain outside its scope | Implemented inspection scope |
| Review panel versus overlay | Review components use the above-dock queue and local confirmation dialogs; broader placement changes need a new design decision | Future placement changes |
| Inline anchoring/output result | Short bounded region, safe fallback on unsupported cursor behavior; PTY surrounding-text evidence | Phase 6 |
| Native Windows harness/IME/assistive matrix | Native runner/emulator and explicit testing conditions; separate unsupported states | Phase 6 claims |
| Performance targets | Proposed gallery budgets require measured baseline and approval, not assumed guarantees | Phase 3 acceptance |

These decisions do not block writing the contract or authorize silent product
policy choices. Deferred capabilities include image protocols, terminal emulation,
audio/OS notifications, dynamic plugins, authentication, session storage and Git
undo/apply. Generic views may expose supplied descriptors/intents for them.

## Remaining gap summary

Question Form, Permission Review and their shared above-dock queue are implemented.
Session/Tree Browser and Code/Diff Inspection are implemented. Other browsers,
broader native acceptance and real host adapters remain separate work. Composer's
submission, history/stash, queue and editor handoff are implemented local scope.
Current per-component gaps belong in the [component contracts](components/README.md),
and feature gaps in the [feature contracts](features/README.md).

Status Panels & Notices is implemented with responsive placement and explicit
clock/action ownership. Inline terminal hosting now has a bounded reusable lease,
ordered print ledger, measured region sizing, retained borrowed views and a native fixture. The bounded
delivery continues with human visual/native acceptance and documented host gaps.
Both native gallery hosts have Unix Ctrl-Alt-Z/external SIGTSTP suspension and
foreground resume. Final visual/platform acceptance remains separate from
engineering verification; further runtime work follows observed acceptance gaps.
Additional browsers and
application features require a concrete consumer need rather than extending
this component delivery indefinitely.
