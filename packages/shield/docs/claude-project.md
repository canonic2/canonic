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

Run `shield mcp setup` for guided management. Choose **Install or repair** or
**Remove owned registrations**, mark at least one integration with Space on
**Select agents**, then press Enter. Installation includes a **Tool permissions**
page. Review the complete changes and choose **Apply reviewed changes** on
**Apply MCP setup?**; **Cancel** is highlighted by default. Unmarked agents keep
their configuration. Setup shows each integration's registration status, and
Box is unavailable until this folder is a Box project, and on Windows. During first-time
`shield init`, no marked agents means **Skip for now** instead.

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

## Set up through Box

In a project that uses Box, Shield can hand its integration to Box instead of
writing each agent's files. Box then publishes Shield to whichever agents each
developer chose with `box setup`, so nobody runs `shield mcp setup` per agent.

```sh
shield mcp setup --agent box
box config sync
```

`box` is the first row on the agent page, and in a folder with
`.box/config.yml` it is the only row marked until you set something up. Setup
writes two files and changes no other Box or agent file:

- `.box/mcp/shield.yml` is shared with everyone who uses the project. It names
  the bare `shield` command, so it holds no path from your machine. It tells Box
  to run Shield's MCP server for each agent, to allow the tools you marked on
  the Tool permissions page except `policy_apply`, which always asks, and to
  install Shield's hooks for Claude Code (`pre_tool_use`, `post_tool_use`,
  `post_tool_use_failure`) and Codex (`pre_tool_use`, `permission_request`,
  `post_tool_use`). The hooks start with `{command}`, which Box replaces with
  the command it launches for the server.
- `.box/local/mcp/shield.yml` is in Box's private local layer. It pins the
  Shield binary you ran setup with, so Box launches that binary and its hooks
  call it. Shield creates the folder owner-only when it is missing.

With both files Box writes the same hook entries a direct setup writes. After
confirming, run `box setup` the first time or `box config sync` later, then
restart and trust the agent as below.

Setup refuses `--agent box` when the project has no `.box/config.yml`; run
`box config init` first. It is not available on Windows, because Box hook
commands are POSIX shell commands. Shield adopts either file when it already
holds exactly what Shield would write, and never replaces one it did not
write.

`shield status` lists Box with the other integrations, and `shield doctor`
reports a file as missing or changed. `shield mcp setup --agent box` rewrites
both. `shield mcp setup --remove --agent box` deletes them only while they still
hold what Shield wrote; restore or delete an edited file yourself first.
Removing them does not remove entries Box already published; Box manages those.

