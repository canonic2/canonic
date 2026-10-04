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

| Spec | Covers |
| --- | --- |
| [Core workbench](core.md) | Configuration, problem reporting, navigation, selection, frames, and failures shared by every screen |
| [TypeScript previews](previews.md) | Preview definitions, discovery, the compiler worker, adapters, controls, the `workbench` lens, and the portable build |
| [Storybook](storybook.md) | Explicit and imported stories, catalog lookup, story selection, preview reuse, and startup |
| [Authored pages and implementations](implementations.md) | Authored pages, URL implementations, iOS Simulator implementations, window implementations, and the native window stream |
| [VS Code extension](vscode-extension.md) | Activation, trust, commands, webviews, refresh, start commands, and the server and helper processes |
| [Agent context](agent-context.md) | Telling chats in the editor which screen the canvas shows: the view route, the server announcement, Shield's hooks and MCP server, and proposed Copilot surfaces |
| [Implementation proxy](implementation-proxy.md) | Why URL and Storybook lenses load through a loopback proxy, what it rewrites, and why lenses must not frame implementations directly |
| [Interactive capture](capture.md) | Bundled Electron helper, live mirroring, camera, handoff, and limits |
| [Design-system export](export.md) | Background reference captures, worker scheduling, ZIP contents, hashes, and warnings |

The workbench is one implementation in `packages/workbench/workbench/`. Projects provide
`workbench.yaml` and may override machine-specific values in the ignored
`workbench.local.yaml`. They do not receive a copy of the workbench code.

## Screen sources

A screen enters the list from `workbench.yaml`, from an implementation catalog,
or from preview discovery. TypeScript previews (`*.workbench.ts` and
`*.workbench.tsx`) are discovered unless `previews: false`, compile in a managed
worker, and render in the Workbench preview host; see
[TypeScript previews](previews.md) and the [preview API guide](../docs/workbench-previews.md).

| Source | How it enters the list | What appears on the canvas |
| --- | --- | --- |
| Authored page | A `sections[].items[]` entry with `src` | The project HTML file as designed |
| TypeScript preview | A discovered definition, placed in the section its title names, or listed by `src` | The compiled component in the preview host, with its states and controls |
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
