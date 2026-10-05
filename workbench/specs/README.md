# Workbench specifications

These internal specifications are maintained separately from the
[public user documentation](../docs/README.md), which is published on the website.
They define what Workbench should do and how it should work: product requirements,
workflows, system contracts, design decisions, and discoveries from investigation.
They guide implementation and review rather than merely describe the current code.
The [workbench README](../workbench/README.md) remains the setup and operations guide.

Keep agreed requirements, proposals, verified behavior, and known implementation
gaps distinct. Record discoveries with evidence and explain their implications;
label hypotheses and open questions. Update the relevant topic when requirements,
decisions, or findings change, and add new topics to the index below. Use the
`product-specs` skill to write and maintain these specifications.

## Multiple-artboard update

The [feature contract](multiple-artboards.md) and
[TypeScript code plan](multiple-artboards-code-plan.md) are the authoritative
support-system record and target architecture for this update. The current scope
is infrastructure only: independent instance state/runtimes, mixed-space access,
full-canvas capture/handoff/context APIs, and native producer ownership. Existing
controls, styles, layout, and the default entry point are unchanged. A temporary
programmatic harness verifies the support without adding production UI. Free positioning and row/grid arrangement are phase 2; persistence
and shared layouts remain open. The feature contract records verification and
limits, and the code plan distinguishes migrated modules from renderer debt.

## Topics

| Spec | Covers |
| --- | --- |
| [Web Components](web-components.md) | Target component structure, public APIs, Shadow DOM, lifecycle, host loading and bounded migration |
| [Themes](themes.md) | Standalone defaults, semantic tokens, VS Code theme forwarding, live updates, accessibility and preview isolation |
| [Terminology](terminology.md) | The agreed names for what a space holds (space, collection, group, page, state, lens) and for each part of the interface |
| [Core workbench](core.md) | Configuration, problem reporting, navigation, selection, the canvas and artboards, and failures shared by every page |
| [Lenses](lenses.md) | Rendering kinds, customizable display labels for every lens including the authored preview, stable identity, defaults, and configuration precedence |
| [Multiple artboards](multiple-artboards.md) | Investigation and proposed comparison canvas, artboard ownership, selection, persistence, implementation phases, and open product decisions |
| [Artboard code plan](multiple-artboards-code-plan.md) | Planned TypeScript capability boundaries, state and runtime APIs, space transport, capture/context workflows, migration slices, and validation |
| [TypeScript previews](previews.md) | Preview definitions, discovery, the compiler worker, adapters, controls, the `workbench` lens, and the portable build |
| [Artboard sizes](sizes.md) | Proposed sizes defined by each space and limited or extended by pages: fill axes, configurable buttons and icons, the size menu with **Custom size…** and **Edit sizes…**, addresses, handoffs, and export |
| [Docs](docs-pages.md) | Docs as a lens of any page: Markdown with live examples on the whole canvas, `docs` lenses one per renderer, Markdown pages, `defineDocs`, the docs canvas mode, and docs references in the export |
| [Storybook](storybook.md) | Explicit and imported stories, catalog lookup, story selection, preview reuse, and startup |
| [Authored pages and implementations](implementations.md) | Authored pages, URL implementations, iOS Simulator implementations, window implementations, and the native window stream |
| [VS Code extension](vscode-extension.md) | Activation, trust, commands, webviews, refresh, start commands, and the server and helper processes |
| [Agent context](agent-context.md) | Telling chats in the editor which page the canvas shows: the view route, the server announcement, Shield's hooks and MCP server, and proposed Copilot surfaces |
| [Implementation proxy](implementation-proxy.md) | Why URL and Storybook lenses load through a loopback proxy, what it rewrites, and why lenses must not frame implementations directly |
| [Interactive capture](capture.md) | Bundled Electron helper, live mirroring, camera, handoff, and limits |
| [Design-system export](export.md) | Page, collection and space scopes, ZIP/PDF/image/browser output, background captures, hashes, and warnings |
| [Module layout](modules.md) | Capability ownership under `src/modules/`, shared UI and infrastructure locations, and bounded migration requirements |

The workbench is one implementation in `packages/workbench/workbench/`. Projects provide
`workbench.yaml` and may override machine-specific values in the ignored
`workbench.local.yaml`. They do not receive a copy of the workbench code.

## Page sources

A page enters the page list from `workbench.yaml`, from an implementation catalog,
or from preview discovery. TypeScript previews (`*.workbench.ts` and
`*.workbench.tsx`) are discovered unless `previews: false`, compile in a managed
worker, and render in the Workbench preview host; see
[TypeScript previews](previews.md) and the [preview API guide](../docs/workbench-previews.md).

| Source | How it enters the page list | What appears on the canvas |
| --- | --- | --- |
| Authored page | A `collections[].items[]` entry with `src` | The project HTML file as designed |
| TypeScript preview | A discovered definition, placed in the collection its title names, or listed by `src` | The compiled component in the preview host, with its states and controls |
| Markdown page | A `collections[].items[]` entry whose `src` is a `.md` file, or a discovered `defineDocs` definition | The Markdown on the whole canvas, with examples rendered through its docs lenses |
| Docs | `docs` on any `collections[].items[]` entry, with `kind: docs` lenses | The page's Markdown as one of its lenses, beside its design |
| Workbench mapping | A `kind: workbench` lens on an authored page | A named TypeScript preview's states beside the design |
| URL implementation | A lens on an authored page | The configured external page in an iframe |
| Storybook mapping | A lens on an authored page | One story from the mapped Storybook title |
| Storybook catalog | `catalog: true` on a Storybook implementation | Imported titles and stories, without a design lens |
| iOS Simulator mapping | A lens on an authored page | A stream from the selected booted device |
| iOS Simulator catalog | `catalog: true` on a Simulator implementation | Imported booted devices, without a design lens |
| Window mapping | A lens on an authored page | A stream of the named window from the configured macOS app |

The specs describe VS Code behavior unless they explicitly mention standalone
browser or server use. A `file://` workbench has no server-backed catalog,
story lookup, or TypeScript previews.
