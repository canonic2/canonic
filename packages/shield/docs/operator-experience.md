# Shield in the terminal

Shield guides policy changes through terminal prompts. Inspection commands
print results directly. For setup, follow [your first project](getting-started.md).

## What initialization reviews

`shield init` asks for a baseline profile and agent integrations, then shows
the policy and settings changes for confirmation. Recommended is preselected.
`shield init --profile strict` selects Strict directly; `--no-baseline` skips the
default pack addition. Existing selected baseline profiles are preserved.
Codex setup preserves a saved Ask fallback or defaults a new choice to Allow for
every profile. The final review shows the fallback and its scope for confirmation.

The baseline comes from the pinned main registry, regardless of your default
source. Its artifacts, dependencies, and catalog checkpoint activate together;
a failed download or validation leaves a new project unenrolled. Use
`shield pack remove baseline` to remove it or `shield pack add main/baseline` to add it.

## Controls and output

Prompts appear below existing output and preserve scrollback. Arrows move the
highlight; Enter confirms. In multi-select lists, Space marks choices and Enter
continues. The footer shows the available keys.

Esc goes back and keeps your answers, or cancels on the first step. Ctrl-C
cancels from any step. Cancellation changes nothing and exits 130. Final
confirmation defaults to Cancel. Text fields support editing, undo, paste, and
Ctrl-J for newlines; oversized input is rejected.

Onboarding lists Claude Code and Codex as independent checkboxes and preselects
those with executable CLIs on `PATH`, without starting the tools. All agents
remain available to select or deselect; aliases and tools outside `PATH` are not
detected. `--agent` selections bypass detection, and `--no-setup` skips agent
setup. Space or a number toggles each agent; Enter continues with the marked
agents. With none marked, Enter skips agent setup for now. Removal is offered
only by `mcp setup --remove`. `--agent claude --agent codex` selects every
registered agent directly, for `init` and `mcp setup` alike.

When enrollment is absent but generated Shield host entries remain, onboarding
can reclaim entries that exactly match the current binary, project and generated
configuration. The final review shows the captured files and adoption before any
write. Edited/unrecognised entries still conflict and name the configuration to
inspect; a label alone never establishes ownership.

On Unix, suspension releases terminal modes; continuing restores the prompt
and draft. Between prompts, signals keep their usual effect. Prompts require
cursor support and enough space: redirected stdio, `TERM=dumb`, failed cursor
reports, and tiny terminal regions cannot confirm. Set `NO_COLOR` for monochrome
output. Native terminal behavior is verified on macOS.

Reviews list what changes: packs added (`+`), removed (`-`) or changed (`~`)
with their version, profile, rule count and short digest, custom rules, access
paths, and how command decisions change, with one example of a stricter or
broader decision. Initialization and setup reviews summarize each agent's hooks,
MCP server and Codex Ask fallback, name the Shield binary the hooks run, and
list each owned file as created, updated, unchanged or removed; Shield only
changes its own entries there, so file contents are not repeated. Digests show
their first 12 characters. Installation and cache status are reported separately
from live interception.

Errors name the failure, cause, and recovery step on stderr. Recovery commands
do not run automatically. Cancellation prints `Cancelled; nothing changed.`
Redirected stderr, `TERM=dumb`, and `NO_COLOR` disable ANSI styling.

`--json` prints the complete result for agents and scripts: every field, full
digests and identifiers, provenance and unchanged items. JSON errors carry their
`code` and `message` plus `details` (the cause) and every `recovery` step.
Policy-changing operations requiring confirmation refuse JSON/unattended
mutation, and Shield has no `--yes` to accept a review unattended.
`mcp setup --dry-run --json` returns the setup review without writing. Agents prepare changes as MCP proposals and request host approval with
`policy_apply`, or the operator can confirm with `shield policy review`.
`cache prepare --json` can rebuild disposable
derived data without activating source edits. Native hooks, MCP and SDK
interfaces never acquire this operator UI or emit human presentation.

## Command workflows

| Command | Workflow |
| --- | --- |
| `init` | Review default baseline and project configuration, select agents, confirm enrollment |
| `mcp setup [--agent AGENT]... [--remove] [--dry-run]` | Install, repair or remove complete project hooks/MCP integrations, including fallback settings; omission opens guided selection or resumes a confirmed repair, removal keeps unselected agents; restart/trust the agent afterwards |
| `status`, `doctor` | Inspect active policy, history, registrations and reported repair needs |
| `pack search`, `pack info` | Discover releases and inspect source, compatibility, profiles and rules before installation |
| `pack add`, `pack update`, `pack remove` | Acquire or select verified artifacts; review the complete policy comparison before activation. `pack add` asks for a new pack's profile (Recommended preselected) unless `--profile` is given |
| `pack list`, `pack show` | List retained selections or inspect their exact source and artifact |
| `pack configure` | Set a selection's variable values and rule overrides, guided or with `--set`, `--unset`, `--override` and `--clear-override` |
| `registry list/add/remove/default [--scope user]` | Inspect or confirm project or user discovery changes independently of installed policy |
| `policy show`, `policy review` | Inspect policy; or review a proposal/manual edit, repair projections or configure history |
| `rule list`, `rule add/edit/remove` | Inspect rules or build/edit a validated rule and confirm the resulting policy |
| `request list`, `request show` | Inspect pending or unused grants and an exact request's scope |
| `request approve`, `request deny` | Review one exact stored request before one-time approval, denial or revocation |
| `history list`, `history show` | Inspect filtered observations and available decision/completion details |
| `protection set` | Choose agent access to protected settings and policy files |
| `cache prepare` | Prepare only the committed generation's compiled data |

Use `--help` for command arguments and `--project PATH` for another project.
Project commands require enrollment; user-scope registry commands do not. See
[troubleshooting](troubleshooting.md) for recovery by symptom.
