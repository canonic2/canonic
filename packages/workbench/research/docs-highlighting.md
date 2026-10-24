# Docs highlighting

## Question

Which highlighting approach gives docs the shortest selection-to-visible and
Show code latency, including first use, without sacrificing readable code,
example/font readiness, correct captures or standalone/export support?
Highlighting stays outside the visibility path; docs warm-up, the compiler,
frame retention and the loading-message delay are not changed.

## Status

Decided, 2026-10-21. The comparison recommended keeping asynchronous Syntect:
neither worker prototype met the overall performance rule. The owner selected
asynchronous highlight.js, accepting its dense-colouring gain despite the
prototype's visibility and small-page regressions. The integrated path defers
worker creation until after document load and paints CSS Highlight ranges over
unchanged text; integration and installed checks pass, but prototype timings
do not measure it. All assets ship locally, with no runtime CDN.

## Hypothesis

A smaller JavaScript highlighter in a document-owned worker may reduce the time
until code is coloured without regressing first visibility. Worker, local
asset loading, WASM and transport costs may erase those benefits; small pages
and retained returns may not improve.

Exploratory evidence before the plan (2026-10-20), under the earlier
synchronous Syntect path:

- HTML and TypeScript waits came mainly from first-use grammar regex
  compilation. An installed Acme trace recorded 43 ms for an HTML fence, then
  448 ms for the example's TypeScript.
- Optimized probes of Syntect 5.3.0 with two-face 0.4.5, ten fresh processes
  per backend, one first use and eleven repeats per input:

  | Input | fancy first-use p50 | Oniguruma first-use p50 | fancy warm p50 | Oniguruma warm p50 |
  | --- | ---: | ---: | ---: | ---: |
  | HTML fence | 28.30 ms | 4.39 ms | 0.308 ms | 0.209 ms |
  | TypeScript example, after HTML | 368.68 ms | 20.65 ms | 0.837 ms | 0.293 ms |

  Parsing accounted for almost all the difference; scope classification took
  0.03–0.08 ms.
