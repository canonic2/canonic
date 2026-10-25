# First page after start

Part of the [preview pipeline](README.md). This page holds design constraints
for a different compiler start. [Performance](../system/performance.md#first-open-and-edits)
owns the first-page requirement; the [compiler](compiler.md) describes the
implemented start.

**Status:** Proposal, not agreed and not implemented. The implemented compiler
declares and builds every entry before serving the first; no alternative start
is adopted. The resulting first-page gap is owned by
[performance](../system/performance.md#implementation-gaps); this topic's unresolved
decision is in [open questions](#open-questions).

## Design constraints

A different start must allow the requested page to build independently of the
catalog size while preserving stable module/chunk identity, freshness, build
errors and hot updates. Lazy compilation and incremental entry registration are
candidates only when new pages, build errors, configuration replacement and
already-open documents all meet their contracts.

The [first-page research](../../research/first-page.md) and
[compiler-host research](../../research/instant-first-page.md) hold
candidate comparisons. Packaging requirements follow the
[extension process boundary](../integrations/vscode-extension.md#process-boundaries) and
[code-quality gates](../system/code-quality.md).

## Open questions

- **The first page after a restart.** How to show the requested first page
  within the [first-page target](../system/performance.md#measured-interactions)
  without waiting for catalog-size-dependent entry preparation and
  compilation remains open.
