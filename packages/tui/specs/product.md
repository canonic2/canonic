# Reusable terminal experience

Status: foundation required; expanded behavior proposed on **2026-10-09**.
See [Inventory](inventory.md) for evidence and [System design](system-design.md)
for module ownership.

## Purpose and scope

Provide terminal interactions developed and verified independently of a product.
Gate needs short choices, confirmations and inspection followed by
ordinary command output. Drone needs a composed full-screen conversation with
an editable draft, streaming content, tool activity and human review. Shared
components supply compatible interaction behavior; each host retains its decisions.

The gallery is the independent environment for every reusable component and
composed screen. Developers open one view, select a realistic state, feed events
and asynchronous updates, inspect actions, resize and switch presentation without
products, accounts, networks or actual permissions.

## Vocabulary

| Term | Meaning |
| --- | --- |
| Host | Application owning the loop, terminal lease, operations and authoritative data |
| Component | State machine and renderer for one bounded reusable interaction |
| Screen | Composition with explicit focus and layout |
| Projection | Bounded view derived from host data; never durable authority |
| Intent | User request returned to host; does not prove execution or authorization |
| Identity / revision | Opaque stable host ID / version for rejecting stale updates or actions |
| Draft | Editable text and explicit context references awaiting host acceptance |
| Inline | Temporary interaction in normal screen/scrollback, followed by command output |
| Full-screen | Interaction within a caller-owned alternate-screen lease |

## Intended workflows

### Short command interaction

1. Host explicitly selects interactive presentation after CLI and TTY checks.
   JSON, hooks, pipes and unattended paths use host-owned output.
2. Picker/review displays caller-supplied choices, explanation and safe default
   or no selection. Search and movement change highlight without execution.
3. Enter confirms an enabled choice. Esc cancels; closing never implies assent.
4. Host validates current operation identity/authority, performs the operation,
   and writes ordinary output after releasing terminal modes. Cleanup failure
   is reported through its error path.

Inline and full-screen use the same selection/review contract; the hosting
adapter differs. An embedded picker never acquires another lease. Plain consumer
interfaces remain independently usable if interactive presentation is unavailable.

### Conversation

1. Host supplies selected conversation, connection/run state, capabilities and
   bounded transcript. Empty/disconnected screens support editing and navigation
   according to explicit capabilities.
2. User edits multiline text, recalls history, stashes drafts and adds host-resolved
   references. Completion inserts/selects explicitly; pasted special prefixes
   never execute a command.
3. Submit returns draft identity/version. Acceptance clears that version;
   rejection preserves it and shows a reason. Busy queuing is a host decision.
4. Text/tool updates do not block editing. Follow keeps new output visible;
   inspecting history holds a logical anchor and shows unread activity.
5. Pending permission/question takes focus without losing the draft. Response
   carries request identity/revision; host validates, applies and acknowledges.
6. Interrupt requests cancellation of the identified run; UI stays pending until
   acknowledged. Session switching/reconnect are host operations. Late old-session
   events cannot change the new draft.

### Independent development

Select a named gallery fixture, reproduce its state with a controlled clock and
update sequence, inspect intents, then open a composition to check that same
component with overlays and streaming. Leaving ends fixture activity.

## Ownership boundaries

| Library | Host / Drone / agent adapter |
| --- | --- |
| Editing, focus, local filtering, selection and intents | Command spelling, availability, parsing and execution |
| Session rows, hierarchy and navigation | Create/resume/fork/delete/move, storage and synchronization |
| Permission/question forms and acknowledgement display | Native request translation, operator authority, grant scope/expiry and enforcement |
| Tool/code/diff/status presentation | Execution, Git snapshots, diagnostics collection and result meaning |
| Reference/attachment summaries | File search/reads, binary transfer, model capability and context inclusion |
| Connection/cancel status | Processes, protocols, retry policy, credentials, deadlines and descendants |
| Theme values/preference intents | Settings paths, persistence, discovery and deployment |

The library neither interprets a vendor protocol nor invents capabilities. A
generic tool card represents unfamiliar tools. Missing data appears as unknown.

## Scope decisions

Keep one unpublished Rust package, Ratatui/Crossterm and local optional consumer
dependencies. Components work without the chat screen or gallery feature. No
required workspace, background service, plugin loader or agent dependency exists.
New traits need an actual replaceable boundary or second implementation.

Attachments and links have text representations. Native image protocols, embedded
terminal emulators, audio playback, OS notification delivery, plugin installation
and IDE discovery remain deferred host-specific work. Full application coverage
informs [Inventory](inventory.md) without adding those operations to this library.

## Acceptance criteria

- **P-01:** Same picker/review contract works in inline and full-screen fixtures;
  cancellation produces no affirmative intent.
- **P-02:** Conversation remains editable while streaming and retains draft
  through review/rejection/disconnection, exposing explicit intents.
- **P-03:** Every reusable view/composition is independently reachable in the
  gallery without accounts, product executables, network or persistent mutation.
- **P-04:** Default-feature imports have no terminal effects; consumer plain output
  is independent of interactive feature availability. Each consumer verifies integration.
