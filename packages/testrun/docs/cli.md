# CLI output

Run Testrun commands directly to read tables, status fields and diagnostics.
Output remains human-readable when redirected to a file. For tools and scripts,
add the global `--json` flag before or after the subcommand:

```sh
testrun target list --compact
testrun --json target list --compact
testrun manager status --json
```

Human results go to stdout. Operation activity goes to stderr when it is a
terminal, names the work and reports its elapsed time and observed outcome.
Narrow terminals stack table records and wrap fields. Artifact paths and resource
identifiers stay available in the result; inline encoded attachments are
summarized, and `--json` includes their complete data.

Human errors go to stderr with their code, reason and supplied diagnostic,
log and recovery details. They do not automatically retry an operation. A partial
or unknown action outcome remains explicit; inspect its action or operation ID
before sending input again.

## Pass JSON arguments

Examples such as `testrun browser send '{"operation":"page_list"}'` pass the
entire JSON object as one argument. On Windows, use PowerShell 7.3 or later
with its default native argument passing or `Standard` mode to preserve the
quotes inside that argument. [PowerShell argument parsing](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing#passing-arguments-that-contain-quote-characters)

## Commands

Most commands group related work as `testrun <noun> <verb>`. Shared target
commands such as `connect`, `observe` and `act` sit at the top level. Many
commands have an MCP equivalent, such as `browser connect` and `browser_connect`.
Use the [MCP catalog](mcp.md#tool-catalog) for tool names and schemas.

| Commands | What they do |
| --- | --- |
| `docs list\|search\|read` | Read the installed usage guides offline, without connecting a target |
| `init` | Discover application projects and create reviewed configuration |
| `connect`, `disconnect`, `status`, `observe`, `act`, `cancel`, `wait`, `doctor` | Use and inspect the connected target |
| `target list` | List simulators, iPhones, Android devices and emulators |
| `developer-mode reveal`, `developer-support install` | Prepare a physical iPhone |
| `ui snapshot\|find\|read` | Read the accessibility tree |
| `app list\|install\|uninstall` | Manage apps on the device |
| `control status\|acquire\|release` | Hand input to a local viewer and back |
| `clipboard read` | Read the iOS simulator clipboard |
| `preview start\|status\|stop`, `recording start\|status\|stop` | Live preview and screen recording |
| `media status\|send`, `volume get\|set`, `audio probe` | Playback and audio |
| `network set` | Turn Android networking off or on |
| `browser list\|doctor\|connect\|fixture-register\|status\|send\|cancel\|disconnect` | Individual browser operations |
| `config init\|check\|show\|schema` | Create, validate, or inspect [project configuration](configuration.md) |
| `project discover\|list\|show\|status\|start\|open` | Discover and inspect projects, or run a supported [project workflow](projects.md) |
| `build start\|status\|log\|cancel` | Run and inspect [test-app builds](builds.md) |
| `diagnostics` | [Plan, collect, query, and export diagnostic evidence](debugging.md) |
| `mcp serve`, `mcp setup` | Run the MCP server, or register it with agents |
| `signing setup` | Choose the default XCTest signing team |
| `task stop` | End the host task and keep connected devices available to other clients |
| `manager status\|blockers\|retry\|operation show\|stop\|drain` | Inspect and recover the background manager |

`testrun <command> --help` describes each command's flags.

Project onboarding uses the top-level `init` workflow: `init --dry-run --json`
previews discovery like MCP's `project_discover`, and `init` selects projects
and creates their configuration like `project_init`. The CLI collects review
through terminal prompts or explicit `--yes`; MCP creation uses the returned
`planDigest`. See [Project configuration](configuration.md).

## Bundled documentation

From any directory, list pages, search for terms or read a page:

```sh
testrun docs list
testrun docs search "connect" --path cli.md --limit 5
testrun docs read cli.md --limit 40
```

These commands use guides embedded in the installed executable, without project
configuration, device discovery or the background manager. All accept `--cursor`
and `--limit`. List/search default to 50 results (maximum 100); reads default
to 100 lines (maximum 400). Human output prints the next command when more may
remain. `--json` returns the same result fields as the MCP documentation tools;
failures appear on stderr and exit nonzero.

## Browser targets

`connect`, `act`, `observe`, `status` and `disconnect` support Chrome, Firefox
and WebKit. Use the returned `browser:` target ID and complete page references
for page reads, input and screenshots. Use `testrun cancel --action-id ID` for
a shared action, or `testrun browser cancel --operation-id ID` for an operation
submitted with `browser send`. Browser submissions return before execution
completes. A zero exit status confirms submission. Inspect `operation.settled`,
`operation.outcome` and `operation.result` to check execution. See the
[browser examples](browser.md#use-the-shared-commands).
Use `status --action-id ID --wait-ms MS` or
`browser status --operation-id ID --wait-ms MS` to poll a browser operation.
Both accept 0 to 30,000 milliseconds and return immediately by default.
If the polling window expires, the operation continues and you can poll the same
ID again. A browser snapshot is compact by default. Add `--include-details` to
`browser status` or shared `status` for resources and the full list of operation
summaries. See [browser lifecycle and cleanup](browser.md#mcp-and-cleanup).

## The background manager

Device and browser commands are clients of the user's background Testrun manager, which
holds the connected devices and browsers; a command starts it when needed.
A command without `--target` uses the only connected device.
`observe --target` returns the observation itself. `act --target` returns the
batch result and requires an existing connection.
Cancelling a command with Ctrl-C abandons its request if it
has not started on the device and releases nothing; use `disconnect` to
release a device. `testrun manager` inspects the manager and its devices:

```sh
testrun manager status
testrun manager blockers
testrun manager retry --target device:SELECTED-UDID
testrun manager operation show OPERATION-ID
testrun manager stop     # only when no device or client needs it
testrun manager drain    # refuse new work, release every device, then stop
```

### Automatic connections for reads

In the CLI, these reads connect an explicitly named `--target` when needed.
They use the default connection and leave the device connected for subsequent
requests. Startup has its own connection
budget. The read gets its normal budget once the connection is ready, and
`wait --timeout-ms` measures time spent polling for the condition.

| Read | Targets |
| --- | --- |
| `observe`, `app list` | Android devices, running Android emulators, and running iOS simulators. On a physical iPhone, `observe` uses the fast profile and `app list` uses the default HID connection. |
| `ui snapshot`, `ui find`, `ui read`, `wait` | Android devices, running Android emulators, and running iOS simulators. Snapshots and finds start with the display scope and the first page. Element reads take a fresh query. |
| `clipboard read` | Running iOS simulators |
| `media status`, `volume get` | Android devices and running Android emulators. Automatic volume reads use the music stream. |

On a physical iPhone, evidence screenshots and accessibility need a connection
with a ready [inspection runner](ios.md#runner-on-a-hid-connection). UI cursors,
element references and media-session volume reads use an existing connection.
Use `connect --boot` to start a simulator or AVD. Status and discovery only
report what they can see.

## JSON, errors and exit status

With `--json`, stdout contains the complete result object and stderr contains
`{"error": ...}` on failure. The command exits nonzero on failure. A result
containing `ok: false` stays complete on stdout and exits nonzero without a
duplicate stderr error. JSON mode emits no human notices or interactive prompts.
Argument errors exit 2; cancelled MCP setup or signing selection exits 130.
Other failures exit 1. A closed stdout pipe exits quietly. Help and version retain their ordinary
command-line forms.

## Setup commands

Agent registration uses `testrun mcp setup`, defaulting to project scope. Repeat
`--agent` to select several agents, use `--remove` for targeted removal and
`--dry-run --json` to inspect selected changes without a prompt or writes.
Applying needs terminal confirmation or `--yes`; `--json` alone cannot apply setup.
See [MCP setup](mcp.md#agent-setup) for supported scopes and recovery.

Interactive signing setup requires terminal stdin and stdout. The inline picker
marks the saved team; arrows and number keys highlight, and Enter confirms.
Lists with more than seven teams support filtering. Escape cancels (or clears
an active filter first); Ctrl-C or Ctrl-D cancels directly. First use never
implicitly confirms the first team. The picker clears before the settled result.
Unsupported terminals return repair guidance without switching input methods.

`--json` and `signing setup --non-interactive` never ask for input. When a team choice
is required, select it explicitly:

```sh
testrun signing setup --runner-team-id TEAM_ID --json
```

Shared terminal UI respects `NO_COLOR`, `COLORTERM`, `TERM` and locale capabilities.
Piped human output contains no terminal escape sequences. `testrun mcp serve` remains
newline-delimited MCP, and internal native workers retain their fixed protocols.
