# Research and verification evidence

Dated observations and comparisons explain design decisions and qualify
implementation status. They do not replace current contracts, change budgets
or certify an untested build, fixture or host. [Performance](../specs/system/performance.md)
owns measurement semantics and targets; the [lab method](../specs/system/labs.md)
owns how comparisons are run.

Each lab ends as one record here, `<topic>.md`, named for its question, with
the sections Question, Status, Hypothesis, Method, Conditions, Results,
Correctness, Threats to validity, and Decision and follow-ups. A rerun or
follow-up adds a dated section to the same record. Requirements and adopted
behavior belong to the owning contract; the record explains the evidence and
tradeoff. Lab runners, raw samples and reports stay in the ignored `labs/`
folder and are not committed.

| Record | Status | Related contracts |
| --- | --- | --- |
| [Artboard swapper](artboard-swapper.md) | Decided 2026-10-27: adopted as the canvas rendering model with its texture atlas (900 draw calls → 2, frame p50 2.0 → 1.0 ms); WebGPU and PNG kept, vector replay dropped | [Canvas rendering](../specs/canvas/rendering.md), [performance](../specs/system/performance.md) |
| [Multicore WebAssembly canvas](multicore-canvas.md) | Recommended 2026-10-27, owner to decide: serve live content cross-site (removes iframe jank entirely); keep rendering on the main thread (a render worker adds a frame of latency); multicore Wasm is 6× faster at 100k but not needed at 60 Hz | [Canvas rendering](../specs/canvas/rendering.md), [performance](../specs/system/performance.md) |
| [Live content isolation](live-content.md) | Recommended 2026-10-27, owner to decide: serve live content from one separate site (canvas freezes over 100 ms while pages load: 9–28 → 0–2 per run); hot updates stay smooth at 20 live, but each loading page still costs a frame; no origin per artboard; stop resending 9.9 MB of unused files per stream connection; memory unmeasured | [Canvas rendering](../specs/canvas/rendering.md), [serving](../specs/preview-pipeline/serving.md), [performance](../specs/system/performance.md) |
| [100k free-form artboards and Rust/Wasm](free-artboards-wasm.md) | Decided 2026-10-27: JavaScript stays; Wasm is 1.4–2× faster on the index and about even on tiles, but JavaScript meets every budget and frame pacing is the same; one instanced draw holds 60 FPS at 100k | [Rendering flow](../specs/system/rendering-flow.md), [artboards](../specs/canvas/multiple-artboards.md), [performance](../specs/system/performance.md) |
| [Chrome paint records as vectors](iframe-paint-record.md) | Decided 2026-10-27: the paint record has a complete replayable payload and replay is sharp to 800%; stock access needs `--no-sandbox`, so pursue only with a native capture hook | [Rendering flow](../specs/system/rendering-flow.md), [capture](../specs/review/capture.md) |
| [Compiler speed](compiler-speed.md) | Reported 2026-10-11; esbuild kept, Rspack later adopted by the pipeline decision | [TypeScript previews](../specs/pages/previews.md), [performance](../specs/system/performance.md) |
| [Preview loading](preview-loading.md) | Provisional, one run set (2026-10-11); led to the Rspack pipeline comparison | [Performance](../specs/system/performance.md), [preview pipeline](../specs/preview-pipeline/README.md) |
| [Rspack preview pipeline](rspack-pipeline.md) | Decided 2026-10-13: implemented with the warm document | [Preview pipeline](../specs/preview-pipeline/README.md), [hot updates](../specs/preview-pipeline/hot-updates.md) |
| [First page after start](first-page.md) | Decided 2026-10-15: no variant adopted | [Compiler](../specs/preview-pipeline/compiler.md), [warm runtime](../specs/preview-pipeline/warm-runtime.md) |
| [Instant first page and Rust preview host](instant-first-page.md) | Stage 1 led to the Rust page index (2026-10-15); stage 2 reported 2026-10-16, adoption open | [Page index](../specs/preview-pipeline/page-index.md), [compiler](../specs/preview-pipeline/compiler.md) |
| [Page listing](page-listing.md) | 2026-10-15 release-server scaling probes; not canvas-paint acceptance | [Page index](../specs/preview-pipeline/page-index.md), [performance](../specs/system/performance.md) |
| [Docs navigation and warm pool](docs-navigation.md) | Decided and confirmed 2026-10-23: warm docs pool adopted | [Docs](../specs/pages/docs-pages.md), [warm runtime](../specs/preview-pipeline/warm-runtime.md), [performance](../specs/system/performance.md) |
| [Docs highlighting](docs-highlighting.md) | Decided 2026-10-21: asynchronous highlight.js adopted; integrated latency unmeasured | [Docs](../specs/pages/docs-pages.md), [readiness](../specs/preview-pipeline/readiness.md) |
| [Installed startup](startup.md) | 2026-10-23 runtime preparation race and shared extraction; not a controlled speed comparison | [Capture runtime](../specs/review/capture.md#bundled-electron-runtime), [VS Code](../specs/integrations/vscode-extension.md) |
| [HTML component-page navigation](html-navigation.md) | 2026-10-23 Sidebar import correction and Chrome compiler stalls; no verified graphics fix | [Performance](../specs/system/performance.md), [rendering flow](../specs/system/rendering-flow.md) |

Preserve dates, fixture setup, source/build identity, cache state, host/tool
versions, sample counts, failures and limitations with each finding. Paths and
host details use neutral placeholders.
