# Asynchronous highlight.js integration

The owner selected highlight.js after the [performance campaign](../2026-10-21-performance/README.md).
Live docs use a document-owned worker with pinned local highlight.js 11.11.1 and
ten explicit grammars. Worker creation waits for document load and two animation
frames. CSS Custom Highlight ranges paint the original text nodes. Capture mirrors
transfer those ranges as root state; portable exports retain server-prehighlighted HTML.
Warm-up, frame retention and readiness blockers are unchanged.

[Check metadata and source hashes](checks.json) record compatibility checks on
2026-10-21 using Workbench 0.14.0, VS Code 1.142.0, Electron 43.7.7 and Chromium
150.0.7871.250 on macOS x86_64. These used temporary installed extension copies in
isolated VS Code profiles with the release server, not the user's normal profile.

| Installed fixture | Fences | Code characters checked | Colour ranges | Character geometry | Native capture |
| --- | ---: | ---: | ---: | --- | --- |
| Overview | 1 | 1,402 | 68 | Identical with ranges enabled/disabled | 98,387-byte JPEG |
| Code-heavy | 20 | 137,983 | 12,011 | Identical with ranges enabled/disabled | 60,591-byte JPEG |

Both docs and Show code coloured, and fonts were loaded. Three setup repeats
timed out before selection with a hidden canvas; explicitly activating the
isolated Workbench editor tab resolved this. The canvas was visible for selection;
the final visibility observation after native capture was hidden. These runs do
not establish installed-webview latency or a raster comparison of captured colours.

Focused browser tests verify readable docs and Show code while the worker is
held; capture completion waits; exact Unicode/source/copy text; identical
per-character positions and text-node identity; language aliases and unknown
languages; stale responses, disposal, cancellation, timeout, worker failure and
restart; and local asset integrity. A real mirror receiver verifies transferred
range text, computed colours, retained nodes and clearing, including colour-only
revision changes. Capture sequencing and native browser module checks also pass
(21 focused tests). The final highlighting/mirror rerun passes separately.

The shipping standalone canvas also passes the integrated lab sequences once
each for Small, Overview and Code-heavy with fresh server/browser processes:
first/second selection, source/reopen, retained return, forced eviction/return,
exact source and copy, independent document state, in-flight supersession,
Markdown/example/shared-import edits, example failure containment/recovery and
native capture. The three source/retained/eviction warm-up cycles are preserved.
Every generated fixture was restored byte for byte; no cleanup failures occurred.
These are startup-path compatibility sequences, not repeated latency samples or
the separate compiled-first cache matrix.

A release-server portable export check loaded nine generated docs pages and all
nine prehighlighted Show code sources, with loaded fonts, no live highlighting
URL, no runtime errors and no external requests. Exported plain/unknown-language
fences remain plain. Portable semantic/raster parity limitations from the lab
remain recorded; this check does not redefine them.

The full performance matrix was not rerun for the integrated range painter.
Prototype latency, host-load qualifications and guardrail failures remain in
the performance report. No new performance target or tolerance is inferred from
these compatibility checks.
