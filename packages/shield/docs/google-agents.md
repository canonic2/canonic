# Connect Gemini CLI and Antigravity CLI

Gemini CLI and Antigravity CLI use Shield's shared policy engine, setup ownership,
proposal validation and session grants. Native checks cover macOS Gemini CLI
0.67.0 with a local scripted provider and logged-in Antigravity CLI 1.5.1.
Gemini's interactive Ask acceptance and cancellation are verified with the local
provider; Antigravity's ordinary interactive Ask acceptance and refusal are verified with the
logged-in CLI. Other-platform native acceptance remains separate.

From an initialized Shield project, run:

```sh
shield mcp setup --agent gemini
shield mcp setup --agent agy
```

Choose the agent you use, review the proposed settings, then restart it.
Gemini stores hooks and MCP in `.gemini/settings.json`. Antigravity uses
`.agents/hooks.json` and `.agents/mcp_config.json`. Setup preserves unrelated
entries and refuses changes to owned entries that you edited manually. Check
the result with `shield doctor`. Remove an integration with
`shield mcp setup --remove --agent gemini` or the corresponding `agy` command.
In a Hub or Box project, `shield mcp setup --agent hub` or `--agent box`
writes a shared definition and private binary pin in that host's folder. With
the matching build, `hub setup --adopt` or `box config sync --adopt` can take
over existing identical Google registrations. The host publishes their hooks
and MCP entries; native per-tool approvals in this definition apply to Claude
and Codex. Google clients retain their native permissions; in-session policy
confirmation has the limitations described below.

## Request coverage

Both integrations check mapped shell commands, file reads and writes, searches
and MCP calls through the shared policy. Unmapped internal tools retain the
agent's permission system. Neither integration replaces the host sandbox.

Gemini CLI 0.67.0 lets a tool call continue if Shield's pre-tool hook times out.
Shield installs a 10-second hook limit. When that limit is exceeded, Gemini
records the hook failure and continues under its native permissions without a
Shield decision. A hook timeout therefore leaves Shield enforcement unavailable
for that call.

Relative file paths and omitted shell directories use the available workspace
context. Explicit directories are honored. Gemini can verify its project directory
through its native project marker; otherwise its reported cwd is the best available
context. Searches and multi-file reads retain host permissions when additional
workspace roots are unknown. Fetch prompts check every extracted HTTP(S) target,
including the raw-content host for GitHub blob links.
Gemini fetch calls must supply either `prompt` or `url`, never both. Mixed inputs
are refused because the host's fetch modes select different fields.

Antigravity supports fresh shell commands in a single workspace and explicit
absolute `Cwd`. Reused terminals and ambiguous workspace defaults retain host
permissions after evaluating the modeled command; Shield does not automatically
approve them based on a guessed directory. Relative file paths in several open
Antigravity workspaces also retain host permissions; use an absolute path for a
complete policy check. Malformed mapped arguments receive recovery guidance.
Antigravity's `view_file_outline` and `view_code_item` receive file-read checks.
Input to an existing terminal through `send_command_input` is blocked; start a
fresh mapped command instead. Gemini's legacy `save_memory` is blocked; current
memory edits use its ordinary file tools. Extended Gemini glob syntax such as
`@(..)` and `+(..)` is refused; use explicit directories and ordinary globs.
Neither host supplies stable tool-call IDs, so Shield does not fabricate
before/after correlation.

Gemini returns native `ask` decisions; the headless fixture waits for confirmation
without returning the requested content. Antigravity uses `auto_approve` for a
fully modeled policy Allow and `force_ask` for Ask. Its ordinary headless mode
rejects unavailable approval, but `--dangerously-skip-permissions` bypasses
`force_ask`. Native Deny still blocks in that mode.

These integrations support Shield's MCP confirmation forms when the client can
present them. When confirmation is unavailable, the agent must close the
unapplied attempt and report the missing capability; it must not direct the user
to a terminal to accept a saved proposal. Closed reviews are retained for outcome
inspection, not later approval.
Gemini 0.67.0 does not advertise MCP forms. Antigravity CLI 1.5.1 advertises them
but cancels them without an approval UI in both interactive and headless operation,
including skip-permissions.
Ordinary native Ask dialogs are independent: Gemini supports acceptance and
cancellation even in yolo mode, and Antigravity's Yes action releases the
withheld read.

Gemini must trust the workspace before it loads project hooks. A successful
setup or `--skip-trust` alone does not establish that the running CLI loaded them.
The native test explicitly trusts only its isolated scratch workspace.
Antigravity doctor reports a disabled Shield hook as a diagnostic. Removing its
integration deletes an empty owned MCP configuration file and preserves unrelated
server entries and settings.

| Capability | Gemini CLI | Antigravity CLI |
| --- | --- | --- |
| Shared policy, setup/repair/removal, explicit MCP identity | Implemented | Implemented |
| Native Allow/Ask/Deny | Local provider and interactive Ask verified | Logged-in CLI and interactive Ask verified; skip-permissions bypasses Ask |
| Policy confirmation | Form adapter implemented; native 0.67.0 lacks forms | Form adapter implemented; native 1.5.1 cancels forms |
| Skills and current-session outputs | Editable skills; read-only session outputs | Editable skills; read-only session outputs |
| Session grant protocol | Shared implementation and transport fixtures | Shared implementation and transport fixtures |
| Hub- or Box-managed publication | Implemented; ownership fixtures | Implemented; ownership fixtures |
| Linux/Windows native acceptance | Not established | Not established |

## Skill folders

The integrations supply default temporary-folder access and read/write access
to their own skill libraries. Gemini recognizes project and user
`.gemini/skills` and `.agents/skills`; `GEMINI_CLI_HOME` selects the parent
containing the two user folders.

Antigravity recognizes project `.agents/skills` and legacy `.agent/skills`,
plus `~/.gemini/config/skills`, `~/.gemini/skills` and
`~/.gemini/antigravity-cli/skills`. Plugin libraries are not automatically
discovered. Skill files can be created and edited; credential siblings and agent
settings stay protected. Ordinary command and file policy still apply.
Symlinked library folders cannot appoint another protected namespace.

Skill editing does not require settings-change approval. Shield applies ordinary
file and command policy, but does not inspect skill content for executable hooks
or permission directives. Host-internal execution may occur outside intercepted
tool calls, and changes to user skill libraries can affect other projects.

Gemini also receives read-only access to its current session's output directory
under `.gemini/tmp`, after matching the transcript layout, session ID and
`.project_root` marker to the enrolled project. Its chats directory is not granted.
Antigravity receives its current conversation's brain directory only after
matching the workspace, conversation ID, artifact directory and transcript layout.
Sibling sessions, substituted symlinks and writes receive no automatic access.
