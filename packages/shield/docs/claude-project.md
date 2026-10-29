# Connect Claude Code and Codex

Shield setup registers native hooks and MCP tools for a project. Hooks check
supported tool requests; MCP lets the agent inspect policy and propose changes.
For your first installation, follow [the setup guide](getting-started.md).

## Which part does what?

| Part | What you use it for |
| --- | --- |
| Native hooks | Check supported requests from the agent's ordinary tools against active policy. |
| Shield MCP tools | Let the agent inspect policy, read guides, evaluate diagnostic requests, and prepare reviewed changes. |
| Operator CLI | Initialize the project and confirm settings or policy changes yourself. |

After setup, restart and trust the agent so it loads both integrations.
Use `shield history list` to inspect recorded requests and completion.

## Install or repair an integration

Initialize the project with `shield init`, then run `shield mcp setup --agent claude` in an operator terminal to install native
command hooks and Shield's project MCP entry. Confirmation shows the owned files,
binary and project. Claude uses `.claude/settings.local.json` and `.mcp.json`.
To add Codex as well, run `shield mcp setup --agent codex`; Codex uses
`.codex/hooks.json` and `.codex/config.toml`. No user/global settings are changed.
When active rules ask for [Codex prefix rules](policy-rules.md#codex-prefix-rules),
Shield also writes `.codex/rules/shield.rules`. Setup and every activation rewrite
it, removing it when no such rule remains or Codex is removed. Codex loads it
only for a trusted project. Shield never replaces a file at that path that it
did not write; status and the activation receipt say to move such a file aside.
Status also reports a stale file and hand edits, and `shield mcp setup`
repairs them. Until then, Codex Asks block whatever the Ask fallback, because
the file may let Codex skip its sandbox. Setup and policy reviews list the
prefixes a change adds or removes.

Other hook groups, MCP servers, plugins, settings and Codex rules files survive
installation and removal. Repeating setup creates one registration per agent.
Missing entries can be repaired by selecting the agent again. Edited owned
entries conflict; Shield preserves them and requires resolution. `shield mcp setup`
without `--agent` resumes a confirmed interrupted plan. `shield mcp setup --remove`
removes the agents you select, and with every agent selected removes all owned
registrations while preserving policy, approvals and history.

## Load the hooks

Restart the agent, trust the project, enable hooks, and approve the MCP
connection. In Codex, review hook definitions through `/hooks`. Shield cannot
override host trust or administrator restrictions.

Setup's Tool permissions page marks tools that can run without prompting.
All except `policy_apply` are marked by default; later setup preserves
your choices. Codex uses a `prompt` server setting with `approve` for marked
tools. Claude adds `mcp__shield__<tool>` entries to `permissions.allow` and
`shield` to `enabledMcpjsonServers` in `.claude/settings.local.json`.

MCP policy changes need separate approval. Claude opens a confirmation form:
Accept applies, while decline or cancel leaves the proposal pending. The form
works in `default`, `auto`, and `bypassPermissions` modes, independently of
ordinary tool approval. Clients without form elicitation cannot apply.
Codex uses its tool approval prompt; `never` approval policy refuses apply.
See [MCP review](project-mcp.md#proposals-and-operator-review).

Repeating setup repairs missing tool approvals. Unmarking a tool or removing
Shield deletes only approvals Shield added; other servers and existing approvals
are preserved. Check `shield doctor --json` for missing or changed files and
binaries. Status reports live verification as `unverified`.

## Intercepted requests

PreToolUse checks requests against the registered project's policy.

| Request | Check |
| --- | --- |
| Claude Read | File read. |
| Claude Write, Edit, MultiEdit, NotebookEdit | File write. |
| Claude Glob, Grep | Read of the search path. |
| Claude Bash and monitor commands | Shell policy. |
| Codex `view_image` | Image-file read; calls naming another environment block. |
| Codex `webrun` searches and direct URL opens | Web policy, as described below. |
| Bound project MCP tool calls | MCP policy using the stable project connection ID and full arguments. |

Configure these bindings through [project MCP connections](mcp-connections.md).
Unbound servers retain their host behavior. Bound calls whose project settings
drift are blocked until setup is reviewed again. Codex's inner MCP calls from
code mode use the same per-call hook path.

Session-management tools pass through to the host's permissions. For Claude,
these include tool loading, task lists and stopping tasks, questions, plan mode,
scheduling, subagents, workflows, agent listing, skills, and MCP resource listing.
Invoked tools are checked normally, except in remotely running subagents or
workflows. Codex passes through plans, questions, permission requests, tool search,
timing, subagents, and MCP resource listing.

Unsupported tools ask in Claude and block in Codex. Examples include MCP resource
reads; Claude also asks for session messages, worktrees, LSP, PowerShell, and
artifacts. Tool messages name supported alternatives where available. A rule
cannot add support for an unmodeled tool.

### Web requests

Codex `webrun` supports `search_query` and `open` with direct HTTP(S) URLs,
including mixed batches of up to 16 operations. Every operation must pass.
An optional `lineno` only positions the view. Opaque result references, clicks,
and other actions block the entire call. Submit a direct URL when available.
The hook sees the initial URL, not later redirect destinations.

### Protected files and policy changes

Claude can read and commit `.shield/config.yml`, `rules.yml`, and `shield.lock`,
but cannot edit them or read `.shield/review/`. Reading agent settings or
`.mcp.json` asks because they may hold credentials. `.shield/.gitignore` excludes
private review state and cache files. Change configurable protection with
[`shield protection set`](project-review.md#protected-files).

Agents cannot run Shield's operator CLI. They inspect policy through MCP,
prepare proposals, and submit `policy_apply` with the returned
`apply_arguments`. You can also review proposals with `shield policy review`.
A proposal changes no permissions until accepted.

The first relevant Ask supplies candidate pack/rule guidance and a proposal
route separately from the user-facing permission message. Ordinary allowed
Claude requests and Shield MCP hook checks add no extra model context.
For project scripts, inspect definitions, lifecycle hooks, and arguments before
proposing a rule.

Shell denials name up to three denied command forms and their matching rule IDs
when that evidence is available. Inspect those rules with `rule_list` before
proposing an allowance. A command can be denied for its argument form even when
its input file is readable; folder-access suggestions require a matching
outside-location failure.

Claude Ask reads and shell requests use its permission prompt or SDK callback;
without a permission host, they block. Stored grants belong to the separate
[SDK consumer](claude-read-only.md) and cannot authorize native-hook retries.
Arguments containing only an unresolved exit-status expansion (`$?` or `${?}`),
such as `echo "exit $?"`, keep ordinary approval instead of requiring a rewrite.
Shield does not guess the status or propose exact literal arguments for it.
Explicit command denials still block these requests; other unresolved expansions
retain their existing rewrite requirements.

### File reads

Read-only text filters can use file-read policy when their only command match is
a yielding fallback or no rule matches. The supported `jq` forms include string
interpolation, arrays and objects (including shorthand fields such as
`{id, decision}`), slices, selections, regular-expression tests and output formats
such as `@tsv`. Environment access, module loading, extra-input
helpers and file-loading options keep ordinary command review. Explicit command
Ask/Deny rules retain precedence.
Pagers such as `less` and `more` require command approval or a command rule;
permission to read their input files does not authorize their execution.

### Temporary and session files

Claude and Codex native adapters include the system temporary folders in the
read/write location unions. On Unix this includes existing `/tmp` and `/var/tmp`
roots plus the native per-user temporary directory; macOS aliases resolve to
their actual directories. Existing and newly created temporary files need no
folder proposal or session ownership check. Explicit command/rule denials and
protected Shield files still apply. Removing or replacing a temporary root
itself is blocked. This access does not edit project `access_paths`.

Temporary environment overrides do not open the home directory, its ancestors
or its descendants, including `.ssh`, `.claude` and `.codex`. Shield checks the
resolved directory, so a symlink alias cannot bypass that boundary. Custom
scratch under home follows ordinary project access or an explicitly reviewed
`access_paths` grant.

For Claude native hooks, the read union also includes the current session's
`tool-results` directory when its transcript path matches the registered project,
session ID and configured Claude directory. This allows inspection of persisted
tool output through Read or supported text filters. Outside temporary folders,
other sessions, settings, credentials and writes receive no extra access through
this session scope. Directory or operand symlinks
cannot redirect this scope outside the verified artifact directory. The scope is
observed afresh for each callback and is unavailable to MCP diagnostic input.

Status lists supported working-file categories. Runtime assets and scratch
folders outside temporary roots are not automatically discovered.

### Shell analysis

Bash source retains flags, quoting and compounds for conservative shell analysis.
A literal `cd` to the already known current directory, including `cd .`, keeps
that working-directory context and needs no extra approval unless an explicit
command rule asks or denies it. Other directory changes retain conservative
handling; returning to the original path after an unresolved change does not
restore known context.
Heredocs and here-strings supply inline data without an extra approval requirement.
The command, output destinations, substitutions and shell interpreter bodies
still follow their own policy checks, so writing a quoted heredoc to an approved
project file can proceed when both the command and write are allowed.
Standalone `[` and `]` in tests such as
`if [ -f AGENTS.md ]; then cat AGENTS.md; fi` preserve the working-directory
evidence for the read. Shield checks every branch against policy; an existence
test does not authorize the read or skip checks on a missing file.
Codex permitted requests return brief model context without an explicit Allow
response or argument rewrite; Ask fallback retains the original policy decision.
Exact shell rules match the complete supplied vendor input and available context.
Protected-path checks resolve existing symlink targets, including ancestors of new files.
Agent shell tools cannot launch Shield's operator CLI or its literal
`mcp serve` startup sequence under another executable name. Use the registered
Shield MCP tools to propose changes and request approval.
Allow permits the command; Deny blocks it. Unmatched requests ask. When the only
explanation for an Ask is source Shield cannot inspect, such as `$(…)`, `eval` or
`sh -c`, the hook blocks the request and tells the agent which part to rewrite as
literal, separate commands; the rewritten request then gets its actual decision.
### Codex approval handling

Codex PreToolUse cannot request native approval. Setup preserves a saved fallback
or defaults a new choice to **allow** for every profile, including Strict. The
final review shows it without a separate picker. Allow includes unmatched
commands and explicit Ask rules; Deny, unsupported tools and protection failures
still block, and so does an Ask on a command a Shield prefix rule covers, which
Codex would run outside its sandbox without asking. Repeating `shield mcp setup --agent codex`
preserves a saved Block or Allow choice. History retains
policy Ask and the configured handoff, without recording individual approval.
Automatic Ask allowance is suspended while a confirmed setup plan needs repair;
run `shield mcp setup` to finish that plan.

Codex also installs PermissionRequest for approvals Codex independently raises.
Shield evaluates the request again against current policy. Deny blocks; Ask
returns no decision, leaving Codex's approval flow in control. Policy Allow can
resolve the approval using the available host context. An explicit workdir takes
precedence; otherwise Shield uses Codex's reported session cwd for all supported
commands. Missing workdir or tool IDs do not change policy Allow into Ask.
Fresh policy Allow approves without a prompt or Codex rules file; Ask and Deny
still apply. History records missing execution context as incomplete while
retaining the policy decision and its actual handoff. The reported session cwd
is the evaluation context, not proof of a hidden per-command execution directory.
With the blocking fallback, an Ask stopped at PreToolUse never reaches this event.
Neither abstention nor completion establishes that a person approved a request.

`node --check FILE` and `node -c FILE` are syntax checks, with an optional `--`
before the single filename. This applies to bare `node`, `/bin/node` and
`/usr/bin/node`; other executable paths follow ordinary command policy.
When Node has no specific rule beyond a yielding Ask
fallback, Shield decides the check through file-read policy. An ordinary readable
project source can therefore pass without an Ask fallback or a Node permission
pack. Explicit command Ask/Deny rules, outside-folder rules and protected files
still apply. Extra runtime flags, stdin and multiple filenames retain ordinary
command evaluation.
When supplied environment metadata contains Node preload options in `NODE_OPTIONS`
(`--require`/`-r`, `--import`, `--loader` or `--experimental-loader`), shell syntax
checks also retain execution review because startup code can run.

Submitted relative paths use known request cwd. After a cwd change the analyzer
cannot resolve, relative file redirections and supported destructive file operations
block because Shield cannot verify their targets. Ordinary file moves into the
project root follow policy; replacing or moving protected configuration blocks.
Redirections to `/dev/null` and between numbered descriptors, such as
`2>/dev/null` or `2>&1`, open no project file and need no policy.

Finite literal `for` loops check their commands through normal policy lookup.
A supported loop and its resolved `break` or `continue` do not require approval
on their own; an explicit policy restriction still applies. For example, if
`node src/server.js` needs approval inside a loop, Shield names that invocation.
Unresolved command arguments or unsupported control flow can separately require
a rewrite or approval.
Using `PATH` as the loop variable separately asks because it changes which
executables the commands select.

For a simple background command such as `node src/server.js &`, Shield tracks
the runtime PID's origin through `$!`, `pid=$!`, and whole-variable copies such
as `copy="$pid"`. A supported `kill "$copy"` can therefore reach the normal
Ask flow instead of requiring a rewrite for an unknown operand. Known IFS must
preserve a single numeric field for unquoted targets; quoting avoids splitting.
Overwrites, uncertain branch values, unsupported mutations, arithmetic, indirect
references and values read from files do not establish this provenance.
Only positive PID references with supported literal signal options are modeled;
process-group and name-based targets retain ordinary evaluation.
The PID remains symbolic: Shield does not know its number or verify that the
original process is still alive. This analysis never grants policy Allow for
the kill; configured Ask handling still applies, including Codex's Allow fallback.
Runtime ownership enforcement requires a runner that owns the child process.

### Suggestions and history

Native agents can receive an initial pack suggestion when a retained catalog
advertises the analyzed command. One pack can advertise several executable names;
multiple matches are alternatives to inspect, rather than a single chosen pack.
Suggestion guidance uses short names such as the `node` rule pack in conversation;
tool arguments retain the full reference. Publisher and registry names distinguish
packs that share a short name.
The full registry/publisher/name reference leads to `pack_info`,
`policy_propose` and user review through `policy_apply`. A relevant
pack may still ask or deny the invocation. Suggestions preserve the current
decision and activate nothing; see [catalog refresh](local-registry.md#suggestions-for-commands).

With the blocking fallback, Codex explains that native approval is unavailable.
The message can offer a rule for the exact command, web search or fetched host,
or an outside folder. The agent submits that change through `policy_apply`
for your approval, then retries after activation. CLI users can also use
`shield history list` and `shield rule add` directly. `shield request approve` does
not authorize a native hook retry. Policy Deny requires a policy change and cannot
be overridden by ordinary host approval. Web searches and fetches follow web
rules; see [policy rules](policy-rules.md). Git/Docker effect hints do not override
Allow merely because indirect effects are possible. Hooks use the submitted
command and available context; they do not verify execution.
The [native run](native-run.md) flow remains a separate checked execution path.

Shield MCP inspection and proposals remain available. Neither a proposal nor a
diagnostic Allow authorizes a command. PostToolUse and Claude PostToolUseFailure
record available observations without output/error payloads. After you
complete an Ask call, PostToolUse reports the host completion and why the call
asked. Completion alone does not prove individual approval. Claude can inspect
`suggestion_list` after the task for eligible recurring reviews. Native completions
are unlinked when the pre-call receipt is unavailable. Shell exit success is
unknown unless supplied by a supported mapping.

## Failure boundary and coverage

An explicit native denial prevents dispatch. The installed command also exits 2
when the Shield process cannot start. A host that skips, disables, kills or times
out the hook may continue execution; installation cannot promise fail-closed
protection for those outcomes. Direct same-user processes remain outside isolation.

The current implementation installs Unix command hooks. Windows setup refuses.
Other agents have no installed native integration. Native hooks cannot consume
the separate runner's stored approval grants.

Claude evaluates policy before showing the prompt; Shield cannot revalidate a
changed policy when that host later accepts the answer. Native completion records
do not establish who approved the request. Codex's exec_command hook omits workdir
and other execution options, so its reported cwd may differ from the command's
effective workdir. Shield records the context the host actually supplies.
