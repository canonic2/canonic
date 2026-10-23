# Research and verification evidence

Dated observations explain design decisions and qualify implementation status.
They do not replace current contracts, change budgets or certify an untested
build, fixture or host. [Performance](../system/performance.md) owns measurement
semantics and targets; [lab methodology](../system/labs.md) owns comparisons.

| Record | Evidence and limits | Related contracts |
| --- | --- | --- |
| [Page listing](page-listing.md) | 2026-10-15 release-server scaling probes, warm/cold indexes and loaded-host repeat; not canvas-paint acceptance | [Page index](../preview-pipeline/page-index.md), [performance](../system/performance.md) |
| [Installed startup](startup.md) | 2026-10-23 runtime preparation race, shared extraction and isolated installed diagnostic; not a controlled speed comparison | [Capture runtime](../review/capture.md#bundled-electron-runtime), [VS Code](../integrations/vscode-extension.md) |
| [Docs navigation](docs-navigation.md) | 2026-10-19 through 2026-10-23 navigation/readiness probes, warm-pool comparisons, shipping confirmation and installed Acme compatibility | [Docs](../pages/docs-pages.md), [warm runtime](../preview-pipeline/warm-runtime.md), [performance](../system/performance.md) |
| [Docs highlighting](docs-highlighting.md) | 2026-10-20 through 2026-10-21 highlighting comparisons, integration/installed/portable checks and unrun warm-up follow-up | [Docs](../pages/docs-pages.md), [readiness](../preview-pipeline/readiness.md) |

Preserve dates, fixture setup, source/build identity, cache state, host/tool
versions, sample counts, failures and limitations with each finding. Keep reviewed,
sanitized evidence in the lab result locations. Requirements and adopted
behavior belong to the owning contract; the record explains the evidence and
tradeoff. New observations extend the relevant record or add a distinct topic.
Archived plan copies with recorded provenance hashes retain their original bytes
and historical paths. Active lab indexes link to the current contracts; use those
indexes to resolve references from a frozen plan.

## Lab records and active follow-up

| Record | Current relevance and remaining work |
| --- | --- |
| [Warm Docs](../../labs/warm-docs/README.md) | Pool and background return checks are implemented. Shipping timing/long-walk and isolated installed Acme checks have evidence in [Docs navigation](docs-navigation.md#warm-docs-integration-2026-10-23), including tail and host limitations |
| [Docs highlighting](../../labs/docs-highlighting/README.md) | Owner selected asynchronous highlight.js after the 1,200-trial comparison; [highlighting evidence](docs-highlighting.md) keeps prototype regressions and integration/portable/installed qualifications distinct |
| [Preview loading](../../labs/preview-loading/README.md) | Shared-build/runtime comparison; navigation requirements belong to [performance](../system/performance.md) |
| [Rspack pipeline](../../labs/rspack-pipeline/README.md) | Design/correctness evidence for compilation and hot updates; full shipping-server/webview confirmation remains open |
| [First page](../../labs/first-page/README.md) | Startup-scaling comparisons; no alternative to the eager startup contract is adopted |
| [Instant first page](../../labs/instant-first-page/README.md) | Native index and compiler decision evidence; no compared variant met every first-page target |
| [Compiler speed](../../labs/compiler-speed/README.md) | Compiler selection evidence; the active compiler is Rspack |

The warm Docs archive retains the plan, reduced reviewed reports, accepted
memory tradeoff and adoption decision after integrated confirmation. Product
tests own regression coverage. Archiving establishes no new performance result.
