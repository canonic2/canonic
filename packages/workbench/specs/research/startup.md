# Installed startup and runtime preparation

Dated observations and comparison evidence. [Performance](../system/performance.md)
owns requirements and measurement semantics; [research ownership](README.md)
defines this record's evidence limits. Measurements below retain their original
conditions and do not establish acceptance for an untested build or host.

## Installed runtime preparation, 2026-10-23

The installed Acme startup log showed config requests taking about 9.1–9.2
seconds while page indexing waited for Electron preparation. Capture and the
evaluator independently hashed and extracted the same archive; preparation
took about 10.5 seconds, including a lost extraction race. Index work itself
took milliseconds once evaluation was available. Subsequent requests against
that installed server returned 36 pages in 14–19 ms.

Preparation now shares one in-flight operation per cache destination within
the host. The [isolated installed check](../../labs/warm-docs/results/2026-10-23-installed-startup-fix/report.json)
against the selected checkout recorded one extraction, capture/evaluator
callers marked shared, and no cleanup failure. On an empty runtime cache,
preparation still took 7.8 seconds (hash 2.1 seconds, unpack 5.7 seconds);
the first measured config overlapped it and took 6.2 seconds, then reads took
29.2 and 22.5 ms. This fixes duplicate work, not the cold-runtime startup target.
A changed binary can invalidate evaluated index records, so an update can
still expose the first-use preparation cost. These are diagnostic samples,
not a controlled before/after benchmark.

The same run verified the workspace CLI's version-1 JSON envelope is accepted
alongside version 2. Its stopped service remains an actionable lens problem, rather than
an invalid-envelope error; Workbench does not start the service automatically.
