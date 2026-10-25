# First page after start

Part of the [preview pipeline](README.md). **Proposal:** this page holds design
constraints for a different compiler start. They are not agreed and not
implemented. [Performance](../system/performance.md#first-open-and-edits) owns
the first-page requirement, and its [status](../system/performance.md#status)
owns the gap; the [compiler](compiler.md) describes the implemented start, which
declares and builds every entry before serving the first.

## Design constraints

A different start must allow the requested page to build independently of the
catalog size while preserving stable module/chunk identity, freshness, build
errors and hot updates. Lazy compilation and incremental entry registration are
candidates only when new pages, build errors, configuration replacement and
already-open documents all meet their contracts. The implemented compiler
declares and builds every entry; no alternative start is adopted.

The [first-page research](../../research/first-page.md) and
[compiler-host research](../../research/instant-first-page.md) hold
candidate comparisons. Packaging requirements follow the
[extension process boundary](../integrations/vscode-extension.md#process-boundaries) and
[code-quality gates](../system/code-quality.md).

## Open questions

- **The first page after a restart.** The implemented eager start still
  prepares and builds every entry. How to show the requested first page
  within the [first-page target](../system/performance.md#measured-interactions)
  without waiting for catalog-size-dependent entry preparation and
  compilation remains open.
