# Verification map

This page maps each capability to the evidence that exists for it and to what
is missing. It does not restate status: each topic states its own at its top,
and its acceptance criteria stay with it. Paths are relative to this file.

Three kinds of evidence are kept apart:

- **Automated coverage:** tests in the package's suites that exercise the
  topic's contracts. A real-browser test counts here, not as host acceptance.
- **Host acceptance:** a recorded check of the behavior on a named host,
  with a link to where it is recorded. A smoke script that exists without a
  recorded result is listed as a script, not as acceptance.
- **Gaps:** the topic's own gaps section, which names unmet requirements and
  missing tests.

[Code quality](code-quality.md) owns which gates run and how;
[labs](labs.md) owns how research evidence is produced.

## Automated coverage

| Capability | Automated coverage | Gaps |
| --- | --- | --- |
| [Core contract](../core.md) | `../../ui/legacy/address.test.js`, `../../ui/src/views/page-list/page-list.test.cjs`, `../../server/node/config.test.js`, `../../contracts/manifest.test.js`, `../../server/node/server.test.js` | [Core](../core.md#implementation-gaps) |
| [Page tree](../pages/page-tree.md) | `../../server/node/config-parity.test.js`, `../../contracts/manifest.test.js`, `../../ui/src/views/page-list/page-list.test.cjs` | [Page tree](../pages/page-tree.md#implementation-gaps) |
| [Scenarios](../pages/scenarios.md) | `../../ui/legacy/scenarios.test.js`, `../../contracts/manifest.test.js`, `../../server/node/config.test.js` (not mapped in the topic) | The topic maps no tests |
| [Artboards and iterations](../pages/iterations.md) | `../../contracts/manifest.test.js`, `../../server/node/config-parity.test.js`, `../../server/node/preview.test.js`, `../../ui/preview/preview-runtime.test.cjs`, `../../ui/legacy/address.test.js`, `../../ui/legacy/reference.test.js` (not mapped in the topic) | [Iterations](../pages/iterations.md#implementation-gaps) |
| [Lenses](../canvas/lenses.md) | `../../contracts/manifest.test.js`, `../../server/node/config.test.js`, `../../server/node/preview.test.js`, `../../ui/legacy/preview.test.js`, `../../ui/legacy/reference.test.js` | None recorded |
| [Sizes](../canvas/sizes.md) | `../../contracts/manifest.test.js`, `../../server/node/config.test.js`, `../../server/src/handoff/tests.rs`, `../../ui/src/modules/canvas/canvas.test.js`, `../../ui/src/views/top-bar/components/size-switcher/element.test.js` (real Chrome), `../../ui/src/views/top-bar/components/size-dialog/form.test.js`, `../../ui/src/views/top-bar/components/sizes-editor/rows.test.js` | [Sizes](../canvas/sizes.md#implementation-gaps) |
| [Navigation](../canvas/navigation.md) | `../../ui/src/modules/navigation/controller.test.js`, `../../ui/src/modules/preview-sessions/controller.test.js`, `../../server/node/src/modules/host/navigation.test.ts` (real Chrome through shell and renderer) | [Navigation](../canvas/navigation.md#implementation-gaps) |
| [Navigation store](../canvas/navigation-store.md) | None: not implemented | [Store](../canvas/navigation-store.md#implementation-gaps) |
| [Interaction](../canvas/interaction.md) | `../../ui/legacy/zoom.test.js`, `../../ui/legacy/keys.test.js`, `../../extension/panel.test.js`, `../../ui/src/views/canvas/components/canvas/element.test.js` | [Interaction](../canvas/interaction.md#implementation-gaps) |
| [Annotations](../canvas/annotations.md) | `../../ui/legacy/annotations-capture.test.js`, `../../ui/src/modules/review/controller.test.js` | [Annotations](../canvas/annotations.md#implementation-gaps) |
| [Persistence](../canvas/persistence.md) | None specific to restoration | [Persistence](../canvas/persistence.md#implementation-gaps) |
| [Multiple artboards](../canvas/multiple-artboards.md) and [artboard system](../canvas/artboard-system.md) | `../../ui/src/modules/canvas/canvas.test.js`, `../../ui/src/modules/canvas/publication.test.js`, `../../ui/src/modules/review/capture.test.js`, `../../ui/src/modules/review/controller.test.js`, `../../ui/src/views/canvas/components/canvas/element.test.js` | [Multiple artboards](../canvas/multiple-artboards.md#implementation-gaps) |
| [Artboard management](../canvas/artboard-management.md) | None: proposal | [Delivery plan](../canvas/artboard-management.md#delivery-plan) |
| [TypeScript previews](../pages/previews.md) | `../../server/node/preview.test.js`, `../../server/node/preview-astro.test.js`, `../../ui/preview/preview-runtime.test.cjs`, `../../ui/preview/requests.test.cjs`, `../../ui/src/app/preview-host/preview-host.test.cjs`, `../../ui/legacy/preview.test.js` | [Previews](../pages/previews.md#implementation-gaps) |
| [Docs](../pages/docs-pages.md) and [Docs host](../preview-pipeline/docs-host.md) | `../../server/node/preview-docs.test.js`, `../../ui/src/modules/docs/canvas/docs-layout.test.js`, `../../ui/src/app/docs/docs-host.test.js`, `../../ui/src/views/docs/mount-examples.test.js`, `../../ui/src/views/docs/highlighting.test.js`, `../../contracts/src/docs/lenses.test.ts`, `../../ui/legacy/content-frames.test.cjs`, `../../ui/legacy/preview-sessions.test.js` | [Docs](../pages/docs-pages.md#implementation-gaps) |
| [Authored pages and implementations](../pages/implementations.md) | `../../server/node/config.test.js`, `../../server/node/server.test.js`, `../../server/node/gate.test.js`, `../../ui/legacy/lenses.test.js`, `../../ui/legacy/simulator.test.js`, `../../capture/window-stream.test.js`, `../../capture/src/native-streams/pool.test.ts`, `../../extension/startup.test.js` | [Implementations](../pages/implementations.md#implementation-gaps) |
| [Storybook pages](../pages/storybook.md) and [adapter](../adapters/storybook.md) | `../../server/node/server.test.js`, `../../server/node/proxy.test.js`, `../../server/node/config.test.js`, `../../ui/legacy/lenses.test.js`, `../../ui/legacy/keys.test.js`, `../../server/src/storybook.rs` unit tests | [Storybook](../pages/storybook.md#implementation-gaps) |
| [Configure pages](../pages/configuration-editor.md) | `../../server/src/config/edit/text.rs` unit tests, `../../ui/legacy/config-editor.test.js` | [Configure pages](../pages/configuration-editor.md#implementation-gaps) |
| [Page index](../preview-pipeline/page-index.md) | `../../server/node/preview.test.js` (parity), `../../server/node/server.test.js`, `../../server/tests/integration/main.rs`, `../../server/src/catalog/answer.rs`, `../../server/src/literal.rs`, `../../server/src/saved.rs` | [Page index](../preview-pipeline/page-index.md#implementation-gaps) |
| [Compiler](../preview-pipeline/compiler.md), [serving](../preview-pipeline/serving.md) and [hot updates](../preview-pipeline/hot-updates.md) | `../../server/node/preview.test.js`, `../../server/node/preview-docs.test.js`, `../../server/node/preview-module.test.js`, unit tests in `../../server/src/compiler/` | [Compiler](../preview-pipeline/compiler.md#implementation-gaps), [serving](../preview-pipeline/serving.md#implementation-gaps), [hot updates](../preview-pipeline/hot-updates.md#implementation-gaps) |
| [Warm runtime](../preview-pipeline/warm-runtime.md) | `../../ui/src/app/preview-host/preview-host.test.cjs`, `../../ui/preview/preview-runtime.test.cjs`, `../../ui/legacy/content-frames.test.cjs`, `../../ui/legacy/preview-sessions.test.js` | [Warm runtime](../preview-pipeline/warm-runtime.md#implementation-gaps) |
| [Readiness](../preview-pipeline/readiness.md) | `../../ui/preview/preview-runtime.test.cjs`, `../../ui/legacy/content-frames.test.cjs` | [Open questions](../preview-pipeline/readiness.md#open-questions) |
| [Plugins](../preview-pipeline/plugins.md) and [adapters](../adapters/README.md) | `../../server/node/preview.test.js`, `../../server/node/preview-astro.test.js`, unit tests in `../../server/src/compiler/` (`plugins.rs`, `node_loader.rs`, `html.rs`, `vue.rs`) | [Plugins](../preview-pipeline/plugins.md#implementation-gaps), [adapters](../adapters/README.md#implementation-gaps) |
| [Interactive capture](../review/capture.md) | `../../capture/electron-runtime.test.js`, `../../capture/electron-capture.test.js`, `../../capture/capture-helper.test.js`, `../../capture/capture-scripts.test.js`, `../../ui/legacy/annotations-capture.test.js`, `../../ui/legacy/capture-sync.test.js`, `../../ui/src/app/capture/capture.test.cjs`, `../../ui/legacy/dom-mirror.test.js`, `../../server/src/handoff/tests.rs` | [Capture](../review/capture.md#implementation-gaps) |
| [Design-system export](../review/export.md) | `../../server/node/server.test.js`, export module tests in `../../server/src/export/`, `../../server/node/preview.test.js`, `../../ui/src/app/capture/capture.test.cjs` | [Export](../review/export.md#verification-points) |
| [Web Components](../ui/web-components.md) and [themes](../ui/themes.md) | `../../ui/src/components/design-pages.test.js` and each element's real-Chrome `element.test.js` | [Components](../ui/web-components.md#implementation-gaps), [themes](../ui/themes.md#implementation-gaps) |
| [VS Code extension](../integrations/vscode-extension.md) | `../../extension/panel.test.js`, `../../extension/sidebar-view.test.js`, `../../extension/startup.test.js`, `../../ui/src/modules/project/sidebar.test.cjs`, `../../ui/src/app/sidebar/sidebar.test.js`, `../../server/node/spaces.test.js` | [Extension](../integrations/vscode-extension.md#implementation-gaps) |
| [Agent context](../integrations/agent-context.md) | `../../server/src/agent_view/tests.rs`, `../../server/node/server.test.js` | [Open questions](../integrations/agent-context.md#open-questions) |
| [Modules](modules.md) and [rendering flow](rendering-flow.md) | `../../scripts/stage-extension.test.cjs`, `../../ui/src/modules/preview-sessions/controller.test.js` | [Modules](modules.md#implementation-gaps), [rendering flow](rendering-flow.md#acceptance-and-verification) |
| [Browser UI port](ui-port.md) | None: not implemented | [Port](ui-port.md#implementation-gaps) |
| [Server](server.md) | `../../server/node/server.test.js`, `../../server/node/config-parity.test.js`, `../../server/tests/integration/main.rs` | [Server](server.md#implementation-gaps) |
| [Access boundaries](access-boundaries.md) | `../../server/node/server.test.js`, `../../server/src/serve/files.rs` and `../../server/src/serve/routes/canvas.rs` unit tests | [Open questions](access-boundaries.md#open-questions) |
| [Implementation proxy](implementation-proxy.md) | `../../server/node/proxy.test.js`, `../../server/src/proxies.rs` unit tests, `../../ui/legacy/lenses.test.js` | [Proxy](implementation-proxy.md#implementation-gaps) |
| [Performance](performance.md) | No automated budget checks | [Performance](performance.md#implementation-gaps) |

## Host acceptance

Hosts: the standalone server in a browser, the packaged extension in VS Code
on each platform, and portable static export. A capability not listed below has
no recorded host acceptance beyond its automated tests.

| Capability | Standalone browser | Packaged VSIX, macOS x64 | macOS arm64, Windows, Linux | Portable export |
| --- | --- | --- | --- | --- |
| Docs, Docs host | [Warm Docs integration](../../research/docs-navigation.md#warm-docs-integration-2026-10-23) | [Warm Docs integration](../../research/docs-navigation.md#warm-docs-integration-2026-10-23) (isolated profile) | Gap | Highlighting: [integrated highlighting](../../research/docs-highlighting.md#integrated-asynchronous-highlightjs-2026-10-21) |
| Docs highlighting readiness | Gap | [Installed asynchronous path](../../research/docs-highlighting.md#installed-asynchronous-path-2026-10-20) | Gap | As above |
| Startup and runtime preparation | Not applicable | [Installed runtime preparation](../../research/startup.md#installed-runtime-preparation-2026-10-23) (platform not recorded) | Gap | Not applicable |
| Page index listing | Server-only probes: [page listing](../../research/page-listing.md#page-listing) | Gap | Gap | Not applicable |
| Compiler, hot updates, readiness | Prototype pipeline only, not the native compiler: [Rspack pipeline](../../research/rspack-pipeline.md#confirmation-run-2026-10-13) | Gap | Gap | Gap |

Smoke scripts exist for the canvas (`../../scripts/smoke-canvas.cjs`,
`../../scripts/smoke-canvas-ui.cjs`), retained sessions and Storybook
(`../../scripts/smoke-preview-sessions.cjs`), lenses and sign-in
(`../../scripts/smoke-lenses.cjs`) and capture (`../../scripts/smoke-capture.cjs`,
`../../scripts/smoke-mirror.cjs`); a run becomes host acceptance when its
result is recorded in [research](../../research/README.md).

## Gaps across capabilities

- Every capability lacks recorded acceptance on macOS arm64, Windows and Linux.
- The full format-by-format confirmation through the native server and the
  VS Code webview has not been recorded; the Rspack pipeline record covers a
  prototype pipeline, not the native compiler.
- No capability records acceptance in a VS Code development host; the
  VS Code evidence above comes from the packaged extension.
