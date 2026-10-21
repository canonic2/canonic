# Docs highlighting lab

Status: full performance campaign running; earlier preflight report retained. The owner agreed
the plan on 2026-10-20. The asynchronous baseline passed a focused
[installed-webview check](../../../../../specs/preview-pipeline/performance.md#installed-asynchronous-highlighting-2026-10-20)
on 2026-10-20. This verifies behavior, not latency or the lab's full correctness
matrix. The final preparation smoke completed all 24 cells without execution or
cleanup failures; role and raster findings remain recorded below. The full
timing sets were not completed. See the [report](#report-2026-10-20). All engine,
grammar, CSS and WASM assets must be pinned and locally installed; no runtime CDN.

Method: [Workbench labs](../../../../../specs/labs.md). Owning contracts:
[docs](../../../../../specs/docs-pages.md), [performance](../../../../../specs/preview-pipeline/performance.md),
[readiness](../../../../../specs/preview-pipeline/readiness.md) and
[rendering flow](../../../../../specs/rendering-flow.md).

## Question and scope

Which highlighting approach gives docs the shortest selection-to-visible and
Show code latency, including first use, without sacrificing readable code,
example/font readiness, correct captures or standalone/export support?

Compare engines with highlighting outside the visibility path. Do not
change docs warm-up, the compiler, frame retention or the loading-message delay.
The separate whole-project warm-up question remains outside this lab.

## Current behavior and hypothesis

The asynchronous implementation returns escaped code in the document and plain
Show code response, then requests colours from the local Rust server. Highlighting
runs on blocking workers without gating page readiness. The canvas still waits
for CSS, examples and fonts before showing the page, as the owner requested.
Captures wait for colouring to finish or fall back to plain text.

The earlier [exploratory probes](../../../../../specs/preview-pipeline/performance.md#highlighting-2026-10-20)
show first-language initialization costs and much cheaper repeat calls. They
precede this plan and are not lab results. Hypothesis: a smaller JS highlighter
may reduce time until code is coloured without regressing first visibility.
Worker, local asset loading, WASM and transport costs may erase those benefits.
Small pages and retained returns may not improve.

## Variants

| ID | Implementation | Visibility and supported subset |
| --- | --- | --- |
| 0 | Current asynchronous Rust server, Syntect 5.3.0/two-face 0.4.5, Oniguruma, unchanged | Plain code first; includes local requests, first-language setup and transfer costs |
| 1 | highlight.js with only supported languages explicitly registered, in a document-owned worker | Plain code first; includes worker/module loading and first-language setup; no language auto-detection |
| 2 | `vscode-textmate` plus `vscode-oniguruma`, pinned language grammars, in a document-owned worker | Plain code first; includes worker, grammar and WASM loading; independent of VS Code internals |
| D | Escaped plain code with highlighting disabled | Diagnostic lower bound only; cannot be adopted |

Variants 1–2 initially cover docs fences and Show code in browser/standalone
Workbench. Portable export and installed-webview confirmation are required before
adoption, not inferred from worker or server probes. Report unsupported aliases,
grammars and token roles as missing contracts, not successes. No installed
extension or production source is modified by the lab.

For every candidate, CSS, examples, fonts and images retain the control's readiness
requirements. Code colours do not gate first visibility. Code text is present
and copyable immediately, and applying colours must not move it. Capture/export
must await highlighting completion or the control's bounded plain-text fallback
(currently a five-second deadline per request). Record successful colouring,
request failure and timeout fallback separately; fallback is never a successful
fully-coloured timing sample. Portable exports remain prehighlighted and require
their own parity check. This preserves the asynchronous product baseline.

## Implementations and delivery

Use the actual docs/source routes and shipping canvas, renderer and examples.
Build temporary server overlays from read-only product source for variants 1–D:
only highlighting delivery and its completion/capture signal may differ. Keep
variant source under this lab, and build/install into the run's temporary folder.
Variant 0 uses the shipping server and locked grammar dependencies unchanged.
Worker/engine startup and first use are measured, not hidden by prewarming.

If a lab gateway is needed, route every variant through it and smoke the control
both directly and through the gateway. Record its overhead separately. Never
implement candidates by replacing already-highlighted control output: that would
leave the original cost on the timed path. Serve worker assets locally, without
a UI build step. Record any lab instrumentation and its effect in the report.

## Fixtures and expected output

Generators use seed `20261020`, neutral Acme/example.com data, pinned dependencies
and identical generated bytes across variants. Record file counts and bytes.

| Fixture | Shape and purpose |
| --- | --- |
| Small | Two short prose pages with one shared HTML examples lens, one example, one local font and one local image; one page has no code and one has a four-line JSON fence. Establish the already-fast case. |
| Overview | Acme Overview Markdown/HTML fence and its TypeScript `tokens-in-use` export, plus a second docs page for navigation. Copy the repository's public Acme sources read-only and hash them. Motivating first-use HTML then TypeScript case. |
| Code-heavy | Two docs pages, each with 20 fences of 160 lines, cycling supported languages; a 1,000-line Show code export; deterministic Unicode, HTML-sensitive text, long lines and multiline strings/comments. Exercise repeated work and responsiveness. |
| Language corpus | Small fixed snippets for every shipping language and alias, including JSX/TSX, embedded markup, unknown/missing languages and malicious-looking HTML. Correctness coverage, not a replacement for canvas timings. |

Freeze expected source text, escaping, language aliases and representative token
roles before the first smoke. Unknown languages remain escaped plain text.
The shipping language/alias matrix is: Bash (`bash`, `sh`, `shell`, `zsh`),
CSS, Diff, JavaScript (`javascript`, `js`, `jsx`, `mjs`, `cjs`), JSON,
Markdown (`markdown`, `md`), SCSS, TypeScript (`typescript`, `ts`, `tsx`, `mts`,
`cts`), markup (`xml`, `html`, `svg`, `vue`, `astro`), and YAML (`yaml`, `yml`).
Preserve the control's distinct JSX/TSX and XML/SVG grammar selection; an alias
does not imply support for all syntax of the framework named by it. Include
missing, unknown, whitespace-padded and mixed-case language names.
Different span boundaries are allowed when they express the same checked token
role; missing or incorrect roles are failures. Do not exclude all code pixels
from parity checks: code geometry/text must match, and checked token roles must
produce the expected palette. Record additional lexical differences for review;
an unresolved difference prevents declaring a candidate correct.

## Sequences, boundaries and cache states

For each page fixture: open the first page; select the second page with a warm
server but a new document; return to a retained first page; evict and revisit it;
open Show code for the first TypeScript use, close and reopen it; edit a fence
and an example/shared module, then restore each. Include rapid supersession,
an example failure and a highlighter failure/timeout.

Measure these distinct boundaries with monotonic clocks and correlated request
and selection IDs:

- Selection to visible: actual selection until the requested generation's text
  and example are painted in the promoted canvas frame, independently observed.
- Document request to response: full route time, plus existing server phases.
- Show code click to readable source, and separately to fully coloured source.
- Selection to all requested code coloured, independently checking its revision.
- Worker/helper/grammar initialization and tokenization, plus transport and DOM
  application; these explain totals, not replace them.
- Edit to visible and edit to coloured for the exact new revision.

Cold cells use a fresh server/helper, empty named compiler caches and a new
browser profile. Separate startup-first selection from a first selection after
examples compilation has settled without touching highlighting. Browser-warm
cells keep server/HTTP/code caches but use a new content document; retained cells
keep the same document. First HTML and first TypeScript grammar use are separate
cells. No whole-set highlighting warm-up. OS file caches are uncontrolled.

## Correctness checks on every run

Check exact code text, escaping, copy output, token-role expectations, document
structure, code geometry and settled screenshots. Compare examples and other
page content at the same viewport, with fixed time/randomness and local assets.
Allow only the declared uncoloured-to-coloured transition in deferred variants.

Verify independent page/worker state, listener and style isolation; fresh fence,
example and shared-module edits; rejection of stale worker responses after edits
or supersession; contained render/highlighter failures; and no new console
warnings/errors. Observe font and image completion independently when the page
appears. Captures must include fully settled highlighting, examples and assets.
Failures/timeouts stay failures, never latency samples. Restore edits in `finally`.
For deliberately injected highlighter failures/timeouts, correctness means exact
plain source remains usable and capture settling completes within the control's
bound, with late colouring rejected. These expected outcomes do not fail the
injection check; unexpected failures in normal cells reject the candidate.

## Metrics, runs and decision rule

Primary metric: time to fully coloured code per fixture/cache cell, for docs
selection and Show code. Selection-to-visible and readable Show code latency
are guardrails: an engine cannot win by slowing readable content. Secondary metrics:
edit latency, long
tasks, loader exposure, transferred/decoded bytes, worker/helper initialization,
server/helper CPU, peak and settled RSS, and browser/worker memory after returns.
Measure named compiler caches separately from dependencies and browser profiles.

Smoke every fixture/group with control and candidate 1, one sample per cell,
then smoke other candidates before including them. Classify runner defects and
findings before the run set. Use two independent run sets, 20 samples per latency
cell per variant, alternating variants in seeded order. Cold cells have no
warmup; warm cells get three declared untimed iterations. Report n, p50, nearest
rank p95 and max, retaining raw samples and partial/failed runs. With 20 samples,
p95 remains exploratory. Repeat the control and leading candidate in a third set.

Proposed decision rule: all correctness checks pass; first-use time to coloured code
improves by both at least 10 ms and 20% in the motivating or code-heavy fixture,
repeats across sets, and exceeds control run-to-run variation. No other latency
cell may regress by more than the larger of 5 ms or 10% at p50. For deferred
variants, coloured-code completion must not regress beyond that bound either.
Additional settled process/worker RSS must stay below the larger of 20 MiB or
20% of control. Report tokenization separately from asset loading, transport and
DOM application; partial winners or missing contracts remain
inconclusive rather than relaxing the rule. These are proposed lab thresholds,
not accepted product targets.

## Host, tools, evidence and authorization

Initial confirmation surface: shipping canvas in bundled Electron or headless
Chrome on this macOS x86_64 host. A result relied on for adoption must subsequently
be confirmed in the installed VS Code webview, including capture and export.
Record OS/CPU/RAM/power/background load, Node/Electron/Chrome versions, pinned Rust
toolchain, all engine/grammar versions, owning Git HEAD and hashes of uncommitted
measured sources/binaries. Pin JS/WASM/grammar packages and integrity hashes in
lab-only lockfiles before any smoke; disclose grammar and theme inputs.

The owner agreed this concrete plan on 2026-10-20 before any run. Network use is limited to public dependency and
grammar acquisition into owned temporary directories, with no paid services or
project transmission. Request host access for loopback/browser/process runs.
Never install dependencies into product manifests or lockfiles. Avoid builds
and heavy suites during measurement; retain and qualify any overlapping run.
Before each installed-webview cell, foreground the actual VS Code process and
independently require `document.visibilityState === 'visible'`, animation-frame
progress, and visibility through every enclosing frame. DevTools attachment or
`Page.bringToFront` alone did not establish this in the baseline verification.
Hidden/background behavior is a separate correctness condition, never a
foreground latency sample. Record tracing state identically for every variant.

Each run owns and cleans up its servers, helpers, workers, browser/profile,
fixtures and caches on success, failure and cancellation, reporting cleanup
failures. Write new dated result folders; refuse overwrite. Retain reviewed,
sanitized raw samples, `report.json` and a summary. Recompute aggregates from raw
samples before reporting. The owner decides adoption separately; agreement to
this plan does not authorize shipping a winner.

## Method amendments and preparation record, 2026-10-20

The owner agreed the plan before preparation smokes. The following refinements
make its implementation and observation boundaries concrete; thresholds, inputs
and required contracts remain as agreed.

- Temporary product overlays copy the shipping modules and release binary.
  Candidates replace only the highlighting module, loading a document-owned
  worker through a lazy local delivery adapter. Portable pages have no live
  highlighting URL, so they do not load that adapter. D disables the highlighter
  API entirely. No Rust rebuild or gateway participates.
- Prebuilt browser assets come from the integrity-pinned lab lockfile. Grammar
  files come from VS Code 1.142.0, pinned by individual SHA-256 in
  `dependencies/assets.json`. They are loaded on demand through the existing
  generic static route; the authored JS/CSS-only module route cannot serve WASM
  or JSON. The TextMate variant reuses the control's scope-to-class rules.
- Every fixture uses the same pinned local font. Acme's external font import is
  replaced in the temporary fixture only. The corpus adds a docs destination;
  five neutral destinations exercise the existing three-idle-document eviction
  bound. File counts, bytes and content hashes are recorded per sample.
- Selection begins after the requested row is selectable, before changing the
  actual shell hash. Readability/visibility stops on the requested document and
  complete visible frame chain at a paint opportunity. Highlight completion is
  observed independently as soon as that revision reports ready; a retained
  document already coloured at selection has zero additional colouring work.
- Three untimed repetitions precede retained returns, evicted/browser-warm
  returns and Show code reopen measurements. First page, second page's first
  use and first Show code use remain separate, without grammar warm-up.
- Process RSS is sampled every 200 ms for the owned host/server/evaluator/helper
  and browser descendants; worker heaps are read after returns without forcing
  garbage collection. Navigation latency and RSS stop before worker inspector
  attachment; the inspector is detached before subsequent sequences. Memory
  instrumentation is identical across variants. RSS peaks are sampled peaks,
  rather than operating-system high-water marks.
- Preparation found and corrected runner defects: relative font URLs needed by
  the compiler, selection before asynchronous config readiness, token-role
  assertions that incorrectly required a single span, and a fault-injection
  navigation that changed only the hash while waiting for a document load.
  Edit restoration must show the exact original example before the next edit.
  Failed/partial smoke reports remain in the owned temporary run directory.
- Native VS Code profiles use a short owned temporary path: macOS rejected its
  IPC socket when the profile was nested under the longer run-folder name.
  Portable overlays include the product license required by the export builder.
- Copy-handler output is observed through a temporary clipboard receiver in the
  owned docs document. Source bytes are checked independently against fixture
  files. Normal and injected-fallback screenshots have separate artifacts.
- Generated fixtures, runtime extractions, compiler caches and browser profiles
  are deleted after each sample's checks, hashes and cache sizes are recorded.
  This keeps the 960-sample two-set matrix within available disk space without
  changing a cache state inside a sample. Raw reports and captures are retained.
- Product files, Node dependencies, bundled runtime, lab delivery/probes and
  fixture templates are frozen once per run set. Variants and samples read those
  copies, so concurrent workspace edits cannot change inputs between cells.
- Supersession holds a never-visited destination's response before choosing its
  replacement; a dedicated neutral page makes that request independent of
  retention state. Canceled interception IDs are recorded and their command
  promises are observed. A generic top-level failure also closes owned resources
  and records the incomplete run.
- Worker common initialization includes local module/WASM setup. TextMate's
  grammar-load duration is separate; highlight.js's internal lazy grammar
  compilation remains inside its first tokenization call. Rust's `highlight`
  server phase combines syntax initialization and tokenization. These internal
  boundaries explain user-facing totals and are not interchangeable benchmarks.
- Code-heavy JavaScript/TypeScript fences include 360-character lines, Unicode,
  multi-line template strings and block comments while retaining 20 × 160 lines.
  The predefined role corpus checks the actual computed palette, not just classes.
- Identical probe wrappers time template parsing and code-node insertion, with
  diagnostics collected after their recorded completion timestamps. Render
  checks require inherited typography and unchanged code bounds. Settled rasters
  compare every code and non-code pixel within two RGB levels after a separate
  browser render normalizes only syntax-span foreground colour to its inherited
  code colour. Normal coloured rasters and semantic palette checks are retained.
  This compares code pixels rather than masking them out. The rightmost 16 viewport
  pixels are excluded for native scrollbar raster. Copy feedback is reset before
  capture, so its timer cannot create an unrelated raster difference.
  An initial algebraic foreground-to-coverage estimate failed its own plain-text
  diagnostic despite identical text, typography and geometry; it is retained as
  a runner defect, not a candidate rendering finding. Browser-rendered colour
  normalization replaces that invalid inference without relaxing glyph geometry.

For the decision rule, control run-to-run variation is the range of control p50s
across the independent run sets (including confirmation when performed). A
candidate must exceed that range as well as the agreed absolute/relative gain.
Every measured latency cell is a guardrail; code-free docs and D have no
successful fully-coloured metric. Missing correctness or memory evidence cannot
be treated as a passing result.

The predefined SCSS variable check finds that the control leaves `$accent`
uncoloured. This is a control finding, not a runner defect; the check remains.
Corpus role failures are recorded independently of runner execution success and
make the sample ineligible for a successful correctness/performance verdict.
Preparation has also observed high and variable host load with concurrent Cargo
tests. Smoke timings are diagnostic only; load and overlaps qualify run sets.

Preparation amendment, 2026-10-20: neutral screenshot comparisons additionally
record pixelmatch 7.1.0's antialias classification (threshold zero, includeAA
false), only within code; non-code retains the RGB channel tolerance of two.
The Small smoke still has 13 differing code pixels for highlight.js and 141 for
plain code after this classification; TextMate has none. Text, typography and
code bounds match, and non-code differences are zero. These remain unresolved
rendering findings, not passing parity checks. Complete the remaining cells and
timing sets with these failures recorded; do not infer adoption eligibility from
execution success or relax the decision rule.

Large per-sample server logs and full-lifecycle process traces are retained in
each sample's `sample.json`. The atomic `report.json` retains timing inputs,
normal-memory samples and final group parity, and references those supplemental
raw traces. This bounds report rewrites during the 480-sample sets without
discarding evidence or changing a measurement boundary.

Host-condition amendment, 2026-10-20: the first timing attempt was interrupted
after 37 samples when one-minute load reached 283 and remained above 100 on 16
logical CPUs. Its last edit check was interrupted by the runner's SIGINT and is
not an engine failure. Retain the partial report; it cannot establish stable
engine costs. Repeated full Electron extraction was found in the sample setup;
its contribution to macOS background activity is unproven.

Subsequent sets extract the identical pinned native runtime once into an owned,
shared run-set cache, before samples. Record extraction separately. Every sample
still starts fresh evaluator/helper processes, browser profile, compiler/index
caches and highlighting state. This models an installed runtime; page warmups,
whole-project docs warming and retention are unchanged. Never combine this
condition with the interrupted cold-runtime attempt. Before timing, require
one-minute host load no higher than the logical CPU count continuously for 60
seconds, waiting at most ten minutes. Retain admission observations and an
incomplete report if that condition cannot be obtained. Host admission is not
product warm-up and does not start a server, browser or grammar.

To investigate the unresolved code raster differences, collect per-character
DOM Range bounds before and after replacing spans with exact plain text, after
all latency and normal-memory observations. Record every nonzero delta's count,
maximum and bounded examples. This is an additional diagnostic; block geometry,
source, typography, palette and raster checks retain their existing rules.

The first shared-runtime smoke stopped after 14 samples: highlight.js in the
Overview/settled cell did not display a Markdown edit within the unchanged
12-second check, with one-minute load 81 before and 60 after. Restoration and
cleanup passed. This is an unresolved correctness finding, distinct from the
earlier intentional cancellation. Add event-stream state, current server revision
and frame visibility to any subsequent edit-timeout diagnostics; retain the
original failure and do not lengthen its deadline.

Worker metric arrays are document history, not per-action deltas. Attribute
jobs by their absolute start in subsequent runs. For the retained reports,
subtract the preceding observed IDs in the same document: first docs before
first Show code, first Show code before cached reopen and retained return.
Fresh/evicted documents use their own complete history. This corrects secondary
phase attribution; primary latency timestamps and resource-start filtering are
unchanged. Do not charge a previous initialization to a cached action.

## Report, 2026-10-20

Continuation, 2026-10-21 UTC: the owner explicitly requested the performance
tests despite the preflight findings. Complete both 20-sample sets and repeat
the control and performance leader. Keep correctness/adoption eligibility
separate from latency observations. An edit/capture failure after independently
validated navigation measurements does not erase those completed timings;
failed/timed-out actions themselves remain failures, not latency samples.
Keep the quiet-host admission wait, but if ten minutes elapse, collect the
authorized matrix with an explicit host qualification rather than stopping.
Record all samples and load without selecting a quieter subset. All existing
metrics, warmups, fixtures, retention behavior and decision thresholds remain.
For confirmation, select the worker with the lowest mean of the four Code-heavy
first-colour p50s (two cache conditions × two sets), solely to choose which
prototype to repeat; this does not override any guardrail or correctness check.
The durable campaign queue runs analysis between sets and reuses the first
set's frozen product, dependencies and fixture bytes for later sets. Product
source metadata retains that origin; current harness hashes are recorded
separately. Reconstructed fixture manifests must match the frozen file hashes.
Performance evidence stores unrounded per-trial metric observations and asserts
that their aggregates equal the full raw report. Full traces remain in the
owned temporary run with content hashes. D has no colour-completion metric,
including for edits; its plain settling time is not successful colouring.
Analysis retains independently completed visibility/readability observations
even if later colouring falls back. Only nonempty highlighted blocks/source
count as successful colour completion; fallback outcomes and settling times
remain explicit, so successful-only tails do not hide failures. The existing
edit probe observes exact updated DOM and its highlight settling promise;
publish that secondary boundary as `highlightSettledMs`, since it does not
independently prove a coloured edit or a subsequent painted frame. This is a
boundary-label correction, not a new winning metric or acceptance threshold.

The [reviewed evidence and findings](../../2026-10-20-preflight/README.md)
cover 87 canvas samples across separate smokes, diagnostic repeats and an
interrupted timing attempt, plus four installed-VS-Code and four portable
compatibility checks. All four variants ran. No production implementation was
changed and no engine was selected.

Worker candidates passed the defined language-role corpus; Syntect left the
SCSS variable uncoloured. Outer code bounds and source text matched, but neutral
raster differences remained. A later character-range diagnostic found subpixel
bound differences when spans were applied in all three highlighters, including
the control. One highlight.js Overview Markdown edit timed out; a bounded
diagnostic repeat passed without erasing that unresolved failure.

The first timing attempt stopped at 37 samples under extreme host load. Shared
runtime-cache smokes confirmed reuse but did not eliminate load spikes. Collection
stopped on the correctness findings before restarting full timing sets: neither
the two independent 20-sample sets nor the third confirmation was completed.
Single-sample values suggest different colouring costs, but establish no latency,
memory or cross-set-variation verdict. Thresholds remain unchanged. The separate
whole-project docs warm-up lab has not run.

The result is inconclusive under the agreed rule. Resolve or explicitly review
the geometry/raster finding and reproduce the edit timeout before completing the
timing comparison on a comparable host. Changing acceptance criteria requires a
reviewed amendment; these results do not establish a new tolerance.

## Reproduction

Use a prepared release Workbench with its pinned packaged runtime, Chrome and
the exact VS Code grammar/font inputs in `dependencies/assets.json`.
`prepare.cjs` verifies those pins. Product sources and dependencies are read-only;
lab dependencies belong only to the owned temporary root.

From the repository root:

```sh
lab_root="$(mktemp -d /private/tmp/workbench-highlighting-lab.XXXXXX)"
cp packages/workbench/labs/docs-highlighting/dependencies/package.json "$lab_root/"
cp packages/workbench/labs/docs-highlighting/dependencies/pnpm-lock.yaml "$lab_root/"
pnpm --dir "$lab_root" install --frozen-lockfile --ignore-scripts --store-dir "$lab_root/pnpm-store"
node packages/workbench/labs/docs-highlighting/prepare.cjs "$lab_root"
node packages/workbench/labs/docs-highlighting/run.cjs "$lab_root" smoke
node packages/workbench/labs/docs-highlighting/compat.cjs "$lab_root"
node packages/workbench/labs/docs-highlighting/installed.cjs "$lab_root"
```

After the outstanding findings are handled, `run.cjs <root> set1` and `set2`
collect 20 samples per cell and apply the documented host-admission check.
`run.cjs <root> confirmation 0,<leader>` repeats the control and selected leader.
`summarize.cjs <run-dir>/report.json` recomputes all aggregates. Focused reporting
checks use `node --test packages/workbench/labs/docs-highlighting/verify-report.cjs`.
Host access is needed for servers, browsers, VS Code and process cleanup.
