# Highlighting comparison: preflight and interrupted timing attempt

Result: **inconclusive; no adoption recommendation**. All four implementations
ran through the shipping canvas and passed isolated installed-VS-Code and
portable compatibility smokes. The agreed two 20-sample timing sets and third
confirmation set were not completed. Correctness findings and unstable host
conditions prevent a performance verdict. No product implementation changed.

## Evidence and sample counts

| Report | Samples | Outcome |
| --- | ---: | --- |
| [Cold-runtime smoke](cold-runtime-smoke/report.json) | 24 | One per variant/fixture/cache cell; execution, restoration and cleanup passed; role/raster failures recorded separately |
| [Interrupted set 1](interrupted-set1/report.json) | 37 | Actual n=1 or 2 per cell, not 20; extreme load; intentional SIGINT interrupted the last edit check |
| [Shared-runtime smoke](shared-runtime-smoke/report.json) | 14 | Stopped at a highlight.js Overview Markdown-edit timeout; restoration and cleanup passed |
| [Overview diagnostic repeat](overview-repeat/report.json) | 4 | Control and highlight.js, both cache conditions; execution passed; glyph movement recorded |
| [Code-heavy diagnostic](code-heavy-glyphs/report.json) | 8 | All variants, both cache conditions; execution, runtime reuse, restoration and cleanup passed; glyph/raster findings remain |
| [Installed VS Code](vscode/report.json) | 4 variants | Temporary isolated installations; compatibility only |
| [Portable](portable/report.json) | 4 variants × 8 pages | Release-server portable exports; compatibility only |

These are separate conditions, not a pooled benchmark. Every timing report has
`summary.json` and `metrics.md`: one table per metric, actual n, nearest-rank
p50/p95/maximum, and raw numeric observations in `report.json`. A one-sample
quantile is just that observation. The incomplete timing attempt cannot
establish stable medians or tails. Earlier preparation failures and smokes are
retained in the owned temporary directory rather than included in these totals.

Each report identifies its plan snapshot, source hashes, binary, fixtures,
dependencies and runtimes. Fixture hashes were checked for equality across
variants within each report. Text is represented by source digests and lengths;
full generated source, logs, process traces, profiles and routine screenshots
are not committed. `provenance.json` pins the original report and supplemental
traces. The projection asserts that recomputing every numeric aggregate gives
exactly the same values as the original report. Duplicate CPU observations are
deduplicated without changing their observed values. Final parity is in the
group report; supplemental per-sample snapshots precede group comparison.

## Correctness

| Check | 0: Syntect | 1: highlight.js | 2: TextMate | D: plain |
| --- | --- | --- | --- | --- |
| Exact fence/source text; copy; escaping | Passed completed cases | Passed completed cases | Passed completed cases | Passed completed cases |
| Defined language-role/palette corpus | SCSS `$accent` variable missing | Passed | Passed | Diagnostic, no colours |
| Outer code bounds; inherited typography | Passed | Passed | Passed | Passed |
| Neutral raster versus control | Reference | Failed | Failed on Code-heavy | Failed; different span structure |
| Character bounds when spans become plain text | Nonzero deltas | Nonzero deltas | Nonzero deltas | Zero in Code-heavy |
| Retention/eviction; stale/disposed source; isolation; supersession; example error containment | Passed completed cases | Passed completed cases | Passed completed cases | Passed applicable checks |
| Injected highlighting failure and bounded timeout | Passed | Passed | Passed | Not applicable |
| Own/shared source edits | Passed completed cases | One unresolved Markdown-edit timeout; diagnostic repeat passed | Passed completed cases | Passed completed cases |
| Native capture | Valid JPEG through actual bridge; pixel parity not established | Same | Same | Same |
| Standalone, isolated VS Code and portable compatibility | Passed smoke | Passed smoke | Passed smoke | Passed smoke |

The neutral screenshots remove only syntax foreground colours. Code pixels
remain in the comparison, with the declared RGB tolerance and antialias
classification. Non-code differences were zero in the complete 24-cell smoke;
all code-box bounds matched. After antialias classification, highlight.js still
had 7 differing pixels in Overview and 13 in Small/Code-heavy. TextMate had 208
in Code-heavy. Plain code also differed. These are retained failures, not
evidence that colours or span boundaries must be identical.

