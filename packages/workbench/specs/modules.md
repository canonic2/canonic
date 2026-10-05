# Workbench module layout

## Contract

New capability modules and substantial capability migrations use
`src/modules/<capability>/`. A capability owns coherent behavior, state and
lifecycle, rather than representing each importable file as a separate module.
The shared [architecture rule](../../../.canonic/rules/module-architecture.md)
and [Workbench guide](../../../.canonic/guides/workbench/AGENTS.md) define the
engineering and runtime constraints.

| Location | Ownership |
| --- | --- |
| `src/modules/<capability>/` | Capability logic, workflows, dedicated adapters, types and behavioral tests |
| `src/components/<name>/` | Shared browser presentation and interaction, following the [Web Components contract](web-components.md) |
| `src/theme/` | Shared theme assets and host mappings, following the [theme contract](themes.md) |
| `src/server/` | Shared server transport, browser asset serving and webview delivery |
| Application entry points | Dependency composition, host registration and application shutdown |

Keep filesystem, HTTP, compiler and browser adapters dedicated to a capability
with that capability. Infrastructure used across capabilities has an explicit
owner outside the capability folders. A capability controller owns operations
and passes inputs to components; components render and emit events.

Expose deliberate public entry points. Consumers use module contracts instead
of private state or incidental files. Pure logic imports no DOM, Node or host
frameworks; browser-safe entry points cannot import host-only adapters. Folder
placement does not replace these dependency and ownership boundaries.

## Migration

Migrate within the requested capability when a change substantially reworks it.
Local fixes may stay in place. Move related ownership together without requiring
a package-wide reorganization or creating empty architecture folders.

A path migration updates callers, runtime imports, browser asset allowlists,
webview resource loading/CSP and packaging references in the same change.
Preserve public behavior, Node 24 loading, browser type stripping and the
package's existing test conventions. Verify the affected entry points and
delivery surfaces. Update source links and current-layout documentation when
the implementation moves.

## Current status

The layout convention was agreed on 2026-10-04. Existing capabilities include
`src/sizes/`, `src/canvas/` and other directories directly under `src/`; the
host still includes CommonJS files such as `server.js`. These capabilities
have not yet migrated to the target directory layout.

Docs are TypeScript under `src/modules/docs/`. The server, the compiler and
the portable export use `index.ts`; the canvas loads `canvas/bootstrap.ts`,
which hands the lens rules (`canvas/lenses.ts`) and the docs geometry to the
classic scripts, and a docs page loads `page/docs-page.ts`. The
`workbench.yaml` rules for docs stay in `workbench/manifest.js`, which both
config readers share, and the canvas's use of the lens rules stays in
`workbench/workbench.js`.

The design-system source and archive exporter is TypeScript under
`src/modules/export/`, exposed through `index.ts`. Its [product contract](export.md)
remains independent of the path migration. Server job orchestration, capture
and preview compilation remain with their existing owners. The
[multiple-artboard code plan](multiple-artboards-code-plan.md) distinguishes
actual paths from future integration paths.
