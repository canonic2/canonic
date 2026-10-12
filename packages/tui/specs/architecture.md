# Current package architecture

`permission_review.rs` owns identified choices, exact scopes, inspection, retry
and host-only outcomes. Its model validates material descriptors, input owns
deliberate confirmation/search and Dialog qualification, and view reuses tool
output classification for complete source inspection. Both review components
re-export the private shared opaque ticket primitive in `review_ticket.rs`.
`question_form.rs` owns structured staged answers and response/copy tickets;
its model validates host snapshots, input isolates Editor and cancellation, and
view renders the bounded above-dock panel and separate Dialog viewport.
`review_queue.rs` owns the host-ordered active/waiting set, one-second arrival
deferral, reorder/withdrawal and single-drain intents. Chat's reviews adapter owns
receipts and saved logical viewport/follow restoration; Composer remains the same
instance. `gallery/reviews.rs` supplies deterministic acknowledgement simulations.

`session_browser.rs` owns host snapshots, stable-ID navigation and isolated
row/discovery request lifetimes. Its model validates resource bounds and material
revisions; input composes Editor, Choice Picker and Dialog/Confirmation; view owns
responsive rows, fixed loading/error feedback and the read-only preview. It shares
the private opaque ticket primitive with reviews. Chat exposes an opt-in session
entry intent; `gallery/sessions.rs` retains independent Chat instances, switches
only after actual simulated acknowledgements and never performs session storage.

This is the implemented package/module map. [System design](system-design.md)
owns architectural rules and proposed capability boundaries; component and
feature specs own detailed behavior and limits. [Inventory](inventory.md)
records implementation gaps; [Verification](verification.md) records evidence.

## Package and consumers

`canonic-tui` is an unpublished Rust library with an optional `gallery`
feature and `tui-gallery` binary. Drone, Gate and Shield declare optional local
path dependencies behind their `tui` features. There is no root Cargo workspace
or registry publication pipeline. Dependency compilation is distinct from a
consumer UI rollout.

## Implemented module map

| Source | Current responsibility | Owning contract |
| --- | --- | --- |
| [editor](../src/editor.rs) | Unicode text, input, transactions, atomic payload markers, layout and view | [Editor](components/editor.md) |
| [picker](../src/picker.rs) | Compatibility index picker and stable-ID ChoicePicker | [Option Picker](components/option-picker.md) |
| [dialog](../src/dialog.rs) | Validated stack, scoped tickets, timed arrival/guards/notices, caution qualification and measured chrome | [Dialog](components/dialog.md) |
| [commands](../src/commands.rs) | Descriptors, search/help views, bindings and remaps | [Command Palette](components/command-palette.md) |
| [completion](../src/completion.rs) | Ranked source snapshots, opaque queries, selection, progress/retry and fixed-height view | [Completion Menu](components/completion-menu.md) |
| [composer](../src/composer.rs) | Draft/submission, queue, recall/stash, payloads, completion, saved state and editor conflicts | [Composer](components/composer.md), [Completion Menu](components/completion-menu.md) |
| [chat](../src/chat.rs) | Identified rich transcript, revision/resync contracts, cached layout, anchors, loaded search/selection and fixed Composer | [Chat](components/chat.md) |
| [tool_call_card](../src/tool_call_card.rs) | Pure tool lifecycle/output model, local expansion, coalesced previews, ordered groups and opaque effect acknowledgements | [Tool Call Card](components/tool-call-card.md) |
| [run_activity](../src/run_activity.rs) | Host run snapshots, identified cancellation/options and independent transport indication | [Run Activity](components/run-activity.md) |
| [question_form](../src/question_form.rs) | Structured answers, validation/review, partial cancellation and host receipts | [Question Form](components/question-form.md) |
| [permission_review](../src/permission_review.rs) | Supplied scope/choices, inspection, deliberate confirmation and explicit retry | [Permission Review](components/permission-review.md) |
| [session_browser](../src/session_browser.rs) | Dedicated conversation browsing, discovery/action tickets and safe deletion | [Session Browser](components/session-browser.md) |
| [review_queue](../src/review_queue.rs) / [Chat reviews](../src/chat/reviews.rs) | Mixed host order, typing deferral, lifecycle isolation and saved viewport/receipts | [Permission Review queue](components/permission-review.md#shared-queue-and-chat-contract) |
| [focus](../src/focus.rs) | Instance-local target orders, modal scopes and return focus | [Input/navigation](features/interaction.md) |
| [text](../src/text.rs), [theme](../src/theme.rs), [ui](../src/ui.rs) | Private sanitized text/wrapping, semantic presentation and shared rules | [Presentation](presentation.md) |
| [runtime](../src/runtime.rs) | Interactive terminal lease, polling/drawing, explicit close and foreground handoff | [Terminal hosting](features/terminal.md) |
| [gallery](../src/gallery.rs) | Deterministic component/scenario composition and host simulation | [Gallery](features/gallery.md) |
| [lib.rs](../src/lib.rs), [main.rs](../src/main.rs) | Public exports/event-rendering types and gallery argument parsing/error reporting | [System design](system-design.md) |

## Composition and side effects

A consumer or gallery owns one terminal runtime and supplies events, frames,
areas, focus, presentation and elapsed time to components. Chat composes Composer
and Tool Call Card. `chat/tools.rs` adapts semantic card rows into anchored blocks;
group members keep lifetime-scoped source slots across growth and retry insertion.
Composer uses Editor, Option Picker, Completion Menu and Dialog. Commands share descriptor/binding
values with host dispatch. Detailed limits belong to the component specs.

Pure construction/update/render does not connect agents, execute commands, read
clipboard contents or write product storage. Composer's explicit foreground
editor adapter performs host-requested process/temp-file work; runtime owns the
terminal handoff. Both boundaries are opt-in and independently report failures.

## Existing limitations

Resource browsers, native inline hosting and broad platform acceptance
remain separately tracked work. No current module map implies those targets exist.
The terminal guard is process-local; it does not establish cross-process ownership
or forced-signal recovery. See the [component checkpoints](components/README.md)
and [feature index](features/README.md).