The later DOM Range diagnostic independently compared every character's bounds
with coloured spans and with exact plain text, after timing and normal-memory
observations. Overview's maximum delta was 0.140625 CSS px in Syntect and 0.1875
in highlight.js, repeated in both cache conditions. For Code-heavy's 137,983
UTF-16 positions, maxima were 0.078125, 0.09375 and 0.109375 CSS px for Syntect,
highlight.js and TextMate, respectively; plain code had zero. Outer bounds did
not move. The mechanism is not isolated, and this diagnostic does not establish
how perceptible the movement is. It does show that unchanged block bounds alone
cannot establish unchanged character geometry, including for the control.

The shared-runtime Markdown-edit failure occurred with one-minute load 81 before
and 60 after. The exact requested fence never appeared within 12 seconds. The
later four-case Overview repeat passed without changing that deadline. This
does not erase the failure or establish its cause. Subsequent timeout diagnostics
record event-stream state, server revision and visible frame bounds.

Portable exports still use Rust prehighlighting in every variant. Their smoke
checks established prehighlighted known blocks, available source and no live
highlight URL; they do not establish complete token-role or native-capture
pixel parity. Installed compatibility used foreground, isolated source overlays
with the release server, not shipping integrations of a selected winner.

## Diagnostic timing observations

The following cold-startup observations come from the same complete 24-cell
smoke, with **n=1 each**, before shared-runtime setup. Units are milliseconds.
They illustrate separate boundaries, not an engine ranking or accepted target.
The full tables include Small, the settled condition, retained/evicted returns,
edits, initialization, DOM work, responsiveness, transfer and memory.

| Fixture | Variant | Selection to visible | Colours complete | Show code readable | Show code colours complete |
| --- | --- | ---: | ---: | ---: | ---: |
| Overview | 0 | 184.9 | 169.2 | 14.7 | 59.8 |
| Overview | 1 | 197.8 | 214.7 | 12.2 | 40.4 |
| Overview | 2 | 180.3 | 245.5 | 10.4 | 105.2 |
| Overview | D | 223.1 | — | 11.8 | — |
| Code-heavy | 0 | 253.7 | 821.0 | 23.8 | 116.7 |
| Code-heavy | 1 | 258.6 | 659.2 | 8.6 | 42.5 |
| Code-heavy | 2 | 254.8 | 1406.0 | 20.2 | 98.1 |
| Code-heavy | D | 269.5 | — | 19.2 | — |

Colour completion is observed independently of the final visible-paint check;
it can precede that check. `visibleAndColouredMs` retains the combined boundary.
The diagnostic D and code-free pages have no successful colour-completion metric.
Worker initialization/tokenization is attributed to new jobs for each action,
not the document's previous job history. Native highlighting's aggregate phase
does not separate regex compilation from tokenization. Settled RSS is the
snapshot after the declared return sequence, not proof of long-term quiescence;
CPU is observed cumulative process time, potentially missing very short-lived
processes between polls. No memory or latency guardrail is established as passed.

## Conditions, deviations and remaining work

Host: macOS 15.8.1 x86_64, Intel Xeon W-2140B, 16 logical CPUs, 64 GiB RAM,
Node 26.12.0, standalone Chrome 155.0.8059.85, bundled Electron 44.2.0. Installed
compatibility used VS Code 1.142.0 / Electron 43.7.7 / Chromium 150.0.7871.250.
Power was not inspected because host policy rejected the query. OS file caches
were uncontrolled. Release binary and uncommitted source hashes are in each
report; the debug server was not used for these comparisons.

The timing attempt reached one-minute load 283 and sustained values above 100.
It was intentionally interrupted and retained. Repeated native-runtime
extraction was found in setup, so later smokes prepared one identical owned
runtime cache before sampling; fresh browser/compiler/highlighting state and
page warm-up/retention behavior remained unchanged. High load recurred despite
confirmed runtime reuse; causation was not established. These conditions must
not be pooled. The new quiet-host admission check was implemented but not used
to start another latency set once the correctness findings were established.

Execution deviation: collection stopped before the agreed sample counts.
Set 2 and the control/leader confirmation set did not run. No comparison against
cross-set control variation, no complete guardrail verdict and no installed
webview latency distribution exist. Thresholds and correctness checks were not
relaxed. The evidence supports keeping adoption undecided, not choosing a faster
engine from partial timings.

Before another timing campaign, resolve or explicitly review the character/raster
finding and reproduce the edit timeout with its connection diagnostics. Then
obtain comparable host conditions and run both full sets and confirmation.
Changing the geometry/parity acceptance rule requires a reviewed plan amendment;
the present observations cannot silently become a new tolerance. The separate
whole-project warm-docs experiment has not started.
