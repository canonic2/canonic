# MCP transport

Use [Agent setup](#agent-setup) to register Testrun. For your first target,
follow [Getting started](getting-started.md) and use the corresponding tools
below. For connection and recovery problems, see [Troubleshooting](troubleshooting.md).

## Tool catalog

Inspect the complete registered schemas with `testrun mcp serve --check`.
This prints the catalog without connecting to a target or starting the device
manager. The main tool groups are:

| Purpose | Tools |
| --- | --- |
| Bundled usage guides, offline | `docs_list`, `docs_search`, `docs_read` |
| Find and prepare devices | `target_list`, `doctor`, `developer_mode_reveal`, `developer_support_install` |
| Connect and act | `connect`, `act`, `observe`, `status`, `disconnect`; browser cancellation: `cancel` |
| Read the screen | `ui_snapshot`, `ui_find`, `ui_read`, `wait`, `clipboard_read` |
| Apps | `app_list`, `app_install`, `app_uninstall` |
| Project configuration | `config_init`, `config_check`, `config_show`, `config_schema`, `project_discover`, `project_init_preview`, `project_init`, `project_list`, `project_show` |
| Project workflows | `project_status`, `project_start`, `project_open` |
| Test builds | `build_start`, `build_status`, `build_log`, `build_cancel` |
| Capture | `preview_start`, `preview_stop`, `preview_status`, `recording_start`, `recording_stop`, `recording_status` |
| Viewer control | `control_status`, `control_acquire`, `control_release` |
| Playback | `media_status`, `media_send`, `volume_get`, `volume_set`, `audio_probe` |
| Network | `network_set` |
| Diagnostic investigations | `diagnostics_open`, `diagnostics_start`, `diagnostics_status`, `diagnostics_read`, `diagnostics_stop`, `diagnostics_close`, `diagnostics_remove` |
| Diagnostic analysis | `diagnostics_analyze`, `diagnostics_analysis_status`, `diagnostics_analysis_cancel`, `diagnostics_query`, `diagnostics_compare` |
| Diagnostic access | `diagnostics_capabilities`, `diagnostics_plan`, `diagnostics_prepare`, `diagnostics_project`, `diagnostics_export` |
| Browser | `browser_list`, `browser_doctor`, `browser_connect`, `browser_status`, `browser_send`, `browser_cancel`, `browser_disconnect` |

One tool set covers mobile and browser targets. Each target reports its own
capabilities. Use a known connected target directly. When connecting a device,
use the returned screenshot if it is current. Use `observe` when the view is
missing or stale. Unsupported requests are refused before input is sent. See the
[CLI guide](cli.md) for target selection and shared command behavior, and the
[browser guide](browser.md) for engine-specific support.

For a browser operation, keep the returned `operationId` from `browser_send`,
or `actionId` from a shared browser action. Poll `browser_status` with that
`operationId`, or shared `status` with that `actionId`, until the operation
settles. For a full browser snapshot with resources, call `browser_status` with
the browser's `targetId` and `includeDetails: true`. See
[browser status and cleanup](browser.md#mcp-and-cleanup) for the result fields.

## Bundled documentation

`testrun docs list`, `testrun docs search QUERY` and `testrun docs read PAGE`
read the same offline guides with `--cursor <CURSOR>` and `--limit <N>`.
Search also accepts `--path`. JSON output carries the same page fields; human
output prints the complete next-page command. Pass `nextCursor` as `cursor`
with the same query and path until null. Cursors bind the tool, filters and
bundle digest; `invalid_cursor` means restart without a cursor. Pages stop at
256 KiB of serialized rows or line strings, allowing one oversized first item
alone within transport bounds. Search omits `total` and stops without counting
remaining matches; its conservative cursor can lead to an empty final page.

`docs_list`, `docs_search` and `docs_read` expose the
guides shipped in the running binary. Every Markdown page under Testrun's
`docs/` directory is bundled, including nested pages. Calls work offline without
a selected target, device discovery, browser preparation, the device manager or
project file access, and they answer while another Testrun request is still
running. Results report the binary's `version`; changing a local guide does
not change an installed binary's documentation.

| Tool | Arguments and result |
| --- | --- |
| `docs_list` | Optional opaque `cursor` and `limit` (1–100, default 50). Returns `documents` with `path`, `title` and `total_lines`, plus `total` and nullable `nextCursor`. |
| `docs_search` | Required `query` (up to 1024 bytes), optional exact catalog `path`, and the same pagination as listing. Returns `matches` with `path`, one-based `line` and `text`, plus nullable `nextCursor`. All whitespace-separated terms must match on one line, ignoring case. Surrounding whitespace makes the whole query literal. Whitespace-only queries and queries with line breaks are refused. |
| `docs_read` | Required exact catalog `path`, optional zero-based `offset` or opaque `cursor`, and `limit` (1–400, default 100). Returns `path`, `title`, `start_line`, `end_line`, `content`, `total_lines` and nullable `nextCursor`. |

Paths are relative to the bundled docs root, such as `mcp.md`. Unknown paths
return `NOT_FOUND` with a route to `docs_list`; invalid arguments return
`INVALID_ARGUMENT`. Malformed, foreign, changed-filter and stale-bundle cursors return
`invalid_cursor` with instructions to restart without a cursor. Catalog paths
sort lexically; search results follow that order, then line order. Follow
`nextCursor` until null, including when the page ends early on its byte budget.
Joining the `content` of successive reads reproduces a page byte for byte,
including its line endings.

Example tool arguments:

```json
{"query":"--json","path":"mcp.md","limit":5}
```

```json
{"path":"mcp.md","limit":40}
```

Documentation uses the normal MCP result envelope described below. Read content
preserves Markdown, examples, indentation and original line endings, including
the final newline when present; concatenate successive `content` chunks directly.
Search `text` omits the line ending. Documentation calls use the responsive read
lane and normal cancellation: a cancelled request receives no reply. The CLI's
`docs list`, `docs search` and `docs read` commands use the same query implementation.

## Agent setup

Run `testrun mcp setup` in a project to choose agents and review their MCP
registration. Project scope is the default.

On Unix, guided install or repair preselects available agents whose CLIs are
executable on `PATH`, without starting them. Press Space to change any mark.
Agents remain selectable when detection misses them; aliases and tools outside
`PATH` are not detected. Editor integrations require manual selection.
Removal starts with nothing marked, and explicit `--agent` selections bypass
detection. Detection does not verify agent versions, sign-in or MCP connectivity.

To select agents directly:

```sh
testrun mcp setup --agent codex --agent claude
testrun mcp setup --agent kimi
testrun mcp setup --scope user --agent qwen
testrun mcp setup --remove --agent codex
testrun mcp setup --agent codex --dry-run --json
```

New or changed registrations and removals require terminal confirmation or
explicit `--yes` with named agents.
`--dry-run` inspects without writes. JSON and pipes never prompt. An owned
registration can be checked or restored to its recorded value on repeat. In the agent list, type
to filter, press Space to mark agents and Enter to continue to the review;
Enter with nothing marked asks you to press Space. Cancelling with Esc leaves
registrations intact. Existing unselected agents and
other servers are preserved. Manually edited or unowned `testrun` entries cause
a conflict; review those entries yourself before retrying. Re-run setup to
repair a missing owned entry. A new user-scope executable path requires review.

Project files such as `.mcp.json` are often committed, so project scope
registers the bare command `testrun` with no working directory: every teammate's
agent runs their own installed `testrun` from PATH. Make sure the agent can find
it there, such as `~/.local/bin` for an agent started outside a shell. User
scope registers the absolute installed path.

### Through Hub's or Box's configuration

In a project that uses Hub's or Box's agent configuration, choose `hub` or
`box`. They are the first two agents in the list, and the configured one is
preselected; Hub is preselected where both are configured:

```sh
testrun mcp setup --agent hub
hub setup
```

```sh
testrun mcp setup --agent box
box setup
```

Setup then writes `.hub/mcp/testrun.yml` or `.box/mcp/testrun.yml`
(`~/.hub/mcp/testrun.yml` or `~/.box/mcp/testrun.yml` with `--scope user`), a
server definition with the tools you approve set to `allow`, instead of an
agent's own file. `hub setup` or `box setup` publishes it to every agent each
developer chose; after a later change, rerun `hub setup`, or run
`box config sync` for Box. Setup refuses `hub` in a folder without
`.hub/config.yml` (run `hub setup` first) and `box` in a folder without
`.box/config.yml` (run `box config init` first). It also refuses a definition
it did not write, unless the file is exactly what it would write, and one
edited since setup wrote it. `testrun mcp setup --remove --agent hub` or
`--agent box` deletes the file.

The setup review and `--dry-run --json` show each agent's identifier,
configuration location and any scope it cannot configure. Setup never substitutes user
scope for unavailable project scope. Before confirmation, setup prints the
resolved project and paths, plus the proposed launch command and arguments for
entries it will write or recover. Goose user configuration is normalized from
YAML to equivalent JSON, preserving values but discarding comments and formatting;
the review calls this out before writes.

Use `--tool-permissions` to open the permissions page for Hub, Box, Claude Code
or Codex. Ordinary setup uses saved choices or the product defaults.
Marked tools run without asking; unmarked tools ask each time. Every tool is
marked by default, and a later setup starts from the approvals already
configured. The page labels saved selections. Use
`testrun mcp setup --agent codex --reset-tool-permissions` to restore current
defaults; add `--dry-run --json` to preview without writes. Reset conflicts with
`--remove` and retains normal setup authorization.
Press Space to unmark a tool and Enter to continue. Codex gets an
`approval_mode` for every tool in its `testrun` entry. Claude Code gets one
`mcp__testrun__<tool>` rule per marked tool in `.claude/settings.local.json`
(`~/.claude/settings.json` at user scope). Setup removes only the rules it added.
Box's definition lists each marked tool as `allow`. Other agents keep their own approval settings. Without a terminal, setup uses
the configured or default approvals.

MCP reads such as `observe`, `wait`, and UI inspection use an existing
connection. The separately approved `connect` tool opens a disconnected
device and returns a screenshot. `app_list` can connect a named device under
its existing approval.

The three read-only documentation tools share these defaults. When an existing
install has saved approvals, setup preserves that selection; newly added tools
start unmarked rather than extending it automatically. Re-run `testrun mcp setup --tool-permissions`
and mark the documentation tools to approve them without asking, then reload the
host. Existing broader host permission rules still apply. No approval migration
runs when the binary starts.

Setup validates Testrun's installed tool catalog without acquiring a target or
starting the device manager. It does not choose devices or change signing preferences.
Trust/reload your agent as needed and check its MCP list after setup;
configuration verification does not establish a live connection.

Setup errors use `AGENT_SETUP_CONFLICT` for entry ownership or concurrent-edit
conflicts, `AGENT_SETUP_CONFIRMATION_REQUIRED` when selection or confirmation
needs a terminal, and `AGENT_SETUP_CATALOG_CHECK` when the executable's catalog
probe fails. An unknown agent, or one the selected scope cannot configure
automatically, uses `AGENT_SETUP_INVALID_SELECTION`. Invalid configuration uses
`AGENT_SETUP`; filesystem failures use
`AGENT_SETUP_IO`. An interrupted publication returns `AGENT_SETUP_PARTIAL` with
`details.completed`, `details.pending` and the underlying error in `details.cause`.
Pending agents may already have partial writes; retry setup and review its plan.
Successful setup includes `host_action` with the remaining trust/reload step.

## Server transport

Run `testrun mcp serve` as a newline-delimited stdio MCP server. Protocol stdout
contains only replies and notifications. Inspect the registered catalog with
`testrun mcp serve --check`; it also validates the bundled skills and does not
connect to a device.

The server accepts two kinds of request on one connection. A client that
initializes uses revision `2025-06-18` or `2025-11-25`: initialization and the
initialized notification precede tool calls. A client using revision
`2026-07-28` sends no handshake; each request carries its own
`io.modelcontextprotocol/protocolVersion` and
`io.modelcontextprotocol/clientCapabilities` in `params._meta`, and
`server/discover` returns the supported revisions, capabilities and
instructions. Replies to these requests add `resultType: "complete"` and the
server identity in `_meta`; tool results keep the same `content`,
`structuredContent`, `isError` and images, without cache hints. A modern
request's client identity is self-reported and grants nothing.

Initialization requires `protocolVersion`, an object `capabilities` and an
object `clientInfo` with string `name` and `version`. Supported requested
versions are `2025-06-18` and `2025-11-25`; unsupported versions negotiate the
latest supported version. Frames are limited to 1 MiB excluding the trailing LF.
Duplicate JSON keys and trailing values are rejected. Oversized frames receive
a parse error and the connection ends.

Requests must have distinct IDs while outstanding. Reusing an outstanding ID
returns `-32600` and ends the connection, including cleanup of this client's
pending work. An ID becomes reusable after completion or cancellation.

Send `notifications/cancelled` with `params.requestId` to cancel an outstanding
request. Cancelled requests receive no reply. Cancellation does not prove that
dispatched device input had no effect: inspect `status` and the action's recorded
outcome before deciding what to do next. Testrun never replays uncertain input.
Cancellation affects only that request; other clients' queued work continues.

`testrun/task_end` and `notifications/testrun/task_end` are host callbacks, not
published tools. They take no parameters or an empty object, and need an
initialized connection: a `2026-07-28` request before initialization returns
`-32602`. A valid callback
runs before queued input; work already running settles normally. The server then
refuses requests it received before the task end with `TASK_ENDED`
(`details.nextAction: "status"`); status and other inspection still answer.
Task end releases no device: the request's acknowledgement reports
`released: []`, and devices stay connected for other clients until a
`disconnect` or idle release. The notification has no reply. Malformed callbacks
do not end a task. Ping is responsive but is not a task stop completion barrier.

Testrun uses canonic-mcp-kit's I/O-free core for protocol admission, its validated
catalog, argument validation and content encoding. Testrun retains its Tokio
transport, responsive and per-target lanes, request cancellation and signals.
On EOF, the ordered call in flight gets up to 250 ms to reply and buffered
ordered calls are refused once the dispatcher sees EOF. Independent long reads
are dropped, and responsive reads get a separate drain of up to 250 ms. See
[device manager](#device-manager) for what happens to it. Fatal protocol errors
close without waiting for the peer to close stdin.

Tool results retain `content`, `structuredContent` and `isError`. Testrun's
structured envelope reports `{ok: true, data, elapsedMs}` or its stable error
code, message, retryability and details. Images use shared content encoding;
Testrun reads at most 2 MiB per image and retains the evidence metadata if an
attachment cannot be read. Internal tools remain unpublished.

## Bundled skill

Testrun publishes one workflow, `ui-verification`, at
`skill://testrun/ui-verification/SKILL.md`. It guides an agent through
verifying a reported UI issue when each step depends on what the screen shows,
and through recovery after an interrupted action. Routine tool use does not
need it.

On revision `2026-07-28`, `skills/list` and `skills/get` return the skill's
frontmatter and file manifest with SHA-256 digests, and `resources/read`
returns the file. Skill results may be cached publicly for five minutes;
tool results are never marked cacheable. Initialized clients do not see the
Skills extension and `skills/*` returns `-32601`, but `resources/list` lists
the skill and `resources/read` returns the same bytes.

The files are embedded in the binary. Reads work offline, without the device
manager or a target, and run beside device work rather than queuing behind it.
Cancelling a read sends no reply. An unknown resource returns `-32002` to an
initialized client and `-32602` to a `2026-07-28` request. Reading the skill
does not activate it or grant any permission; the host decides whether to use it.

## Device manager

Each `testrun mcp serve` process is a client of the user's background Testrun
manager, which holds every connected device and browser. The server opens a
connection to the manager with its first device request and keeps it open, so
the manager keeps running while the server does. Every MCP server and CLI
command sees the same connected devices; `connect` to a device another client
connected returns it ready. Requests to one device from every client share that
device's queue in arrival order: `act` (one batch is one entry), app operations,
`network_set`, probes, recordings, preparation and `disconnect` run one at a
time, while reads such as `observe`, UI reads and `status` run beside the
queue. Calls for different devices run in parallel.

When the server exits, stdin closes or a request is cancelled, requests that
have not started on their device are abandoned; work already running on a
device settles in the manager, and its outcome remains available through
`status`. Nothing is released: devices stay connected for other clients until
a `disconnect` or ten minutes without use.

Restart MCP servers after installing a new Testrun build. While a busy manager
from a build with another manager protocol drains, requests return retryable
`OWNER_UPGRADE_PENDING`; that manager releases its devices and retires, and the
next request starts the current one. See [distribution](distribution.md) for
replacing a manager of the same protocol. A draining manager closes the server's
connection; the next device request connects to its successor.
