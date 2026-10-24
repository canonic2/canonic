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
| [Tool Call Card](tool-call-card.md) | Identified tool states, measured progress, child tasks/artifacts and acknowledged effects | Reusable cards, Chat integration and interactive gallery scenarios implemented |
| [Run Activity](run-activity.md) | Whole-run status, identified cancellation and connection independence | Reusable state/view, Chat composition and gallery scenarios in Tool Call Card |
| [Question Form](question-form.md) | Staged choices/text, review, cancellation, host acknowledgements and queued above-dock focus | Implemented with shared queue, Chat and gallery scenarios; human acceptance pending |
| [Permission Review](permission-review.md) | Deliberate identified choices, exact scopes, inspection, retry and host outcomes | Implemented with mixed queue, Chat and gallery scenarios; human acceptance pending |
| [Session Browser](session-browser.md) | Stable session rows, local/remote search, acknowledged actions, paging and dedicated-screen return | Implemented with live gallery scenarios; human acceptance pending |
| [Tree Browser](tree-browser.md) | Stable trees, lazy children/paging, filtering, acknowledged actions and host placement | Implemented with live gallery scenarios; human acceptance pending |
| [Code/Diff Inspection](code-diff-inspection.md) | Read-only supplied code/patches, navigation, folds, find, selection, host effects and return | Reusable inspector and live gallery scenarios; human acceptance pending |
| [Status Panels & Notices](status-panels-and-notices.md) | Supplied status sections, explicit expiry, persistent errors and acknowledged actions | Reusable component, Chat adapter and live gallery scenarios; human acceptance pending |

Focus, bindings, themes and the terminal runtime are shared infrastructure.
Session/tree browsers and status panels/notices have standalone contracts.
See the [specifications index](../README.md) for
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

Each row describes its recorded component scope. Checkpoint definitions and
test coverage are in [Verification](../verification.md); human status is in
[Human testing](../human-testing.md#acceptance-record).
“Defined” means the current contract is written; it does not assert a full target
specification has been approved. Partial/pending entries keep outstanding work
visible. All five checkpoints must be assessed for the scope being handed over.

| Component | Specification | Design | Functionality | Engineering | Human acceptance |
| --- | --- | --- | --- | --- | --- |
| [Editor](editor.md) | Defined reusable editing contract | Defined; redesign review open | Implemented | [Covered](../verification.md#editor-and-option-picker) | Functional gate passed; redesign actions pending |
| [Option Picker](option-picker.md) | Defined stable-ID contract | Defined; visual follow-up open | Implemented | [Covered](../verification.md#editor-and-option-picker) | Functional gate passed; visual follow-up separate |
| [Dialog](dialog.md) | Defined stack/timing/caution contract | Defined panels/arrival/confirmation; inline hosting separate | Reusable stack, timed policies and caution implemented | [Covered](../verification.md#dialog) | Visual review pending |
| [Command Palette](command-palette.md) | Defined ranking/help/host contract | Defined docked/profile views | Reusable view and gallery scenarios implemented | [Covered](../verification.md#command-palette) | Visual review pending |
| [Composer](composer.md) | Defined local workflow and identified host controls | Defined layout/states/fallbacks | Expanded recovery, typed file references and gallery scenarios implemented | [Covered](../verification.md#composer) | Complete component review pending |
| [Completion Menu](completion-menu.md) | Defined complete source/insertion contract | Defined layout/states/fallbacks | Standalone component, Composer integration and gallery scenarios implemented | [Covered](../verification.md#completion-menu) | Complete component review pending |
| [Chat](chat.md) | Defined complete local contract | Defined content/states/fallbacks | Expanded acknowledgements, paging and gallery scenarios implemented | [Covered](../verification.md#chat) | Complete component review pending |
| [Tool Call Card](tool-call-card.md) | Defined complete local contract | Defined states/output/actions/fallbacks | Implemented reusable cards and Chat/group integration | [Covered](../verification.md#tool-call-card-and-run-activity) | Complete visual review pending |
| [Run Activity](run-activity.md) | Defined run/connection contract | Defined states/cancellation/fallbacks | Implemented reusable state and Chat composition | [Covered](../verification.md#tool-call-card-and-run-activity) | Pending |
| [Question Form](question-form.md) | Defined full structured answer contract | Defined review/field/cancellation/profiles | Reusable form, shared queue, Chat and gallery scenarios | [Covered](../verification.md#question-form-and-permission-review) | Human visual review pending |
| [Permission Review](permission-review.md) | Defined complete review/host contract | Defined scopes/inspection/queue/profiles | Reusable review, mixed queue, Chat and gallery scenarios | [Covered](../verification.md#question-form-and-permission-review) | Human visual review pending |
| [Session Browser](session-browser.md) | Complete host/view contract | Dedicated screen, actions, deletion and profiles defined | Reusable browser, Chat entry and 72 live scenarios | [Covered](../verification.md#session-browser) | Human visual review pending |
| [Tree Browser](tree-browser.md) | Complete host/view contract | Files/tasks, placement, lifecycles and profiles defined | Reusable tree, Chat entry and 66 live scenarios | [Covered](../verification.md#tree-browser) | Human visual review pending |
| [Code/Diff Inspection](code-diff-inspection.md) | Complete bounded read-only/host contract | Code/unified diff, navigation/search/copy/return and profiles defined | Reusable Inspector, Chat/tool/tree hosts and 51 live scenarios | [Covered](../verification.md#code-and-diff-inspection) | Human visual review pending |
| [Status Panels & Notices](status-panels-and-notices.md) | Complete supplied-facts/clock/action contract | Five states, responsive placement, notice lifecycle and profiles defined | Reusable component, opt-in Chat and 27 live scenarios | [Covered](../verification.md#status-panels-and-notices) | Human visual review pending |
