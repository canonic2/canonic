# Agent tools

Workbench lets a coding agent render your pages, inspect their structure and
styles, check problems, and exercise interactive states. Captures use private
browser pages. `view set` is the command that changes the user's canvas.

## Connect an agent

Run setup from a project containing `workbench.yaml`:

```sh
workbench mcp setup
```

Choose the agents to configure and review the paths, launch command and tool
approvals before applying. Project scope is the default. A Box-configured project
offers a Box server definition; Box publishes it to the agents each developer
selects. Direct host choices include `claude`, `codex`, `copilot-vscode`,
`copilot-cli`, `cursor`, `gemini`, `kiro`, `kilo`, `kimi`, `opencode`, `qwen`,
`amp` and `droid`. Other surfaces have user-only or manual setup routes, shown
by the picker when automatic configuration is unavailable.

For an explicit, noninteractive workflow, inspect first:

```sh
workbench mcp setup --agent claude --agent codex --dry-run --json
workbench mcp setup --agent claude --agent codex --yes --json
```

Setup edits only Workbench's server entry and owned approval rules, keeping
other servers and settings. Rerun it to repair an owned entry. `--remove`
removes the selected owned registration; it refuses entries edited outside
setup. `--scope user` explicitly chooses personal configuration, and `--project`
selects another project. The launch command is `workbench mcp serve` in both
scopes, so agents must find `workbench` on `PATH`.

Read-only tools are selected for approval by default where the host supports
per-tool settings. Canvas-changing and interactive tools are left to ask.
Existing saved choices remain in effect on repair and are labeled in the picker.
Run `workbench mcp setup --agent codex --reset-tool-permissions` to restore current
defaults; add `--dry-run --json` to preview without writes. Reset conflicts with
`--remove` and retains normal setup authorization. Host trust/reload may be
needed after setup; a successful file write does not prove a live connection.
No runtime pages or project manager start during setup.

Configure your MCP client to run `workbench mcp serve` from a folder inside
the project. The server finds `workbench.yaml` in that folder or an ancestor.
To choose a project explicitly, pass `--project /path/to/project`.
The `workbench` command must be on `PATH`. Page rendering requires the
Workbench browser runtime on the host; `capture_unavailable` explains a
missing runtime or unsupported host.

Run `workbench mcp serve --check` to validate and print the installed tool
catalog as JSON without opening the MCP transport or starting a project manager.

Every MCP tool has a CLI command that runs the same operation. Add `--json`
for its structured result. Commands below run from your project root; replace
the example page paths with pages listed by `config show`.

```sh
workbench view show --json
workbench config show --json
workbench page screenshot pages/pricing.html --size mobile --json
```

`view_show` identifies the canvas the user used last and each artboard's
target. `config_show` lists pages, scenarios, sizes and implementation lenses.
A target names a `page`, optionally a `space`, `scenario`, `artboard`,
`iteration`, `lens`, and `size`. Use `width` and `height` instead of `size`
for explicit CSS dimensions. The CLI takes these as flags. If several spaces
contain the page, supply `--space` with its ID from `config show`.

## Check a page

```sh
workbench page check pages/pricing.html --sizes mobile,laptop --json
workbench page inspect pages/pricing.html --selector h1 --properties font-size,color --json
workbench page snapshot pages/pricing.html --size mobile --json
workbench page console pages/pricing.html --json
```

`page_check` runs build, console and layout checks. Use
`--checks build,console` to select checks. Problems carry a severity, message
and target; compiler diagnostics include their source file and range when
available. `ok` means there are no errors; warnings can still need review.
Layout checks identify document overflow, clipped elements, truncated text,
overlapping controls, failed images and controls smaller than 24 × 24 CSS
pixels. Intended clipping and layered controls can produce warnings.

`page_inspect` returns up to 20 matching elements, their selectors, accessible
roles and names, border boxes in document CSS pixels, and computed styles.
Use `--role button --name 'Send message'` for an accessible name instead of
a selector. `matched` and `more` disclose additional matches.
`page_snapshot` returns the accessibility tree and readable text.
`page_console` reads errors and warnings by default; `--levels` selects other
levels and `--after` reads entries following an earlier `last` sequence.

