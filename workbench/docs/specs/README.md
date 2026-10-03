# Workbench specifications

These files record the current, observable contract of the workbench shipped in
the Workbench VS Code extension. They are a starting point for reviewing changes:
update the relevant spec when behavior changes, then update the implementation
and its tests. The [workbench README](../../workbench/README.md) remains the
setup and operations guide.

| Spec | Covers |
| --- | --- |
| [Core workbench](core.md) | Configuration, navigation, selection, frames, editor integration, and failures shared by every screen |
| [Storybook](storybook.md) | Explicit and imported stories, catalog lookup, story selection, preview reuse, and startup |
| [Other previews](other-previews.md) | Authored pages, URL implementations, iOS Simulator implementations, and window implementations |
| [VS Code extension](vscode-extension.md) | Activation, webviews, server and process lifecycle, startup, and editor handoff |
| [Interactive capture](capture.md) | Bundled Electron helper, live mirroring, camera, handoff, fallbacks, and limits |
| [Design-system export](export.md) | Background reference captures, worker scheduling, ZIP contents, hashes, and warnings |

The workbench is one implementation in `packages/workbench/workbench/`. Projects provide
`workbench.yaml` and may override machine-specific values in the ignored
`workbench.local.yaml`. They do not receive a copy of the workbench code.

## Preview types

| Preview | How it enters the list | What appears on the canvas |
| --- | --- | --- |
| Authored page | A `sections[].items[]` entry with `src` | The project HTML file as designed |
| URL implementation | A lens on an authored page | The configured external page in an iframe |
| Storybook mapping | A lens on an authored page | One story from the mapped Storybook title |
| Storybook catalog | `catalog: true` on a Storybook implementation | Imported titles and stories, without a design lens |
| iOS Simulator mapping | A lens on an authored page | A stream from the selected booted device |
| iOS Simulator catalog | `catalog: true` on a Simulator implementation | Imported booted devices, without a design lens |
| Window mapping | A lens on an authored page | A stream of the named window from the configured macOS app |

The specs describe VS Code behavior unless they explicitly mention standalone
browser or server use. A `file://` workbench has no server-backed catalog or
story lookup.
