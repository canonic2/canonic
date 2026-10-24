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
