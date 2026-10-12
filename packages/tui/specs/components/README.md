# Component specifications

Each component has one owning contract. These public specs contain behavior and
implementation design details without referencing private design files.

| Component | Purpose | Current status |
| --- | --- | --- |
| [Editor](editor.md) | Unicode multiline editing, selection, undo and viewport | Implemented |
| [Option Picker](option-picker.md) | Filter and confirm supplied choices by stable identity | Implemented |
| [Dialog](dialog.md) | Stack caller-owned bodies with isolated input, guarded arrival and restored focus | Stack/timing/caution policies and 36 gallery scenarios implemented |
| [Command Palette](command-palette.md) | Search host commands and shared keyboard help | Implemented |
| [Composer](composer.md) | Session draft, submission, queue, history, stash and editor handoff | Implemented reusable component |
| [Completion Menu](completion-menu.md) | Complete typed command/file prefixes from host candidates | Standalone state/view, Composer integration and 30 gallery states implemented |
| [Chat](chat.md) | Rich transcript, identified streaming, search, selection and fixed Composer dock | Implemented with 76 interactive scenarios |
| [Tool Call Card](tool-call-card.md) | Identified tool states, measured progress, child tasks/artifacts and acknowledged effects | Reusable cards, Chat integration and 79 interactive scenarios implemented |
| [Run Activity](run-activity.md) | Whole-run status, identified cancellation and connection independence | Reusable state/view, Chat composition and twenty scenarios in Tool Call Card |
| [Question Form](question-form.md) | Staged choices/text, review, cancellation, host acknowledgements and queued above-dock focus | Implemented with shared queue, Chat and 52 scenarios; human acceptance pending |
| [Permission Review](permission-review.md) | Deliberate identified choices, exact scopes, inspection, retry and host outcomes | Implemented with mixed queue, Chat and 56 scenarios; human acceptance pending |
| [Session Browser](session-browser.md) | Stable session rows, local/remote search, acknowledged actions, paging and dedicated-screen return | Implemented with 72 live scenarios; human acceptance pending |
| [Tree Browser](tree-browser.md) | Stable trees, lazy children/paging, filtering, acknowledged actions and host placement | Implemented with 66 live scenarios; human acceptance pending |

Focus, bindings, themes and the terminal runtime are shared infrastructure.
Session and tree browsers have standalone contracts. Panels and notices are proposed in
[Browsing](../features/browsing.md). See the [specifications index](../README.md) for
package-wide contracts and [scenario coverage](../features/gallery-designs.md) for fixtures.

## Contract format and ownership

Use the same section order in each component spec: Purpose, Current status,
Behavior and state transitions, Implementation design, Host boundary,
Acceptance criteria, Remaining gaps. Add focused subsections for keys, states,
limits and responsive/color fallbacks where needed. Label proposed behavior at
its scope; an implemented foundation does not imply its full target is complete.

Each requirement has one owner. Editor owns generic cursor/selection/undo;
Composer owns recall and submission; Option Picker owns choice navigation;
Completion Menu owns completion tickets/insertion; Command Palette owns shared
descriptor consistency. Other specs link to these requirements. Stable acceptance
IDs are defined only by their owner and referenced by the roadmap and reviews.

## Completeness checkpoints

Each row describes its recorded component scope.
Definitions and dated results are in [Verification](../verification.md).
“Defined” means the current contract is written; it does not assert a full target
specification has been approved. Partial/pending entries keep outstanding work
visible. All five checkpoints must be assessed for the scope being handed over.

| Component | Specification | Design | Functionality | Engineering | Human acceptance |
| --- | --- | --- | --- | --- | --- |
| [Editor](editor.md) | Defined reusable editing contract | Defined; redesign review open | Implemented | Recorded for current implementation | Functional gate passed; redesign actions pending |
| [Option Picker](option-picker.md) | Defined stable-ID contract | Defined; visual follow-up open | Implemented | Recorded for current implementation | Functional gate passed; visual follow-up separate |
| [Dialog](dialog.md) | Defined stack/timing/caution contract | Defined panels/arrival/confirmation; inline hosting separate | Reusable stack, timed policies and caution implemented | [205 tests and host checks](../verification.md#dialog-alignment-engineering-record--2026-10-10) | Visual review pending |
| [Command Palette](command-palette.md) | Defined ranking/help/host contract | Defined docked/profile views | Reusable view and 25 scenarios implemented | [Expanded checks passed](../verification.md#command-palette-alignment-engineering-record--2026-10-10); latest consumer checks pass | Visual review pending |
| [Composer](composer.md) | Defined local workflow and identified host controls | Defined layout/states/fallbacks | Expanded recovery, typed file references and 99 scenarios implemented | [Expanded checks passed](../verification.md#composer-alignment-engineering-record--2026-10-11) | Complete component review pending |
| [Completion Menu](completion-menu.md) | Defined complete source/insertion contract | Defined layout/states/fallbacks | Standalone component, Composer integration and 30 scenarios implemented | [Expanded checks passed](../verification.md#completion-menu-alignment-engineering-record--2026-10-11) | Complete component review pending |
| [Chat](chat.md) | Defined complete local contract | Defined content/states/fallbacks | Expanded acknowledgements, paging and 76 scenarios implemented | [Expanded checks passed](../verification.md#chat-alignment-engineering-record--2026-10-11) | Complete component review pending |
| [Tool Call Card](tool-call-card.md) | Defined complete local contract | Defined states/output/actions/fallbacks | Implemented reusable cards and Chat/group integration | See [verification record](../verification.md#tool-call-card-engineering-record--2026-10-10) | Complete visual review pending |
| [Run Activity](run-activity.md) | Defined run/connection contract | Defined states/cancellation/fallbacks | Implemented reusable state and Chat composition | [Expanded checks passed](../verification.md#gallery-profiles-and-run-activity-engineering-record--2026-10-11) | Pending |
| [Question Form](question-form.md) | Defined full structured answer contract | Defined review/field/cancellation/profiles | Reusable form, shared queue, Chat and 52 scenarios | [Review checks](../verification.md#question-form-and-permission-review-engineering-record--2026-10-11) | Human visual review pending |
| [Permission Review](permission-review.md) | Defined complete review/host contract | Defined scopes/inspection/queue/profiles | Reusable review, mixed queue, Chat and 56 scenarios | [Review checks](../verification.md#question-form-and-permission-review-engineering-record--2026-10-11) | Human visual review pending |
| [Session Browser](session-browser.md) | Complete host/view contract | Dedicated screen, actions, deletion and profiles defined | Reusable browser, Chat entry and 72 live scenarios | [Engineering verified](../verification.md#session-browser-engineering-record--2026-10-12) | Human visual review pending |
| [Tree Browser](tree-browser.md) | Complete host/view contract | Files/tasks, placement, lifecycles and profiles defined | Reusable tree, Chat entry and 66 live scenarios | [Engineering verified](../verification.md#tree-browser-engineering-record--2026-10-12) | Human visual review pending |
