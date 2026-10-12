# Component design alignment

## Scope and evidence

The current handoff expands existing components and adds Run Activity, Question
Form and Permission Review. Complete the reusable contracts, behavior, host
composition, deterministic scenarios and engineering checks for each component.
Human visual acceptance and consumer/platform acceptance remain independent.

| Scope | Current implementation | Remaining work |
| --- | --- | --- |
| Shared profiles | Disabled role, plain ANSI 16 line info, updated palettes, independent ASCII/NO_COLOR and eight gallery profiles | Remaining component frames, reduced-motion hosting and terminal lifecycle fixtures |
| Editor | Ten-entry kill ring/yank cycle, word-part and whitespace kills, line joining, redo bindings, IME limits, selection line counts, cursor context and edge fixtures | Human visual acceptance and broader input-method/platform acceptance |
| Option Picker | Match-class ranking, host min/max, empty-source behavior, glyph profiles, inspection gating and refreshed marks | Human visual acceptance |
| Tool Call Card | Nine states, host progress/tasks/artifacts, safe controls, ID-preserving reorder and 79 gallery scenarios | Human visual acceptance and real host adapters |
| Run Activity | Identified cancel/options, cancellation races, independent connection state, 140 ms animation, reduced motion with injected host seconds, compact action keys and twenty gallery scenarios; expanded engineering checks passed | Human visual acceptance and real transport adapters |
| Dialog | Stack, timed arrival/guard/notices, attention intents, close arming, caution policies and 36 named scenarios | Human visual acceptance and native inline-screen hosting |
| Command Palette | Match-class ranking, captured opening height, unavailable/conflict wording, timed overlay input and 25 interactive frames | Engineering verified; human visual acceptance and host execution |
| Completion Menu | Source tickets, alias ranking, distinct timeout warning, Alt-R retry, source labels, separate activity/footer help and 30 interactive frames; expanded engineering checks passed | Human visual acceptance and real discovery/execution |
| Composer | Identified host-confirmed work controls, fixed activity ordering, typed file references, queue/shell/editor recovery, complete keys and 99 interactive scenarios; expanded engineering checks passed | Human visual acceptance and real host adapters |
| Chat | Rich transcript, explicit pending/failure acknowledgements, serial paging/retry, quiet notes, host suggestions, fallback search and 76 interactive frames; expanded engineering checks passed | Human visual acceptance and real host adapters |
| Question Form | Structured answers, validation, review, partial cancellation, identified host acknowledgements, expiry/copy, above-dock Chat and 52 live scenarios | Human visual acceptance and real host adapters |
| Permission Review | Standalone contract, exact scopes, safe defaults, explicit confirmation/retry, inspection, mixed queue, Chat and 56 live scenarios | Human visual acceptance and real host authority adapters |
| Session Browser | Dedicated screen, stable IDs, search/paging, row actions, safe deletion, retained Chat instances and 72 live scenarios | Human visual acceptance and real host storage/action adapters |
| Terminal lifecycle | Existing alternate-screen runtime and PTY restoration | Inline result scrollback, plain-output/handoff/error/quit frames and wider platform acceptance |

## Open decisions

Ctrl-R retains Composer history search; Alt-R is the completion retry key.
Quit-arming message placement and ANSI 16 reverse versus cyan selection await a design decision.
Keep existing routing/presentation until a decision changes the owning contract.

## Next design work

Tree Browser is the next design-team handoff in the [roadmap](roadmap.md),
covering file/diff and parent/child task navigation. Session Browser is implemented;
TUI development is not finished.
