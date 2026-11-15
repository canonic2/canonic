# Agent tools

Workbench lets a coding agent render your pages, inspect their structure and
styles, check problems, and exercise interactive states. Captures use private
browser pages. `view set` is the command that changes the user's canvas.

## Connect an agent

Run setup from a project containing `workbench.yaml` or
`.config/workbench.yaml`:

```sh
workbench mcp setup
```

Choose the agents to configure and review the paths, launch command and tool
approvals before applying. Project scope is the default. A project configured
for Hub or Box offers a server definition for that host (`hub` or `box`); the
host publishes it to the agents each developer selects. When a project has
both, setup preselects `hub`. Direct host choices include `claude`, `codex`,
`copilot-vscode`, `copilot-cli`, `cursor`, `gemini`, `kiro`, `kilo`, `kimi`,
`opencode`, `qwen`, `amp` and `droid`. Other surfaces have user-only or manual
setup routes, shown by the picker when automatic configuration is unavailable.

For an explicit, noninteractive workflow, inspect first:

```sh
workbench mcp setup --agent claude --agent codex --dry-run --json
workbench mcp setup --agent claude --agent codex --yes --json
```

For Codex and Claude Code, setup also installs the `UserPromptSubmit` hook
`workbench hook user-prompt`. It supplies the current canvas reference on each
prompt while a canvas is open. Codex uses `.codex/hooks.json` beside its MCP
configuration; Claude Code uses `.claude/settings.local.json` in project scope
and `~/.claude/settings.json` in user scope. A Hub or Box definition includes
this hook for the host to publish to Codex and Claude Code. Other agents use
`view_show`.

Setup edits only Workbench's server entry, its owned approval rules and prompt
hook, keeping other servers, settings and hooks. Rerun it to repair an owned
registration without adding duplicate hooks. `--remove` removes the selected
owned registration and hooks; equivalent hooks already present before setup
remain unowned. It refuses conflicting external edits.
An existing MCP registration created by another configurator is unowned and
blocks setup even if it uses the same command. Use one configurator for the
registration. To hand it over, disable the previous configurator's Workbench
integration and remove its generated registration and prompt hook before running
setup. For a prompt-hook conflict, align the existing group's fields with the
requested definition or remove that group; setup preserves unrelated hooks.
`--scope user` explicitly chooses personal configuration, and `--project`
selects another project. The launch command is `workbench mcp serve` in both
scopes, so agents must find `workbench` on `PATH`.

All Workbench tools are selected for approval by default where the host supports
per-tool settings, including canvas-changing and interactive tools.
Existing saved choices remain in effect on repair and are labeled in the picker.
Run `workbench mcp setup --agent codex --reset-tool-permissions` to restore current
defaults; add `--dry-run --json` to preview without writes. Reset conflicts with
`--remove` and retains normal setup authorization. Host trust/reload may be
needed after setup; a successful file write does not prove a live connection.
No runtime pages or project manager start during setup.

Configuration written successfully does not establish a live agent connection.
Reload or reconnect the agent and approve its prompt hook if requested. Inspect
the agent's MCP server list to confirm Workbench connected. With a canvas open,
run `workbench view show --json` from that checkout to check the current reference,
and `workbench hook user-prompt` to check prompt-hook output. Then ask the agent
which Workbench page is open; it should name the selected page and scenario.
A hook with no open canvas prints nothing. A failed host connection, a missing
canvas and a successful registration are separate states.

Configure your MCP client to run `workbench mcp serve` from a folder inside
the project. The server finds `workbench.yaml` or `.config/workbench.yaml` in
that folder or an ancestor. To choose a project explicitly, pass
`--project /path/to/project`; to use another manifest, pass
`--config /path/to/workbench.yaml` with an absolute path. A `--config` file
that is missing fails each tool with `config_not_found` rather than falling
back. The CLI agent commands (`workbench page`, `workbench config show` and
the others) take the same two options.
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

## Read the Workbench workflow

The server publishes the `canonic-workbench` skill, covering authoring,
configuration and review workflows. A host with native Skills-over-MCP support
can discover it through the standard Skills methods. Other resource-capable
clients can list resources and read:

```text
skill://workbench/canonic-workbench/SKILL.md
```

Ask your agent to read that resource from the Workbench server. It is bundled
with the executable and available without a project or running manager.
Reading it does not install instructions, activate a skill or grant tool
permissions. Your host owns skill loading and approval. Existing local skill
copies are not changed by the server.

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

## View a capture in chat

When your chat host supports MCP Apps, `page_screenshot` can display a page
preview beside its result. Use **Zoom** to fit the captured image or view it
at 100% or 200% of its image pixels. **Full resolution** loads the retained PNG
when the host supports resource reads. If it has expired or exceeds the
resource limit, ask the agent to capture again or use the returned file.

