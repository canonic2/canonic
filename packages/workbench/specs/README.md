# Workbench specifications

Internal product contracts, system boundaries and acceptance evidence live here.
[User guides](../docs/README.md) describe supported behavior; the
[development guide](../DEVELOPMENT.md) owns development commands and source locations.
A specification can define agreed future behavior without claiming it is shipped.

Start with [core](core.md) and [glossary](glossary.md), then the folder that owns
the change. Each requirement has one owner. Cross-cutting summaries and research
link to that owner rather than creating another version of its rules.

## Layout and ownership

| Area | Owns | Boundary |
| --- | --- | --- |
| Root: [core](core.md), [glossary](glossary.md) | Product model, configuration composition, shared navigation and problem reporting | Summarizes capabilities; detailed rules stay with their topic |
| [Canvas](canvas/README.md) | Lens selection, sizes, durable restoration and artboard composition | Live documents and compilation belong to the preview pipeline; UI primitives belong to UI |
| [Pages](pages/README.md) | Preview/Docs authoring, authored implementations and Storybook page workflows | Adapters own technology-specific rendering; pipeline owns shared delivery |
| [Review](review/README.md) | Interactive capture, handoff acquisition and design-system export | Agent delivery belongs to integrations; export jobs do not redefine canvas navigation |
| [UI](ui/README.md) | Web Components, semantic themes and presentation lifecycle | Capability controllers own state and I/O; project content keeps its own styles |
| [Integrations](integrations/README.md) | VS Code host lifecycle and delivery of canvas context to agents | Canvas persistence owns saved data and precedence; server owns transport/process rules |
| [System](system/README.md) | Architecture, rendering boundaries, server/proxy ownership, performance budgets, engineering gates and lab method | Capability specs own behavior; research owns dated observations |
| [Preview pipeline](preview-pipeline/README.md) | Discovery, compilation, serving, hot updates, warm documents and readiness | Page formats own semantics; system performance owns budgets |
| [Adapters](adapters/README.md) | Technology-specific build, server evaluation, mount and update behavior | Shared lifecycle and resource bounds stay with the pipeline |

Keep folders one level deep. Add a topic when it has a distinct owner; split
mixed contracts rather than splitting solely by length. Folder indexes explain
ownership and reading paths. Keep implementation plans beside the capability
they develop, separate from current behavior and observed acceptance.

## Reading paths

| Task | Read |
| --- | --- |
| Understand routing and failures | [Core](core.md), then the owning [page format](pages/README.md) |
| Implement restart restoration | [Persistence](canvas/persistence.md), then [VS Code](integrations/vscode-extension.md#persistence-and-restored-editor-tabs) |
| Change artboard composition | [Artboards](canvas/multiple-artboards.md), [artboard system](canvas/artboard-system.md), then [rendering flow](system/rendering-flow.md) |
| Change project rendering | [Pages](pages/README.md), the relevant [adapter](adapters/README.md), then [pipeline](preview-pipeline/README.md) |
| Change a shell component or theme | [Web Components](ui/web-components.md), [themes](ui/themes.md), then the owning capability |
| Change drawing, focus or shortcuts | [Annotations](canvas/annotations.md) and [interaction](canvas/interaction.md) |
| Change configuration writes | [Configure pages](pages/configuration-editor.md), [sizes](canvas/sizes.md) and [access boundaries](system/access-boundaries.md) |
| Check host prerequisites or upgrades | [Hosting](integrations/hosting.md), then the owning host/capability |
| Change processes or host messages | [Server](system/server.md), then [VS Code](integrations/vscode-extension.md) |
| Investigate performance | [Budgets and measurement](system/performance.md), [research](../research/README.md), then [lab method](system/labs.md) |
| Assess verification | [Verification index](system/verification.md), then the capability's verification section and [quality gates](system/code-quality.md) |

## Status and verification

Each topic states its status once, in a paragraph at its top, and lists its
unmet requirements in its own gaps section. [Verification](system/verification.md)
maps automated coverage and host acceptance evidence across capabilities and
hosts. An implemented support API, passing unit check or prototype comparison
does not establish a complete user workflow or acceptance on every host.

## Maintaining the boundaries

- Put required behavior, failure/recovery rules and acceptance in the capability
  that owns them. Keep current implementation, agreed future work and proposals explicit.
- Put shared architecture and engineering policy in `system/`; UI presentation
  contracts in `ui/`; technology-specific differences in `adapters/`.
- Put dated findings in the package's top-level [research](../research/README.md)
  folder, with fixture, host, source identity and limitations; each lab ends as
  a research record there, while its runners and raw reports stay in the ignored
  `../labs/` folder. Record the adopted
  behavior in the owning contract.
- Update incoming links and anchors when moving or splitting a topic, including
  user guides, source comments and research records.
- Keep public guide routes stable. Reorganizing specs changes neither runtime
  behavior nor product requirements and establishes no new test results.