## Capture sizes and compare views

```sh
workbench page sheet pages/pricing.html --sizes mobile,laptop --json
workbench page compare --a '{"page":"pages/pricing.html","size":"mobile"}' --b '{"page":"pages/pricing.html","scenario":"dark","size":"mobile"}' --json
```

`page_sheet` returns one labelled PNG with the requested sizes. Without
`--sizes`, it uses every supported size, up to 12. Its base target cannot
include a size or explicit dimensions. Fit and Resizable use 1440 × 900
when there is no canvas to measure; filled axes use the corresponding
dimension. Each entry in `sizes` includes its target and console problems.

`page_compare` takes two JSON targets or page handles and returns A, B and
their difference side by side. Both must have the same CSS dimensions.
`changed` is the fraction of visibly different pixels, from 0 to 1, at one
device pixel per CSS pixel. Comparison uses a white background and exact
pixel equality. `bounds` contains up to 20 region boxes in those pixels;
nearby changes can share a box. Each side includes its console problems.

Both tools return `file`, the full-resolution PNG, and a capture `resource`.
MCP also receives a smaller inline image fitting 1,568 pixels. Composition
refuses pictures beyond 32 megapixels, 16,384 pixels per edge, 12 MiB of input
PNGs, or a 16 MiB output PNG; request fewer or smaller sizes when refused.

For one page, `page_screenshot` supports `--full-page`, `--clip '#selector'`,
and `--device-scale-factor 2`. `--save` also writes that screenshot into
`.canonic/.handoffs/`. Other captures stay outside the project: MCP captures
last for their connection, while CLI captures keep the project's newest 50.

## Exercise interactive states

```sh
workbench page start pages/contact.html --size mobile --json
workbench page list --json
workbench page interact --handle pg_0123456789abcdef --steps '[{"action":"click","target":{"role":"button","name":"Send message"}}]' --snapshot --json
workbench page stop --handle pg_0123456789abcdef --json
```

Replace the example handle with `page_start`'s `page`. Handles retain the
page's state between calls; `page_interact` returns each step's outcome,
new console entries and a screenshot, optionally a snapshot. Steps stop at
the first failure. A handle closes after 10 minutes without a call; CLI
handles also close when the manager stops, including its idle shutdown.
MCP handles belong to their connection, while CLI handles belong to the project.
Pass handles to `page_compare` to compare states already reached through input.

## Change the canvas

To read the user's drawings and notes, run:

```sh
workbench annotation list --json
```

`annotation_list` returns the latest published snapshot from the canvas used
last, grouped by artboard with each target. Entries include kind, text, anchor,
box in local CSS pixels and a readable element description when available.
Arrows point at their visible tip; comments use their pin. Text notes and
unreadable external pages have no element description. Reading leaves drawings
and selection intact and does not capture images or resume a preview.

`available: false` means a renderer snapshot is unavailable, rather than empty.
`total` and `omitted` disclose the publication limits of 200 complete entries
and 64 KiB across the canvas. Notes that do not fit are omitted whole. Snapshots
are session-only, follow renderer updates and disappear with canvas withdrawal
or expiry. With no open canvas the result is `open: false`.

```sh
workbench view set pages/contact.html --size mobile --json
```

`view_set` replaces the selected artboard of the canvas used last, keeps
the other artboards, and returns the canvas record after acknowledgement.
It does not reveal or focus the editor tab. A closed or disconnected canvas
fails with `canvas_closed`; open or reload it and retry. If selection changes
or the call times out, read `view_show` before retrying. The command uses the
canvas's media color scheme; choose a page scenario for dark or light content.

Calls default to a 60-second deadline; `--timeout` accepts milliseconds up
to 120,000. Refusals name the next step. The offline `docs list`,
`docs search <query>` and `docs read <path>` commands also work outside a project.