- Release servers (three fresh processes per backend, Acme's Overview only):

  | Request | fancy cold p50 | Oniguruma cold p50 | fancy warm p50 | Oniguruma warm p50 |
  | --- | ---: | ---: | ---: | ---: |
  | Overview document | 57.17 ms | 9.18 ms | 2.49 ms | 1.23 ms |
  | TypeScript Show code | 1174.28 ms | 33.62 ms | 4.17 ms | 1.94 ms |

  A 31-input comparison found all 18 Acme inputs byte-identical between
  backends; two synthetic JavaScript/JSX snippets differed in token classes.
  The server moved to Oniguruma and highlighting became asynchronous: escaped
  code first, colours applied without gating readiness, captures waiting for
  colours or a bounded plain fallback.
- A trace smoke on 2026-10-20 (debug server, headless Chrome) measured single
  selection-to-visible samples of 556, 182 and 28 ms for first page, second
  page and retained return, with 333 of the first document's 335 server
  milliseconds in highlighting.

## Method

**Variants.**

| ID | Implementation | Visibility and subset |
| --- | --- | --- |
| 0 | Asynchronous Rust server, Syntect 5.3.0/two-face 0.4.5, Oniguruma, unchanged | Plain code first; includes local requests, first-language setup and transfer |
| 1 | highlight.js with supported languages registered explicitly, in a document-owned worker | Plain code first; includes worker/module loading; no auto-detection |
| 2 | `vscode-textmate` plus `vscode-oniguruma`, pinned grammars, in a document-owned worker | Plain code first; includes worker, grammar and WASM loading |
| D | Escaped plain code, highlighting disabled | Diagnostic lower bound only |

Temporary overlays copy the shipping modules and release binary; candidates
replace only the highlighting module, loading a worker through a lazy local
delivery adapter. Grammar files come from VS Code 1.142.0, pinned by SHA-256;
JavaScript and WASM packages are pinned in a lab lockfile. Portable exports
stay server-prehighlighted in every variant.

**Fixtures** (seed `20261020`, Acme/example.com, one pinned local font):

| Fixture | Shape |
| --- | --- |
| Small | Two short pages with one shared HTML examples lens; one page without code and one with a four-line JSON fence |
| Overview | Acme's Overview Markdown/HTML fence and its TypeScript `tokens-in-use` export, plus a second page; the motivating first-use HTML-then-TypeScript case |
| Code-heavy | Two pages, each with 20 fences of 160 lines across supported languages, 360-character lines, Unicode, multi-line strings and comments; a 1,000-line Show code export |
| Language corpus | Snippets for every shipping language and alias, unknown/missing languages and malicious-looking HTML |

The shipping language matrix is Bash (`bash`, `sh`, `shell`, `zsh`), CSS,
Diff, JavaScript (`javascript`, `js`, `jsx`, `mjs`, `cjs`), JSON, Markdown
(`markdown`, `md`), SCSS, TypeScript (`typescript`, `ts`, `tsx`, `mts`,
`cts`), markup (`xml`, `html`, `svg`, `vue`, `astro`) and YAML (`yaml`,
`yml`); unknown languages stay escaped plain text.

**Sequences.** Per fixture: open the first page, select the second page in a
new document, return to a retained page, evict and revisit, open Show code for
the first TypeScript use and reopen it, edit a fence and an example/shared
module and restore each, rapid supersession, an example failure and a
highlighter failure/timeout.

**Boundaries.** Selection to visible (the requested generation's text and
example painted in the promoted frame, independently observed); document
request to response; Show code click to readable and to fully coloured;
selection to all requested code coloured; worker, grammar and tokenization
phases; edit to visible and to coloured. Cold cells use a fresh server, empty
compiler caches and a new browser profile; startup-first and compiled-first
selections are separate cells; first HTML and first TypeScript grammar use are
separate; no highlighting warm-up.

**Correctness checks**, every run: exact code text, escaping and copy output;
token-role expectations (span boundaries may differ, roles may not); document
structure, code geometry and settled screenshots; isolation; freshness and
stale-response rejection; contained failures; no new console messages;
independent font and image completion; captures with settled highlighting.
Injected failures must leave exact plain source usable and settle capture
within the control's bound.

**Metrics and runs.** Primary: time to fully coloured code per fixture and
cache cell, for docs selection and Show code; selection-to-visible and
readable Show code are guardrails. Two independent sets of 20 samples per
latency cell per variant, alternated in a seeded order, then a third set
repeating control and the leader.

**Decision rule** (proposed thresholds). All correctness checks pass;
first-use time to coloured code improves by at least 10 ms and 20% in the
motivating or code-heavy fixture, repeats across sets and exceeds control
run-to-run variation; no other latency cell regresses by more than the larger
of 5 ms or 10% at p50; settled process/worker RSS grows less than the larger
of 20 MiB or 20%. Partial winners are inconclusive.

**Amendments.** Before the run (2026-10-20): runner defects were corrected
(relative font URLs, selection before config readiness, token-role assertions
requiring a single span, a fault-injection navigation that changed only the
hash); VS Code profiles use a short owned path because macOS rejected a nested
IPC socket; raster comparisons normalize only syntax-span foreground colour in
a separate render and add antialias classification within code; a
character-range diagnostic compares per-character bounds with spans and with
plain text. After the first timing attempt reached load 283, the native
runtime is extracted once per set and each set waits for a quiet host (load no
higher than the logical CPU count for 60 s, up to ten minutes). On 2026-10-21
the owner asked for the performance sets despite the preflight findings; the
quiet-host wait stays, but after ten minutes the matrix runs with an explicit
host qualification, and correctness eligibility stays separate from latency.

## Conditions

Release binary (`5663597d8c69…`), tracing enabled identically, Node 26.12.0,
standalone Chrome 155.0.8059.85 on macOS x86_64 (16 logical CPUs, 64 GiB).
Installed checks used VS Code 1.142.0 / Electron 43.7.7 / Chromium
150.0.7871.250 and Workbench 0.14.0 in isolated temporary profiles. Product and
fixture hashes match across sets; later sets reuse the first set's frozen
inputs. All three performance sets passed the quiet admission; subsequent
one-minute load ranged 5.1–381.6, 6.9–156.6 and 4.3–50.2 on 16 logical CPUs.
Power state was not inspected (host policy disallowed the query).

## Results

### Preflight, 2026-10-20

87 canvas samples across smokes, diagnostic repeats and an interrupted timing
attempt, plus four installed-VS-Code and four portable compatibility checks.
All four variants ran through the shipping canvas and passed the installed and
portable smokes. The timing attempt stopped after 37 samples at load 283; a
shared-runtime smoke stopped on a highlight.js Overview Markdown-edit timeout
(load 60–81). Single-sample cold-startup observations, which establish no
ranking:

| Fixture | Variant | Selection to visible | Colours complete | Show code readable | Show code coloured |
| --- | --- | ---: | ---: | ---: | ---: |
| Overview | 0 | 184.9 | 169.2 | 14.7 | 59.8 |
| Overview | 1 | 197.8 | 214.7 | 12.2 | 40.4 |
| Overview | 2 | 180.3 | 245.5 | 10.4 | 105.2 |
| Code-heavy | 0 | 253.7 | 821.0 | 23.8 | 116.7 |
| Code-heavy | 1 | 258.6 | 659.2 | 8.6 | 42.5 |
| Code-heavy | 2 | 254.8 | 1406.0 | 20.2 | 98.1 |

The preflight alone was inconclusive.

### Performance campaign, 2026-10-21

1,200 attempted trials: two 480-trial sets and a 240-trial control/highlight.js
confirmation, 20 attempts per variant, fixture and cache cell. Aggregates were
recomputed independently from raw observations. Nearest-rank p50 in
milliseconds.

Cold-startup Code-heavy:

| Set | Syntect coloured | highlight.js coloured | Gain | TextMate coloured | Syntect second page visible | highlight.js second page visible |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| set 1 | 874.1 | 649.8 | 25.7% | 1670.3 | 230.6 | 407.9 |
| set 2 | 910.2 | 671.9 | 26.2% | 1769.5 | 248.5 | 414.7 |
| confirmation | 812.0 | 633.6 | 22.0% | — | 212.2 | 402.2 |

Control first-colour p50 variation was 98.2 ms (startup) and 85.6 ms
(compiled-first); highlight.js gains were 177.0–238.3 ms across those six
cells. Initial dense-page visibility was similar (254.1 / 251.0 ms in
confirmation). First Show code was readable at 18.3 / 19.8 ms and coloured at
109.3 / 50.2 ms. Overview first colouring went from 174.7 to 209.2 ms and
Small's second-page JSON colouring from 99.7 to 115.1 ms in confirmation, with
the same regressions in the full sets. Retained visibility stayed about
45–49 ms; evicted returns about 132–149 ms for every variant.

Phases (set 2, cold dense cell, p50): the native highlight phase 339.1 ms with
a 346.7 ms request; highlight.js initialization/tokenization 28.8/93.6 ms and
request-to-reply 132.4 ms; TextMate initialization/grammar/tokenization
21.7/92.3/904.6 ms and request-to-reply 1021.6 ms. Dense first-page
main-thread long-task total p50 was 0/0/169 ms for Syntect/highlight.js/
TextMate.

Memory: highlight.js stayed within the settled combined-RSS allowance in all
18 measured cells; TextMate exceeded it in set 2's cold Code-heavy cell
(+407.5 MiB, 20.95% against 20%).

Execution: six failures (five TextMate trials and one Syntect fault-injection
check); confirmation had none. All sets restored every fixture and reported no
cleanup failures.

### Integrated asynchronous highlight.js, 2026-10-21

Live docs use a document-owned worker with pinned local highlight.js 11.11.1
and ten explicit grammars, created after document load and two animation
frames; CSS Custom Highlight ranges paint the original text nodes; capture
mirrors transfer the ranges; portable exports keep server-prehighlighted HTML.

| Installed fixture | Fences | Code characters | Colour ranges | Character geometry | Native capture |
| --- | ---: | ---: | ---: | --- | --- |
| Overview | 1 | 1,402 | 68 | Identical with ranges on/off | captured |
| Code-heavy | 20 | 137,983 | 12,011 | Identical with ranges on/off | captured |

Focused browser tests cover readable docs and Show code while the worker is
held, capture waits, exact text and per-character positions, aliases, stale
responses, cancellation, timeout, worker failure and restart, and the capture
mirror. The standalone canvas passes the lab sequences once each for all three
fixtures. A nine-page portable export loaded every prehighlighted page with no
live highlighting URL and no external requests. Three hidden-canvas setup
timeouts were resolved by activating the editor tab. The full performance
matrix was not rerun for the integrated painter.

### Installed asynchronous path, 2026-10-20

In installed Workbench 0.14.0 on VS Code 1.142.0, with the colouring responses
held through DevTools, Overview became ready and visible with plain fence text,
loaded fonts and its mounted example, and Show code displayed plain
TypeScript; releasing the responses applied colours without moving code
bounds. A temporary VS Code process stayed `hidden` in the background with no
animation frames until its native window was focused; `Page.bringToFront` did
not establish visibility.

## Correctness

| Check | Syntect | highlight.js | TextMate | Plain D |
| --- | --- | --- | --- | --- |
| Exact text, copy, escaping | passed | passed | passed (1 unobserved) | passed |
| Language-role corpus | SCSS `$accent` variable uncoloured in every trial | passed | passed (3 unobserved) | diagnostic |
| Outer code bounds, typography | passed | passed | passed | passed |
| Neutral raster against control | reference | failed (359 of 360) | 155 passed, 80 failed | differing |
| Character bounds, spans against plain text | deltas up to 0.140625 px | up to 0.1875 px | up to 0.171875 px | zero |
| Retention, supersession, isolation, containment | passed | passed | passed | passed |
| Own/shared edits | passed | one unresolved preflight Markdown-edit timeout; campaign trials passed | passed | passed |

Unchanged block bounds do not establish unchanged character geometry,
including for the control. The edit timeout and the geometry/raster findings
remain recorded; they independently prevent claiming a fully correct
replacement, and the performance rejection does not depend on them.

## Threats to validity

Host load varied substantially, including after quiet admission; p95 at n=20
is exploratory. These are release-server and standalone-Chrome measurements,
not installed-webview latency. Prototype timings do not measure the integrated
range painter. Single-sample preflight values establish nothing.

## Decision and follow-ups

- Neither worker prototype meets the overall rule. highlight.js improves dense
  first-page colouring by 22–26% beyond control variation in every set, but
  slows dense second-page visibility and smaller-page colouring beyond the
  allowance. TextMate is slower on the motivating and dense cases and missed
  the RSS allowance once. The comparison recommended keeping asynchronous
  Syntect.
- Owner decision, 2026-10-21: adopt asynchronous highlight.js, with deferred
  worker creation and CSS Highlight ranges. The decision does not relax
  correctness requirements or performance thresholds.
- Installed-webview latency of the integrated path remains unmeasured.
- Whole-project docs warm-up remains a separate question. Code-path inspection
  (2026-10-20) found `docs_bundle()` waits for the compiler's `warmed()`
  barrier and `Space::warm_docs` requests every docs lens sequentially; the
  readiness observer retries at 50 ms and loading feedback is deferred 100 ms.
  A comparison of shipping warm-up against on-demand Markdown plus only the
  selected page's example bundle has not run.
