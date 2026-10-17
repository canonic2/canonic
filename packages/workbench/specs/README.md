# Workbench specifications

These internal specifications are maintained separately from the
[public user documentation](../docs/README.md), which is published on the website.
They define what Workbench should do and how it should work: product requirements,
workflows, system contracts, design decisions, and discoveries from investigation.
They guide implementation and review rather than merely describe the current code.
The [workbench README](../ui/legacy/README.md) remains the setup and operations guide.

Specs describe the current desired product and its intended next steps. Keep
implemented behavior, agreed future requirements, proposals and verification
gaps distinct. Delete superseded designs, removed features and completed task
lists; history belongs in release notes and lab reports. Keep rationale when it
explains an active contract, and link supporting evidence without reproducing
the investigation diary. Update the relevant topic when requirements,
decisions, or findings change, and add new topics to the index below. Use the
`product-specs` skill to write and maintain these specifications.

## Reading order and ownership

Start with [Core workbench](core.md) and [Glossary](glossary.md) for the
product model. Feature topics own user-visible behavior. The
[preview pipeline](preview-pipeline/README.md) owns discovery, compilation,
delivery and runtime contracts; its index separates implemented capabilities
from lab work.

For the Rust page-index work, read [Page index](preview-pipeline/page-index.md)
for correctness, lifecycle, cache bounds and acceptance; then
[Performance](preview-pipeline/performance.md#page-listing) for measured results
and target gaps, and [Code quality](code-quality.md) for the engineering gates.
Each requirement has one owning topic. Cross-links connect the topics without
copying acceptance criteria or treating lab results as shipped verification.

## Delivery boundaries

| Area | Current implementation and desired next step |
| --- | --- |
| Preview pipeline | Native listing and the Rspack worker are implemented. Complete-list and first-page targets still require work; compiler-host alternatives remain lab work |
| Multiple artboards | Instance isolation, mixed-space support, review composition and context APIs are implemented. User-facing multi-artboard integration is future work; arrangement is phase 2 and persistence/sharing remain open |
| Sizes and shell | Configurable sizes and migrated Web Components are implemented. Configure pages size cards, guide alignment and the canvas theme bridge remain gaps |

Each owning topic supplies the detailed contract, acceptance criteria and
verification limits. An implemented support API does not establish that its
user-facing workflow is delivered.

## Topics

| Spec | Covers |
| --- | --- |
| [Web Components](web-components.md) | Target component structure, public APIs, Shadow DOM, lifecycle, host loading and bounded migration |
| [Themes](themes.md) | Standalone defaults, semantic tokens, VS Code theme forwarding, live updates, accessibility and preview isolation |
| [Glossary](glossary.md) | The agreed names for what a space holds (space, collection, group, page, state, lens) and for each part of the interface |
| [Core workbench](core.md) | Configuration, problem reporting, navigation, selection, the canvas and artboards, and failures shared by every page |
| [Lenses](lenses.md) | Rendering kinds, customizable display labels for every lens including the authored preview, stable identity, defaults, and configuration precedence |
| [Multiple artboards](multiple-artboards.md) | Implemented instance/review support, desired comparison workflows, remaining UI integration, arrangement phases and persistence decisions |
| [Artboard code plan](multiple-artboards-code-plan.md) | Implemented TypeScript owners, target capability boundaries, state/runtime/space contracts, remaining integration and validation |
| [TypeScript previews](previews.md) | Preview definitions, discovery, the compiler worker, controls, the `workbench` lens, and the portable build |
| [Adapters](adapters/README.md) | What each technology needs to render, one spec per adapter: [HTML](adapters/html.md), [React](adapters/react.md), [React Native Web](adapters/react-native-web.md), [Vue](adapters/vue.md), [Astro](adapters/astro.md), [Storybook](adapters/storybook.md) and [custom adapters](adapters/custom.md); how the Workbench server serves pages |
| [Artboard sizes](sizes.md) | Sizes defined by each space and limited or extended by pages: fill axes, configurable buttons and icons, the size menu with **Custom size…** and **Edit sizes…**, addresses, handoffs, and export |
| [Docs](docs-pages.md) | Docs as a lens of any page: Markdown with live examples on the whole canvas, `docs` lenses one per renderer, Markdown pages, `defineDocs`, the docs canvas mode, and docs references in the export |
| [Storybook](storybook.md) | Pages mapped to stories, story lookup, story selection, preview reuse, and startup |
| [Authored pages and implementations](implementations.md) | Authored pages, URL implementations, iOS Simulator implementations, window implementations, and the native window stream |
| [VS Code extension](vscode-extension.md) | Activation, trust, commands, webviews, refresh, start commands, and the server and helper processes |
| [Agent context](agent-context.md) | Telling chats in the editor which page the canvas shows: the view route, the server announcement, the agent hooks and MCP server, and proposed Copilot surfaces |
| [Implementation proxy](implementation-proxy.md) | Why URL and Storybook lenses load through a loopback proxy, what it rewrites, and why lenses must not frame implementations directly |
| [Interactive capture](capture.md) | Bundled Electron helper, live mirroring, camera, handoff, and limits |
| [Design-system export](export.md) | Page, collection and space scopes, ZIP/PDF/image/browser output, background captures, hashes, and warnings |
| [Preview pipeline](preview-pipeline/README.md) | How native pages are compiled, served, kept warm, made ready and updated: [performance](preview-pipeline/performance.md) requirements and targets, the Rspack [compiler](preview-pipeline/compiler.md), [serving](preview-pipeline/serving.md), [hot updates](preview-pipeline/hot-updates.md), the [warm runtime](preview-pipeline/warm-runtime.md), [readiness](preview-pipeline/readiness.md) and [plugins](preview-pipeline/plugins.md) |
| [Page index](preview-pipeline/page-index.md) | Native discovery and listing, literal/evaluated definition parity, invalidation, bounded saves, process lifecycle, fallback, packaging and acceptance evidence |
| [Code quality](code-quality.md) | Rust tooling and lint policy, test conventions, dependency review, host integration and release checks |
| [Labs](labs.md) | How engineering questions are settled by comparing working variants: plans, fixtures, boundaries, correctness checks, measurement, evidence and decisions |
| [Module layout](modules.md) | Peer product parts, capability ownership, shared contracts, delivery and bounded migration requirements |

The product root holds peer `extension/`, `server/`, `capture/`, `ui/`, `sdk/`
and `contracts/` owners. The browser canvas is in `ui/`, and the server's
Rust index and Node services are in `server/`. Projects provide
`workbench.yaml` and may override machine-specific values in the ignored
`workbench.local.yaml`. They do not receive a copy of the workbench code.

## Page sources

A page enters the page list from `workbench.yaml`, from the iOS Simulator
catalog, or from preview discovery. TypeScript previews (`*.workbench.ts` and
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
| iOS Simulator mapping | A lens on an authored page | A stream from the selected booted device |
| iOS Simulator catalog | `catalog: true` on a Simulator implementation | Imported booted devices, without a design lens |
| Window mapping | A lens on an authored page | A stream of the named window from the configured macOS app |

The specs describe VS Code behavior unless they explicitly mention standalone
browser or server use. A `file://` workbench has no server-backed catalog,
story lookup, or TypeScript previews.