Codex cannot ask at its hook, so the review shows the same
[Ask fallback](#codex-approval-handling) a direct Codex setup uses, Allow for a
new choice. Box carries it for the Codex hooks it publishes.

If Claude Code or Codex already has a direct Shield installation, installing
Box hands it over by default: Shield stops managing that installation and
leaves its hooks, MCP entry, approvals and settings exactly as they are. Run
`box config sync --adopt` so Box takes those identical entries over. Guided
setup asks first; choose **Keep direct installations** to keep managing them.
With `--agent box` the review lists the hand-over. Naming an agent too, as in
`shield mcp setup --agent box --agent claude`, keeps that agent's direct
installation. A kept direct installation can still be removed with
`shield mcp setup --remove`, which deletes its entries, including those Box
then uses. Box does not publish Codex's prefix rules file, so Shield keeps
writing it from your active policy while Codex is set up through Box.

Some direct settings have no Box form: Shield does not publish
[project MCP connections](mcp-connections.md) through Box, and Claude Code
cannot pre-approve `policy_apply`. Set an agent up directly
when you need these.

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
timing (including the host's `clocksleep` tool), subagents, and MCP resource listing.

In every native agent integration, unmodeled internal tools defer to the agent's
own permissions. Claude examples include MCP resource reads, session messages,
worktrees, LSP, artifacts and WebSocket watches. Codex examples include
`read_mcp_resource`, plugin installation and interactive terminal input. This
also covers app tools without an MCP mapping and future internal tools.
Shield returns no decision, including at a permission prompt; it does not approve
the tool or record a policy Allow. Named `view_image` environments and unmodeled browsing actions also defer
to Codex. Searches and direct HTTP(S) opens in a mixed browsing call still receive
Shield policy checks. Recognized shell, file and MCP requests remain checked;
malformed requests for those mappings still block. Encoded `mcp__server__tool`
calls retain connection and policy checks rather than the internal-tool fallback.
Shield does not analyze commands typed through `write_stdin` into a running
interpreter, or actions requested through Claude's `SendMessage`. A Deny rule for
a shell command does not enforce those delegated actions.

### Web requests

Codex `webrun` supports `search_query` and `open` with direct HTTP(S) URLs,
including mixed batches of up to 16 modeled operations. Every modeled operation
must pass. An optional `lineno` only positions the view. Opaque result references,
clicks, screenshots, find and other unmodeled actions defer to Codex permissions.
If a mixed call's modeled operations may proceed, Shield returns no decision
for the call, including at a permission prompt; modeled refusals still apply.
The hook sees the initial URL, not later redirect destinations.

### Protected files and policy changes

Claude can read and commit `.shield/config.yml`, `rules.yml`, and `shield.lock`,
but cannot edit them or read `.shield/review/`. Reading agent settings or
`.mcp.json` asks because they may hold credentials. `.shield/.gitignore` excludes
private review state and cache files. To let the agent make one blocked change,
accept its [one-time request](project-review.md#allowing-one-blocked-request) in
the conversation; change configurable protection with
[`shield protection set`](project-review.md#protected-files).

Agents can request command help, such as `shield protection --help`, under
ordinary command policy. Help requests consist of plain lowercase words followed
by `--help` or `-h`, without other options or paths. Operator actions remain
blocked through agent shell tools. Agents inspect policy through MCP,
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
Agent shell tools cannot run Shield's operator actions or launch its literal
`mcp serve` startup sequence under another executable name. Help-only requests
follow ordinary policy and protected-path checks. Use the registered
Shield MCP tools to propose changes and request approval.
Allow permits the command; Deny blocks it. Unmatched requests ask. When the only
explanation for an Ask is source Shield cannot inspect, such as `$(…)`, `eval` or
`sh -c`, the hook blocks the request and tells the agent which part to rewrite as
literal, separate commands; the rewritten request then gets its actual decision.
### Codex approval handling

Codex PreToolUse cannot request native approval. Setup preserves a saved fallback
or defaults a new choice to **allow** for every profile, including Strict. The
final review shows it without a separate picker. Allow includes unmatched
commands and explicit Ask rules; Deny, invalid requests and protection failures
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
`suggestion_list` when you ask to review recurring permission interruptions.
One-time permission requests do not authorize lasting policy changes. Shield
publishes no MCP skills; its tools include the review guidance and examples.
Native completions
are unlinked when the pre-call receipt is unavailable. Shell exit success is
unknown unless supplied by a supported mapping.

## Failure boundary and coverage

An explicit native denial prevents dispatch. The installed command also exits 2
when the Shield process cannot start. A host that skips, disables, kills or times
out the hook may continue execution; installation cannot promise fail-closed
protection for those outcomes. Direct same-user processes remain outside isolation.

On Windows, Claude Code setup installs direct executable hooks with blocking
failure handling. Codex setup writes encoded Windows command hooks. Claude's
`PowerShell` tool and Codex's native default use the PowerShell grammar: one
literal invocation of a supported native program. See the
[Windows development limits](development.md#native-windows-development) for
the supported names and quoting. Other PowerShell syntax and cmd requests block
even when Ask fallback is set to allow.
Other agents have no installed native integration. Native hooks cannot consume
the separate runner's stored approval grants.

Claude evaluates policy before showing the prompt; Shield cannot revalidate a
changed policy when that host later accepts the answer. Native completion records
do not establish who approved the request. Codex's exec_command hook omits workdir
and other execution options, so its reported cwd may differ from the command's
effective workdir. Shield records the context the host actually supplies.
