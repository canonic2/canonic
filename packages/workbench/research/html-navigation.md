# HTML component-page navigation

## Sidebar investigation — 2026-10-23

The motivating page was `design/sidebar.html`, state `menu`, through its Design
lens at the Sidebar size. It is authored HTML that composes the shipping
`wb-space-switcher` and `wb-space-mark` with the legacy page list. It does not
use the warm Docs host. HTML states load separate documents; exact retained
returns reuse those documents.

The installed development extension reported version `0.15.0`. Diagnostic
scripts used the repository's `scripts/chrome.cjs` helper, CDP traces and native
macOS process samples. Browser profiles, fixtures, native samples and raw traces
were temporary and are not shared product assets. These were exploratory probes,
not a planned performance comparison or platform acceptance.

### Graphics stalls

Visible Chrome 155.0.8059.85 on Intel macOS 15.8.1 with an AMD Radeon Pro Vega 56
reproduced a retained return of about 932 ms. Its content was ready at about
11 ms and its frame promoted at about 17 ms; a GPU-main-thread task took about
931 ms wall time with about 15 ms thread CPU time during the same transition.

A subsequent GPU-service trace captured a 697 ms
`RasterDecoderImpl::DoEndRasterCHROMIUM::Flush`. A simultaneous three-second
native GPU-process sample showed a condition-variable wait on the GPU main
thread and worker calls through `newLibraryWithSource`,
`newRenderPipelineStateWithDescriptor`, and synchronous XPC requests to Apple's
Metal compiler service. This supports shader/pipeline compilation as the wait
in that reproduction; it does not identify the particular paint operation or
prove the same stall in the user's existing VS Code window.

Disabling Graphite in a temporary Chrome profile still produced Metal compiler
waits. Requesting OpenGL fell back to software rendering, rather than establishing
a usable hardware OpenGL alternative. Neither experiment supports changing the
product's graphics policy or recommending a global editor setting as a verified
fix. Some long GPU tasks named a renderer other than the inspected Workbench
renderer, so browser startup and other renderer work must remain separate from
the selected page's cost.

### Editor confirmation

An automated diagnostic extension opened the installed VS Code in a disposable
profile, embedded the installed Workbench server in a webview, drove the Sidebar
states, and closed its window. It did not require interaction with the user's
open editor. The runtime was Electron 43.7.7 / Chromium 150.0.7871.250, distinct
from the Chrome profiler and bundled Electron 44.2.0 / Chromium 152 capture runtime.
Hardware acceleration and Graphite were enabled in the editor diagnostic.

One sequence acknowledged the two new states in approximately 53–57 ms, with
most retained returns around 6–7 ms. These are host `wb-here` acknowledgements,
not painted-frame measurements. The one-second Chrome stall did not recur in
this sequence. A native sample of the existing editor did not include a Sidebar
transition and is not evidence about that transition.

A subsequent CDP probe showed that the disposable editor window could remain
`document.visibilityState === 'hidden'`. In that condition its ready messages
arrived before document-load completion, and the canvas did not emit a visible
phase. The acknowledgement sequences above therefore do not establish foreground
rendering latency or rule out an editor graphics stall. Navigation probes must
wait for each destination's visible phase before requesting the next one and
verify the editor window's visibility.

Activating the disposable editor process before each selection allowed a complete
foreground sequence with GPU compositing, rasterization and Graphite enabled.
With the import correction installed, selection to the canvas's visible phase
was 251 ms initially, 97 and 110 ms for new states, and 30, 28, 22 and 21 ms for
retained returns. This boundary is a paint opportunity, not a measurement of
physical display presentation. The trace contained compositor presentation
events, but they were not correlated to a particular content frame.

The largest GPU task in that trace was about 33 ms, with about 30 ms CPU; the
earlier 697 ms compilation wait did not recur. The largest page-renderer task
was an 81 ms module evaluation containing a 72 ms layout (about 54 ms CPU).
Retained documents were ready around 7–8 ms and promoted around 8–14 ms, with
the remaining interval reaching the canvas's next animation frame. These are
one sequence's observations, not latency percentiles or proof that the
intermittent graphics stall is fixed.

### Component import correction

The Sidebar bootstrap imported a combined registration module whose static
dependencies included all eight UI elements. Registration is split into space,
size, Actions and canvas families. The sidebar and design-example helpers import
their family directly; `register.js` retains the full-shell exports. The shared
registration rule still permits repeated registration and rejects conflicts.

A static browser probe against identical temporary Sidebar fixtures counted
38 resource requests for the installed bootstrap and 17 for the updated one.
The updated page loaded only the two space elements; both pages displayed the
open-menu state. The focused space-component browser test covers this dependency
boundary alongside existing interactions, lifecycle and CSP behavior.

Single editor-fixture sequences gave mixed latency changes: the installed control
acknowledged the two new states in about 16 and 35 ms, and the candidate in about
66 and 5 ms. These samples establish neither a percentage latency improvement
nor a fix for the graphics stall. The correction removes independently verified
unnecessary imports.

The candidate was measured from the dirty development checkout based on private
commit `29a284bf9b5dea0022c8bee75e845eb6f9379e7b`. SHA-256 identities:

- `ui/src/bootstrap/sidebar.js`: `c78274d78f4c1a2a9c3fcb9bdd8c6cac43e473511f433fabdeea16463508f077`
- `ui/src/bootstrap/register-space.js`: `3245c4e3b797dbf86884a3312c2c32564a12598d3991086021e264f799441591`

### Closed-menu layout correction

A follow-up trace with JavaScript call stacks reproduced the expensive layout
in the same installed VS Code runtime. `WbSpaceSwitcher.#render()` read the
menu's `scrollTop` after changing the trigger and mark DOM, even during
construction and closed-menu input updates. In the editor control fixture,
that read forced a 69.5 ms document layout during custom-element registration.
A separate Chrome probe also measured a 22 ms layout at the same read when
the shell supplied icons to a closed switcher.

The correction reads scroll position before DOM changes, and only for an open,
connected menu. Closed updates still render synchronously and leave layout to
the browser's paint cycle. An open menu retains its explicit scroll restoration
and focus behavior. The public properties, registration and navigation remain
the same.

The focused real-browser regression test records creation and closed input
updates between User Timing marks, and asserts that the renderer executes no
layout inside that synchronous boundary. It failed on the control with four
layouts and passed after the correction. The same test exercises a long open
menu and verifies that updates preserve its scroll offset and focused action,
alongside the existing keyboard, lifecycle, input and CSP checks in both hosts.

One foreground editor sequence per identical temporary fixture gave these
selection-to-visible observations, with tracing enabled:

| Destination | Control | Corrected |
| --- | --- | --- |
| Initial menu | 305 ms | 222 ms |
| New filtered state | 146 ms | 97 ms |
| New default state | 102 ms | 95 ms |
| Four retained returns | 32–35 ms | 25–31 ms |

Across those sequences, layouts whose call stack included the switcher fell
from 25 (76.7 ms total, 69.5 ms maximum) to two (6.7 ms total, 6.4 ms maximum).
The remaining read occurred during connection after replaying the pre-upgrade
open state. These sequential exploratory runs do not control system/font-cache
warming and do not establish a percentage speedup or latency distribution.
They verify the removed forced-layout path and continued rendering through
the VS Code webview. They do not resolve the separate Metal compiler stall.

SHA-256 identities of `ui/src/components/space-switcher/element.js`:

- Control: `f282229394d9aa2e4d7711347c2de3431010fa144b9e302b44dd94269763ae98`
- Corrected: `23bf8fe5aa8c102b2e5e889855ffcfc21eadc078f2e93a73da2837e104559ab2`

### Remaining first-selection work and icon dependency

The corrected 222 ms sequence split into about 83 ms to content-renderer
readiness, 122 ms from there to project-document load, and 17 ms from load to
the canvas's visible phase. These are wall-time phases, not HTML execution time.
The renderer loads its scripts/module graph and configuration before requesting
the project document; the latter starts around 111 ms from selection in this
trace. Native module imports then discover more requests in successive rounds.

The Sidebar page also loaded the complete 419,264-byte Lucide UMD library.
Its evaluation took about 20 ms in the initial page document and 15–19 ms in
subsequent new-state documents. The parser-blocking script delayed discovery of
the component registration graph. The authored page now imports its eleven
displayed icons directly and initializes its controls in the same native module.
This reduces the page's own dependency cost; it does not change arbitrary
projects' icon choices or remove the Workbench content renderer.

Direct module imports exposed that authored `.mjs` assets were served as
`application/octet-stream`, which the browser refused as JavaScript modules.
The static-file server now serves `.mjs` as JavaScript, like `.js`. The scoped
Rust file-response regression verifies content type and unchanged source; the
Sidebar browser test verifies all three states, nonempty icons in the page and
nested Shadow DOM, and a total icon-module budget below 20 KB. The resource
budget failed against the full-library page before the change.

A follow-up foreground VS Code comparison used the rebuilt release server for
both fixtures and retained the preceding layout fix in both. The original HTML
loaded 419,264 icon bytes per document; the revised HTML loaded 4,843 bytes across
eleven modules. First selection measured 195 ms for the full-library control and
176 ms for the revised page. New states measured 101/102 ms versus 96/84 ms;
retained returns were 24–33 ms versus 25–39 ms, with no demonstrated improvement.
These are single sequential sequences with tracing, not controlled percentiles.
The control's full-library evaluations took about 15–18 ms each. The revised
initial selection reached renderer readiness at 62 ms, document load at 141 ms,
promotion at 156 ms and the canvas's visible phase at 176 ms. Renderer startup
and the reveal handshake remain material costs; the icon change does not remove
them or establish a universal first-open latency.

Measured `design/sidebar.html` SHA-256:
`39bc39c9e448dee3e3c7ac84486fdcea4a4505482fb397ffdd60f890ca9302f2`.
The packaged and installed host server binaries matched SHA-256
`c29323dac844cfa713dfba4dab50a293e6664b671df4387b9897c58e8592b8cd`.

The [performance contract](../specs/system/performance.md) owns timing boundaries and
budgets. Further graphics work needs a captured failing editor transition and
a verified rendering-path correction; removing imports alone does not establish it.