**Refresh** takes another screenshot with the original arguments, except that
it does not save the capture into handoffs. It requires the host to support
tool calls from the preview. Captures use the same private browser and leave
your canvas alone.
An open page handle keeps its existing lifetime; if it expires, ask the agent
to start or capture the page again.

The displayed image updates on capture, not automatically on source edits.
Chat hosts control whether the UI appears and which operations it can use.
Hosts without Apps keep the ordinary image, text and structured result.

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

## Inspect and edit images

Images handed over for a page are rarely ready to use. Workbench reads and
writes them itself, with no image tools to install:

```sh
workbench image inspect assets/banner.png --json
workbench image list pages/home.html --sizes mobile,laptop --json
workbench image edit assets/banner.png \
  --steps '[{"trim":{}},{"fit":{"ratio":"16:9"}},{"round":{"radius":12}}]' \
  --outputs '[{"format":"webp","width":320,"densities":[1,2,3]}]' \
  --out public/images
```

`image_inspect` reports an image's format, size and aspect ratio, bits per
channel, colour profile, frame count and file size; its transparency; the box
its content fills and the padding around it; its edge, corner and dominant
colours; and a preview over a checkerboard. It reads PNG, JPEG, WebP, GIF,
TIFF, BMP and ICO files up to 64 MiB, 16,384 pixels an edge and 32
megapixels. AVIF can be written but not read.

`image_list` renders a page privately at each requested size and lists every
`<img>`, `<picture>` and CSS background image: its element, the file it
loads, the size it is shown at and its own pixels. Findings name what to fix:
`undersized` (blurry at one of `densities`, 1 and 2 by default), `oversized`
(more than twice the pixels the largest density needs), `no_density_variants`
(no `srcset` or `image-set()`), `aspect_mismatch` (stretched or cropped by
`object-fit`), `heavy` (over 50 KiB and more than 1 byte a pixel for JPEG,
WebP and AVIF, or 3 for other formats) and `not_found`. An image inside a
shadow root has no `selector`; `shadowHost` names the element that hosts it,
and `image_edit` cannot show an edit in its place.

`image_edit` runs a list of steps, then writes each output:

| Step | Arguments |
| --- | --- |
| `crop` | `x`, `y`, `width`, `height` in pixels |
| `trim` | `tolerance`, 0–255: crop to the content box |
| `pad` | `all`, `top`, `right`, `bottom`, `left`, `colour` |
| `fit` | `ratio` such as `16:9`, `mode` `pad` or `crop`, `anchor`, `colour` |
| `resize` | `width`, `height` (one keeps the ratio), `filter` |
| `rotate`, `flip` | `turns` 1–3 clockwise; `axis` `horizontal` or `vertical` |
| `round` | `radius` in pixels, for rounded corners |
| `mask` | `file`, `channel` `alpha` or `luminance` |
| `remove_background` | `colour` (the top-left pixel's by default), `tolerance`, `softness` |
| `adjust` | `brightness`, `contrast`, `saturation`, each −1 to 1 |
| `convert_colour` | `to` `srgb` or `display-p3` |

Colours are `#rrggbb`, `#rrggbbaa` or `transparent`. An output names its
`format` (`png`, `jpeg`, `webp`, `avif`, `gif`, `ico`, `tiff` or `bmp`) and
optionally `quality`, `lossless` (WebP), `colors` (a smaller palette PNG),
`effort` (PNG and AVIF) and `background` (behind transparency in JPEG); an
option the format does not use is refused. Give `width` or `height` in CSS
pixels with `densities` for `@2x` files, or `widths` for a `srcset` with
width descriptors. Without outputs, the source's format is written at its
size. An edit writes at most 12 files, each within 16,384 pixels an edge and
32 megapixels (WebP 16,383 an edge, ICO 256).

The result lists each file with its size and any change the format required,
such as transparency flattened for JPEG, a before/after picture, and the
`srcset` text for the files. Add `--page` and `--selector` (`target` and
`selector` for MCP) to see the edited image in its element on the page, before
and after; the pictures cover the element at both its old and new size. An
MCP call that reaches its `timeout` keeps nothing it finished later. A format
that cannot carry the image's colour profile, such as
lossy WebP for a Display P3 image, is refused until a `convert_colour` step
converts it.

Nothing is saved until you give `--out` (`out`): a file for one output,
otherwise a folder where files are named after the source, or `--name`, with
`@2x` or `-640w`. Existing files are kept unless you add `--replace`. MCP
paths are relative to the project; CLI paths to the working directory.
`--steps` and `--outputs` also take `@file` for a JSON or YAML file, so a
recipe can live beside its images.

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
