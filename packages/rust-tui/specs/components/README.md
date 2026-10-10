# Component specifications

Each component has one owning contract. These public specs contain behavior and
implementation design details without referencing private design files.

| Component | Purpose | Current status |
| --- | --- | --- |
| [Editor](editor.md) | Unicode multiline editing, selection, undo and viewport | Implemented |
| [Option Picker](option-picker.md) | Filter and confirm supplied choices by stable identity | Implemented |
| [Dialog](dialog.md) | Stack caller-owned bodies with isolated input and restored focus | Foundations implemented; timing requirements pending |
| [Command Palette](command-palette.md) | Search host commands and shared keyboard help | Implemented |
| [Composer](composer.md) | Session draft, submission, queue, history, stash and editor handoff | Implemented reusable component |
| [Completion Menu](completion-menu.md) | Complete typed command/file prefixes from host candidates | Implemented inside Composer; standalone component pending |
| [Chat](chat.md) | Compose transcript and Composer with scrolling and delivery state | Plain bounded host implemented; rich conversation targets pending |

Focus, bindings, themes and the terminal runtime are shared infrastructure.
Session browsers, trees, panels and notices are proposed in
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

This summary reflects recorded scope on 2026-10-10, not a new Rust test run.
Definitions and dated results are in [Verification](../verification.md).
“Defined” means the current contract is written; it does not assert a full target
specification has been approved. Partial/pending entries keep outstanding work
visible. All five checkpoints must be assessed for the scope being handed over.

| Component | Specification | Design | Functionality | Engineering | Human acceptance |
| --- | --- | --- | --- | --- | --- |
| [Editor](editor.md) | Defined reusable editing contract | Defined; redesign review open | Implemented | Recorded for current implementation | Functional gate passed; redesign actions pending |
| [Option Picker](option-picker.md) | Defined stable-ID contract | Defined; visual follow-up open | Implemented | Recorded for current implementation | Functional gate passed; visual follow-up separate |
| [Dialog](dialog.md) | Defined; timing targets explicit | Partial: timing/inline transitions pending | Foundations implemented; timing pending | Recorded for foundations | Fallback visual review pending |
| [Command Palette](command-palette.md) | Defined palette/help contract | Defined current views | Implemented | Recorded for current implementation | Pending |
| [Composer](composer.md) | Defined complete local workflow | Defined layout/states/fallbacks | Implemented reusable workflow | [Recorded](../verification.md#composer-engineering-record--2026-10-10) | Complete component review pending |
| [Completion Menu](completion-menu.md) | Integrated contract defined; standalone API pending | Integrated view defined; standalone pending | Integrated route implemented; standalone pending | Recorded through Composer; standalone pending | Standalone review not presented |
| [Chat](chat.md) | Plain scope defined; rich targets proposed | Plain host defined; rich target decisions pending | Plain projection implemented; rich targets pending | Recorded for plain host; rich targets pending | Foundation accepted; rich targets not presented |
