# Build consistent CLI prompts

Use `canonic_tui_kit::runtime::prompts` for ordinary CLI questions. It owns the
inline terminal, keyboard handling, presentation profile, sizing and cleanup.
Your product supplies choices and defaults, validates answers, prints complete
reviews and applies operations after confirmation. JSON, MCP, hooks and piped
invocations must stay on their noninteractive paths.

## Pick an API

| Task | API |
| --- | --- |
| One choice by index | `choose` or `select` |
| One stable choice ID with details | `choose_choices` |
| Several stable choice IDs | `choose_many` |
| A guided step with Back distinct from cancellation | `Host::pick` with `PickerPrompt` |
| A structured question with a typed alternative and validation | `Host::question` |
| Multiline text | `Host::edit` |
| A supplied permission review | `Host::review` |
| Single-line or hidden input over caller-owned storage | `read` |
| MCP setup action, agents and final confirmation | `setup` |

The convenience choice functions use the same `Host` and `PickerPrompt` as
guided flows. Choice and question answers leave a short settled line in
scrollback. Multiline editing records only that input was received. `read`
does not print its answer or keep history; use it for secrets and zeroize the
caller-owned buffer afterward. Structured questions are for nonsecret answers.

## Keep a guided choice when returning to a step

This function selects an environment. Keep `prompt` alive across steps if the
user can return: it retains the search query, highlighted row and marks.

```rust
use canonic_tui_kit::{
    picker::Choice,
    runtime::prompts::{Host, Outcome, PickerPrompt},
};
use std::io;

fn environment() -> io::Result<Outcome<Vec<String>>> {
    let mut prompt = PickerPrompt::single(
        "Environment",
        "Choose where to run the preview.",
        vec![Choice::new("local", "Local"), Choice::new("staging", "Staging")],
        Some("local"),
    )?.back(true);
    Host::open(12)?.pick(&mut prompt)
}
```

`Outcome::Submitted` carries confirmed stable IDs. `Back` means Escape left the
step; the product returns to the previous page, or cancels at the first page.
`Cancelled` means Ctrl-C/Ctrl-D or a cooperative host interruption. Neither
outcome submits a default. The `back` flag controls the footer label.

For multiple choices use `PickerPrompt::multiple` or `choose_many`. Space marks
the highlighted row even while filtering; Enter confirms the marked set. A
nonzero minimum keeps an empty selection open with a notice. A zero minimum
accepts empty and names that action in the footer. Escape clears a nonempty
filter before leaving the step. Numbers highlight or mark; only deliberate
Enter submits. Repeated Enter events and insufficient visible space cannot
confirm an answer.

`Host::question` supports multiple choices or a typed alternative, but refuses
their combination before opening a terminal. Its `Reply` carries either choice
IDs or typed text. Use Question Form directly when both are required together.

## Compose MCP setup

Use the `setup` pages in this order:

1. `action()` chooses **Install or repair** or **Remove owned registrations**
   when flags have not already selected an operation.
2. `agents(choices, marked, remove)` shows **Select agents** with at least one
   required mark. Supply registration status and disabled reasons in `Choice`.
   Removal starts with no marks; install defaults come from the product.
3. Ask **Tool permissions** using a multiple-choice prompt with minimum zero
   when the selected hosts support tool approvals. Removal and dry runs skip it.
4. Print the complete product review to normal output after the prompt closes.
5. `confirm(context)` shows **Apply MCP setup?**, defaulting to **Cancel**.
   In this interactive flow, **Apply reviewed changes** submits confirmation
   intent for the product to validate before publication.

The `action_prompt`, `agent_prompt` and `confirmation_prompt` constructors
return the same pages for consumers that attach checkpoints or retain page
state. Explicit agent and operation selections skip the relevant selection
pages; they select intent. Product-owned unattended modes remain outside this
terminal contract. A confirmed repair journal can resume through its product's existing
recovery boundary. Initialization may allow skipping agent configuration with
zero marks; that is separate from the setup agent page's minimum of one.

Keep supported agent lists, discovery, per-tool approval defaults, file formats,
ownership checks, plan preparation and publication in the product. Shield also
reviews native hooks, policy settings and Box handover. These facts belong in
descriptors and complete reviews, not alternate picker implementations.

## Terminal lifecycle and custom screens

Prompt presentation uses `runtime::profile::detect`, including `NO_COLOR`,
RGB/indexed profiles and ASCII locales. The shared host measures content and
recomputes height as the window or filter changes. Unsupported terminals report
an error; there is no handwritten input fallback.
Picker height may grow to 16 rows; `Host::open` supplies the initial height.

An application already holding a terminal must release or hand off its lease
before opening a prompt. Drone uses `TerminalSession::handoff` for this. Custom
chat/gallery screens can render components directly under their existing lease.
They still use the shared components' keyboard and presentation contracts.

`Host::with_checkpoint` attaches an existing native cooperative signal owner.
The callback receives the current `InlineSession`; it can pause/resume that
lease or return `io::ErrorKind::Interrupted` to cancel. It must restore any lease
it consumes on failure and must not acquire a second one. The shared host adds
no process signal handlers. Ordinary keyboard suspension releases modes and
retains component state; Ctrl-Alt-Z suspends, and plain Ctrl-Z does so where it
is not editor undo.

If a checkpoint consumes the lease and returns an error, it also owns any final
settled line. The host cannot print that line after losing the lease.

Normal completion explicitly restores terminal modes. Operational and cleanup
errors remain observable together; Drop is a best-effort backstop. Full native
terminal compatibility needs its own acceptance; state/render tests establish
neither host trust nor configuration publication.
