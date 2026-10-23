# Rspack pipeline lab

Methodology: [Workbench labs](../../specs/system/labs.md). Owning contracts:
[performance](../../specs/system/performance.md) for targets and edit behavior,
[TypeScript previews](../../specs/pages/previews.md) for compilation, adapters and
readiness, [docs](../../specs/pages/docs-pages.md) for docs pages,
[core](../../specs/core.md) for frames, [capture](../../specs/review/capture.md) and
[export](../../specs/review/export.md) for the other compiler consumers.

Status: the owner agreed the plan on 2026-10-12 and asked for stage 1. Stage 1
ran the same day: the pipeline is feasible with the changes in the
[stage 1 report](#stage-1-report). The owner accepted that result on
2026-10-12, with the fixed entry set. Stage 2 ran two sets on 2026-10-13
([report](#stage-2-report)). The owner decided the same day to implement the
pipeline with the warm document, its state changes re-rendering in place, and
to settle the font wait in a later lab. The implementation is in the product;
the confirmation run through the shipping server and the VS Code webview is
next. The target pipeline is
specified in [preview pipeline](../../specs/preview-pipeline/README.md).

Amendments, 2026-10-12, during stage 1, each from a stage 1 finding:

- **Fixed entry set instead of lazy compilation.** Every discovered preview
  and docs lens is declared as an entry before the first build, and all of
  them are built. Adding entries while pages were open silently dropped hot
  updates (finding 4). Rspack's lazy compilation was tried as an option and
  is not yet reliable in the prototype (finding 9). Stage 2's D1 control
  becomes the default, and lazy compilation is a separate question.
- **Hidden frames apply hot updates as they arrive**, instead of holding them
  until shown. Hidden frames now render behind the visible one, and no
  console errors resulted. Their cost is measured in stage 2.
- **Vue style blocks use `style-loader` and `css-loader`** (finding 7), pinned
  in the lab's tools.
- **Astro islands are not covered.** Workbench rejects them today, so the
  formats fixture has none.

Amendments, 2026-10-12, after stage 1 was accepted and before any stage 2
sample, from building the stage 2 runner (see [stage 2 method](#stage-2-method)):

- **D1 is dropped.** The fixed entry set is the pipeline's default, so D1 and
  variant 2 are the same.
- **Cells are limited to the variants that differ in them.**
  - Start sequences (A1–A5) run variants 0, 2 and D2. Variant 3 shares
    variant 2's compiler and first page. Variants 1 and D3 differ from 0
    and 2 only after mount, and N2 measures that.
  - Edits run variants 0, 2 and 3.
  - Variant 1 and D3 run the navigation sequences.
  - On the huge fixture, navigation runs variants 0, 2 and 3.
- **The added latency runs on the web-font fixture only**, for N2 and N8.
  The other fixtures load no web fonts and only small SVG images.
- **Render parity compares DOM structure and text, not screenshots.** It
  compares each element's tag sequence, the text and the image count against
  the control. CSS module class names and asset URLs differ by compiler by
  design. Screenshots through the capture helper (F2) are not taken.
  Readiness is checked directly instead: when a page becomes visible, its
  document's fonts have loaded and its visible images are complete.
- **The sidebar bundle (F4) is not run.** It does not change in the
  pipeline: stage 1's finding 11 keeps it on esbuild, and its own test
  passed.
- **Variants alternate per group, not per sample.** Each group runs its
  variants one after another in a seeded order that differs per fixture and
  per run set. Most cells need one server session per variant, so
  alternating sample by sample would add a server start to every sample.

Amendment, 2026-10-13, between run sets 1 and 2: two runner defects found in
set 1 are fixed for set 2. Neither changes a timed boundary or a sample.

- The undo of the formats definition edit was checked with a substring of
  the edited text, so it passed at once. Set 1's undo samples for that edit
  are invalid; its apply samples stand. Text checks are exact when one value
  contains the other.
- The freshness check compared the live page with a fresh copy whose mocked
  data was still loading. That caused set 1's failure for variant 3 on the
  large fixture. The fresh copy now waits for its data, and a failure records
  where the two pages differ. A rerun of variants 0 and 3 passed.

## Plan

### Question

Can the proposed preview pipeline deliver every part of the performance
theory in one working implementation, on every native format, without
breaking an existing contract? The pipeline is:

1. Rspack as the preview compiler, with one development compiler per project.
2. Hot updates for HTML, CSS, JavaScript, React, Vue, Astro and docs pages.
3. One build per project with shared, content-hashed chunks, and lazy
   compilation, so that only opened previews are built.
4. Edits pushed from the server instead of polled.
5. A warm frame or document for the next page.
6. Workbench rendering pre-warmed when a project starts.
7. Workbench's plugins, and the project plugin contract, rewritten for
   Rspack.
8. Readiness that waits only for fonts and images that are still loading,
   in a frame that the browser renders.

The answer decides whether this pipeline is built into the product as
planned, or which parts change first. It also decides between a warm spare
frame and a warm document per project.

### Evidence so far

- [Compiler speed lab](../compiler-speed/README.md):
  - Rspack builds the large fixture 21–27% faster than esbuild from cold, and
    rebuilds it 26–71% faster.
  - Its HMR and React Fast Refresh can be driven from code.
  - Its development output is about three times esbuild's size.
- [Preview loading lab](../preview-loading/README.md), run set 1:
  - Shared chunks cut showing a new preview from 1.4 s to 0.13–0.2 s.
  - Rspack with HMR showed a new preview in 0.34 s and kept React state on
    every edit.
  - Rspack logged exceptions in hidden retained frames.
  - That variant's slow compile error came from the lab's own code.
- A probe on 2026-10-12, in headless Chrome against the shipping server, of
  a page that loads no web fonts, with the compile cache warm:
  - Selection to ready took about 600 ms.
  - About 300 ms went to loading the page's 5.2 MB bundle.
  - About 130 ms went to the font wait.
  - 130–215 ms went to the image wait, for one SVG and one small JPEG.
  - The runtime logs each phase's duration on the event that ends it, so
    `fonts.waiting` carries the time before the font wait
    ([gap](../../specs/pages/previews.md#implementation-gaps)).
  - The probe suggests that previews mounting in a hidden spare frame wait
    for rendering updates they rarely get. This is not established.

### Hypothesis

- The pipeline meets the proposed targets in [performance](../../specs/system/performance.md#measured-interactions)
  on the large fixtures.
- Selecting a new page costs only that page's own chunks, its mount and any
  font or image it actually loads. Shared code, global stylesheets and
  loaded fonts stay warm.
- Every format gets the edit behavior the requirement asks for. Astro is the
  most likely to fall short, because its components render on the server.
- Readiness fixes alone recover a large part of today's wait, independently
  of Rspack.
- A warm document is faster than a warm spare frame for back-to-back
  navigation. Whether it can keep previews isolated is not established.

### Variants

| # | Variant | Supported subset and missing contracts |
| --- | --- | --- |
| 0 | **Control.** Shipping behavior | Full |
| 1 | **Readiness fixes only.** The control, with readiness waiting only for fonts in `loading` status and images not yet complete, phase durations logged on the right events, and previews mounting in a frame the browser renders until promotion | Full |
| 2 | **Pipeline, warm spare.** Items 1 to 8 of the question. One prepared spare frame per project and adapter has loaded the runtime, the shared chunks, global stylesheets and fonts. A selection takes the spare, and the next spare starts preparing | Every native format. The lab lists each contract it cannot meet |
| 3 | **Pipeline, warm document.** Variant 2, but one long-lived document per project and adapter changes page in place, like an application's router. Between pages it restores a baseline and loads the next preview's project modules fresh, keeping dependencies, stylesheets and fonts | As variant 2. Eligible only if it passes every isolation check |

In variant 3, the baseline restore removes everything a preview added after
the baseline:

- stylesheets;
- window and document listeners;
- timers, intervals and animation frames;
- body children outside the preview root;
- `<html>` and `<body>` attributes and classes;
- `window` globals;
- request mock handlers.

It drops the previous preview's project modules from Rspack's module cache.

Diagnostic controls, not candidates:

- **D1:** variant 2 without lazy compilation, building every preview at
  start. It measures what lazy compilation saves at each project size.
- **D2:** variant 2 without pre-warming. It measures what pre-warming saves
  and costs.
- **D3:** variant 2 with readiness marked immediately after mount. It
  measures the remaining readiness cost, to inform the open question of
  showing a page before fonts and images load.

### What the prototype implements

The prototype implements the pipeline across Workbench's compile worker,
server routes, preview runtime and canvas. It lives in the lab, as overlays
on a snapshot of the package, following the preview loading lab's mechanism.

| Plan item | Prototype |
| --- | --- |
| Rspack compiler | One Rspack development compiler per project and adapter in the compile worker, with its persistent cache in the run directory and output in memory |
| Shared chunks | Shared chunks split by Rspack, served with content-hashed names as immutable; entry modules served per preview |
| Lazy compilation | Rspack's `lazyCompilation`, or entries added to the compiler as previews are first requested if that option does not fit Workbench's entries; the report says which |
| Push | One event stream per canvas from the server. Preview frames receive build results through the canvas, and the revision poll is removed |
| HMR scope | Only the visible preview's frame applies hot updates. Retained hidden frames mark themselves stale, and update or reload when shown |
| Format edits | CSS: Rspack's CSS hot replacement. React: React Refresh. Vue: the Vue loader's hot reload. HTML previews: reload that keeps state, size and inputs, with linked CSS replaced in place. Astro: a server re-render of the preview whose HTML replaces the old in place, with islands hot-updated by their own framework. Docs: Markdown re-rendered in place, examples hot-updated as their adapter allows |
| Reload rule | Edits to a preview definition, an environment, `workbench.config.ts`, or a module that accepts no update reload the preview, keeping state, size and inputs |
| Errors | Build errors from Rspack's stats show in the frame as soon as the build reports them; the next good build recovers without a manual reload |
| Pre-warm | When a project's server starts, the worker starts the compiler, restores its persistent cache, builds the most recently selected preview (or the first) and prepares the spare or warm document, at a lower priority than any request |
| Plugins | Workbench's alias, Vue, HTML, Astro and React Native Web handling as Rspack loaders and plugins. Project `workbench.config.ts` plugins accept Rspack plugins and rules; the proposed contract is written in this README before the run |
| Readiness | Variant 1's readiness fixes |

These keep their current behavior unless the run shows Rspack can take
them over, and the report says which compiler each would use:

- definition and config evaluation, bundled for Node;
- export and portable builds;
- the sidebar's in-memory component bundle;
- capture.

If esbuild stays for any of them, the VSIX ships both binaries, and the
report gives the size.

### Fixtures

The [shared Acme fixture](../fixtures/README.md), extended for this lab:

| Size | Shape |
| --- | --- |
| Small | The existing 5 component previews. The shipping behavior already handles it well |
| Large, system fonts | The existing 40-page React fixture with system font stacks only and no web fonts |
| Large, web fonts | The 40-page fixture with heavy web fonts loaded by the environment: Inter's full variable font, Noto Sans JP with text that loads many of its unicode-range files, a variable icon font, and a monospace family on some pages only; plus a large raster image per page |
| Huge | 200 page previews of the same shape, to measure how lazy compilation scales |
| Formats | One project with every native format: React pages, Vue single-file components with scoped styles and component state, Astro previews with props, states and a React island, HTML previews with a linked stylesheet, React Native Web screens, docs pages with Markdown and examples in web and React Native Web lenses, a custom adapter registered in `workbench.config.ts`, global CSS and CSS modules |

The formats fixture's `workbench.config.ts` carries three project plugins of
the kinds real projects use:

- a virtual module served by the plugin;
- a source transform applied while loading files;
- an import redirected depending on the importing file.

The fixture provides each plugin in the old esbuild form and the proposed
Rspack form, so the lab measures both.

Font and image responses run at two latencies: as fast as the local server
allows, and with 150 ms added per response, to model a CDN.

The fixture's isolation sentinels are extended for variant 3. Each page:

- adds a window listener, a timer, a portal node, a `<body>` class and a
  `window` global;
- registers a request mock handler.

The next page checks that none of these remain.

A selected local project may be run as an additional read-only input, under
the methodology's rules. Its Rspack versions of its own config plugins are
supplied at run time from outside the repository. Its results stay in the
run's temporary directory and are reported to the owner directly. Neither
they, nor its plugins, nor anything that identifies it are committed, not
even by shape.

### Sequences

Start and first page:

| ID | Action |
| --- | --- |
| A1 | **Cold start:** an empty persistent cache and a new browser profile. Start the server and open the first page |
| A2 | **Hot start:** restart the server with the cache from A1 and the same browser profile, and open the first page |
| A3 | Open a page as soon as the server reports its URL, while pre-warming is under way |
| A4 | Open a page after pre-warming finishes |
| A5 | A1 and A4 on the huge fixture, against the large one |

Navigation:

| ID | Action |
| --- | --- |
| N1 | Select a second page right after the first |
| N2 | Walk ten pages not yet opened, paced: each selection waits until the previous page is visible and the canvas is idle for 1 s |
| N3 | The same walk, back to back |
| N4 | Return to a page shown earlier |
| N5 | Change state, inputs and size on the current page |
| N6 | Reload the current page |
| N7 | Switch between adapters in the formats fixture: React, Vue, Astro, HTML, React Native Web, a docs page |
| N8 | Select a page that uses a font not loaded yet |

Edits, each starting from a page with typed text and a changed counter:

| ID | Edit | Expected |
| --- | --- | --- |
| E1 | Global CSS, a CSS module, a Vue scoped style | Applied without a reload; state kept |
| E2 | A React component's render output | Fast Refresh; state kept |
| E3 | A module exporting more than components | Hot update where accepted; otherwise the affected tree re-mounts or the preview reloads, as the requirement says |
| E4 | A Vue component's template, then its script | Applied without a reload; state kept where Vue's hot reload keeps it |
| E5 | An HTML preview's markup, then its linked stylesheet | Markup: reload keeping state, size and inputs. Stylesheet: replaced in place |
| E6 | An Astro component, then its React island | Astro: re-rendered in place without a document reload. Island: hot update, state kept |
| E7 | A docs page's Markdown, then one of its examples | Markdown: re-rendered in place, scroll kept. Example: hot update as its adapter allows |
| E8 | A preview definition, the environment, `workbench.config.ts` | Reload, keeping state, size and inputs |
| E9 | A module shared by every page | Hot update in the visible page; hidden retained frames refresh when shown |
| E10 | A syntax error, then its fix | Error shown in the frame; recovered without a manual reload |
| E11 | Two saves 150 ms apart | The page ends with the second save's output |
| E12 | An edit while several retained frames are hidden | No console errors; each shows the edit when selected |

Errors and other surfaces:

| ID | Action |
| --- | --- |
| F1 | Select the preview that fails to compile, then the one that throws, then a working page |
| F2 | Capture screenshots of ten pages and states |
| F3 | Export a preview and build the portable output |
| F4 | Load the sidebar's component bundle |
| F5 | Render a page through the formats fixture's custom adapter |

### Boundaries and cache states

Timers follow the methodology's boundaries. The primary boundary is
selection to visible; edits use edit to visible. Each sample also records:

- compile, module load and mount;
- the font wait and the image wait, logged on their own events;
- fonts, images and scripts requested during the selection, with bytes and
  cache hits;
- whether the frame or document reloaded.

Two cache states are added for this lab:

- **Hot start:** a new server and compile worker with the persistent cache
  from an earlier run, and the same browser profile.
- **Runtime-warm:** a prepared spare or warm document that has loaded the
  shared code, global stylesheets and fonts.

### Correctness checks

All checks in the methodology, on every run, for every variant and
diagnostic control:

- **Render parity:** every adapter on the formats fixture, plus ten pages
  each of the large fixtures, at laptop and mobile sizes. In variant 3, each
  page is also captured after navigating from a different page.
- **Isolation:** the fixture's sentinels and variant 3's added sentinels, on
  every page of the N2 walk.
- **Freshness:** every E sequence ends with output equal to a fresh load of
  the edited source.
- **Error containment:** F1 and E10. A build error shows within 1 s of the
  build reporting it.
- **Readiness and capture:** screenshots show the final fonts and images.
  D3 is exempt.
- **Clean console:** no errors or warnings the control does not produce,
  including in hidden frames.

These further checks apply to variants 2 and 3:

- no revision polling requests;
- no file written inside the project, checked by hashing the project tree
  before and after each run set;
- Rspack's native binding loads in the bundled Electron runtime in Node mode,
  on this host;
- every project plugin of the formats fixture works in its Rspack form.

### Metrics

- **Primary:** N2 selection to visible, on both large fixtures.
- **Secondary:**
  - A1 to A5, N1, N3, N4, N6, N7 and N8;
  - E1 to E9 edit to visible;
  - bytes transferred and evaluated per selection;
  - the readiness waits;
  - worker peak RSS;
  - heap per retained frame or of the warm document after N2;
  - persistent cache size;
  - VSIX size per platform.

### Runs

The lab runs in two stages.

- **Stage 1, feasibility:** the formats fixture only. Every E and F sequence
  once, every correctness check, Electron loading and the plugin forms. A
  failed item is reported with the smallest change that would fix it or a
  fallback, such as keeping an adapter on its current compile path. Stage 2
  starts only when the owner accepts stage 1's result.
- **Stage 2, performance:** three warmup samples and ten samples per cell,
  with cells alternated in a seeded order. A1 and A2 take ten samples, each
  with its own server start. Two run sets at least an hour apart, one lab at
  a time under the lab lock, with load recorded per sample.

### Decision rule

The targets are the proposed ones in [performance](../../specs/system/performance.md#measured-interactions).
They are proposed, not agreed. The 10% threshold is the owner's.

- A variant that fails a correctness check is rejected.
- **The plan is confirmed** if variant 2 or 3:
  - passes every correctness check on every fixture;
  - meets the proposed targets on both large fixtures in both run sets;
  - is not slower than the control by more than 10% in any sequence on the
    small fixture.
- **A part of the plan is changed before implementation** if it fails a
  check or a target. The report names the part and the change. The plan is
  confirmed without that part only if the rest still meets every target.
- **Warm spare or warm document:** variant 3 is chosen only if it passes
  every isolation check and its N2 or N3 p50 is at least 10% faster than
  variant 2's on a large fixture. Otherwise variant 2, which keeps a new
  document per preview.
- **Readiness fixes:** if variant 1 is at least 10% faster than the control
  in N2, they are reported as worth shipping on their own, ahead of the
  rest.
- **Lazy compilation and pre-warming** are kept if D1 and D2 show they save
  time on the huge or large fixture without slowing any sequence by more
  than 10%.
- A difference within the spread of the two run sets is inconclusive for
  that comparison.

### Host and tools

macOS 15 on Intel (16 threads, 64 GB). Apple silicon and other
platforms are not measured. Headless Chrome through the standalone server
for run sets. The labs methodology requires confirming the integrated
product in the VS Code webview after implementation. Pinned in the lab:

- Rspack 2.2.7 and its React refresh plugin 2.0.2, as in the earlier labs;
- Vue Loader 17.4.2, with `style-loader` 4.0.0 and `css-loader` 7.1.5 for Vue style blocks;
- the `react-refresh` runtime 0.19.0 and `memfs` 4.79.0 for in-memory output.

esbuild is the version the package pins, for the control and for whatever
stays on it. Fixture packages are pinned by the shared fixture and installed
into the run directory.

## Running the lab

```sh
node labs/rspack-pipeline/run.mjs --variants 0,2,3 --fixture formats --results labs/rspack-pipeline/results/<date>-<label>
```

- `--only <edit,...>` runs the named edits only; `--debug` prints the
  worker's pipeline log; `--trace` prints the hot-update client's steps.
- `--lazy` turns on Rspack's lazy compilation (finding 9).
- `--project <path> --project-config <file>` runs a selected project read-only,
  with Rspack forms of its config plugins from a file outside the repository.
  Its report stays in the run directory; `--results` is refused.

The runner snapshots the package, copies `overlay/` over it for variants 2
and 3, installs the pinned tools and the fixture into a run directory under
the system temporary folder, and holds the lab lock. Variant 3 is variant 2
with the canvas flag `lab-warm-document`.

## Stage 2 method

`stage2.mjs` runs one run set. Variants 0, 1 and 2 are snapshots, as in
stage 1. Variant 1 copies only the overlay's preview runtime and canvas
stylesheet. Variant 3 and the diagnostic controls run variant 2's snapshot
with a flag:

- variant 3 with the canvas flag `lab-warm-document`;
- D2 with `WORKBENCH_LAB_NO_PREWARM`, so the first request starts the build;
- D3 with `WORKBENCH_LAB_READY_ON_MOUNT`, so readiness follows the hooks.

```sh
node labs/rspack-pipeline/stage2.mjs --set 1 --results labs/rspack-pipeline/results/<date>-stage-2-set-1
```

**Fixtures:**

| Fixture | Generator size | Shape |
| --- | --- | --- |
| Small | `small` | 5 component previews |
| Large, system fonts | `large-system` | 40 pages, system fonts, with extended sentinels |
| Large, web fonts | `large-web` | As above, plus the environment's web fonts (Inter variable, Noto Sans JP with text from across the CJK block, a variable icon font), a monospace family on every fourth page, and a 2.5 MB photo per page |
| Huge | `huge` | 200 pages, system fonts |
| Formats | `formats` | As stage 1 |

**Boundaries:**

- **Selection to visible** starts when the canvas changes its address. It
  stops two animation frames after the active frame is ready and shows the
  selected page's own element: the page's root, the component, the
  preview's sentinel class, or the docs marker.
- Start sequences count from the server process starting (A1, A2), from the
  server reporting its URL (A3), or from navigating the canvas (A4).
- **Edit to visible** starts when the file is written. It stops two
  animation frames after the page shows the edit's marker.
- Bytes and counts per selection come from the canvas's network events.

**Cells:**

| Group | Sequences | Fixtures | Variants |
| --- | --- | --- | --- |
| start | A1 cold, A2 hot (same cache and browser profile), A3 (A2 from the URL), A4 (after pre-warming); A5 compares huge with large | small, large system, huge | 0, 2, D2 |
| nav | N1 (a new browser per sample), then N2 paced, N3 back to back, N4, N5 state and size, N6, F1, idle polling, in one canvas | small, large system, large web | 0, 1, 2, 3, D3 |
| nav | The same | huge | 0, 2, 3 |
| nav | N7 between adapters, with the leak sentinels | formats | 0, 1, 2, 3 |
| nav | N8 (a new browser per sample), and N2 and N8 with 150 ms added to every font and image response | large web | 0, 1, 2, 3, D3 |
| edit | E2, E3, E8 definition and environment, E9 | large system | 0, 2, 3 |
| edit | E2 | small | 0, 2, 3 |
| edit | E1 to E9 from the fixture's edit list, applied and undone; E10 and E11 once | formats | 0, 2, 3 |
| parity | Ten pages in two states and two sizes; every formats page and the docs page | large system, formats | 0, 1, 2, 3 |

Each cell takes three warmups and ten samples. Every run checks:

- isolation sentinels on each N2 page and each N7 page;
- readiness on each N2 and N3 page;
- the broken previews (F1);
- no polling;
- the console;
- freshness after the edits, against a fresh load;
- the syntax error and the double save;
- render parity;
- no project writes.

## Stage 1 report

Run on 2026-10-12 at Workbench commit `63a096f` plus this lab's uncommitted
files, on the host above, under the lab lock. Variants 0, 2 and 3 on the
formats fixture, every sequence once. Evidence:
[report](results/2026-10-12-stage-1/report.json),
[summary](results/2026-10-12-stage-1/summary.md).

### Outcome

| Check | 0 control | 2 warm spare | 3 warm document |
| --- | --- | --- | --- |
| Every page and docs lens renders (20 renders, two walks) | pass | pass | pass |
| Starting state of every probe, including plugin effects | pass | pass | pass |
| Isolation sentinels, every page of both walks | pass | pass | pass |
| Broken previews show their errors; working pages load after | pass | pass | pass |
| Export and portable output (stays on esbuild) | pass | pass | pass |
| No revision requests during 5 s idle with every page open | fail | pass | pass |
| No file written inside the project | pass | pass | pass |
| Worker on the bundled Electron runtime | pass | pass | pass |
| Console clean apart from the control's own entries | pass | pass | pass |

Edit to visible, one sample each:

| Edit | 0 control | 2 warm spare | 3 warm document |
| --- | --- | --- | --- |
| Global CSS | 777 ms, re-render, state lost | 80 ms, hot, state kept | 77 ms, hot, state kept |
| CSS module | 921 ms, re-render, state lost | 55 ms, hot, state kept | 52 ms, hot, state kept |
| Vue scoped style | 858 ms, re-render, state lost | 54 ms, hot, state kept | 54 ms, hot, state kept |
| React component | 843 ms, re-render, state lost | 189 ms, hot, state kept | 82 ms, hot, state kept |
| Module exporting values | 932 ms, re-render, state lost | 79 ms, hot, state kept | 78 ms, hot, state kept |
| Vue template | 639 ms, re-render, state lost | 52 ms, hot, state kept | 52 ms, hot, state kept |
| Vue script | 795 ms, re-render, state lost | 52 ms, hot, state reset by Vue | 53 ms, hot, state reset by Vue |
| HTML markup | 797 ms, re-render | 52 ms, re-render in place | 52 ms, re-render in place |
| HTML linked stylesheet | 773 ms, re-render | 52 ms, hot | 53 ms, hot |
| Astro component | 801 ms, re-render | 80 ms, re-render in place | 80 ms, re-render in place |
| Docs Markdown | 752 ms, page reload | 51 ms, in place | 51 ms, in place |
| Docs example | 961 ms, page reload | 80 ms, in place | 80 ms, in place |
| Preview definition | 996 ms, re-render, state lost | 106 ms, preview reload, state lost | 103 ms, preview reload, state lost |
| Environment | 931 ms, re-render, state lost | 106 ms, preview reload, state lost | 133 ms, preview reload, state lost |
| `workbench.config.ts` | 964 ms, re-render, state lost | 375 ms, compilers replaced, reload, state lost | 384 ms, compilers replaced, reload, state lost |
| Module shared by every page, and a hidden page then shown | 937 ms, re-render, state lost | 78 ms, hot, state kept; hidden page current | 78 ms, hot, state kept; hidden page current |
| Syntax error, then its fix | did not recover within 30 s | error shown, recovered in 317 ms | error shown, recovered in 266 ms |
| Two saves 150 ms apart | ends on the second save | ends on the second save | ends on the second save |

"Re-render" for the control means its warm host loaded the rebuilt preview
again, losing component state. Times are single samples on one host, for
feasibility only; stage 2 measures them. The control's missed recovery after
the syntax error is consistent with the stale-output gap in
[TypeScript previews](../../specs/pages/previews.md#implementation-gaps), and was not
investigated further here.

A selected local project was also run read-only, with Rspack forms of its
config plugins supplied from outside the repository. Its results were
reported to the owner directly.

### Findings

1. **Rspack runs where the worker runs.** Rspack 2.2.7 loads and builds in the
   bundled Electron runtime (Electron 44.2, Node 24.20) in Node mode, writing
   output to an in-memory file system. Rspack 2 is ES modules only.
2. **Rspack writes its persistent cache inside the project by default**
   (`node_modules/.cache/rspack`). The pipeline points it at the system
   temporary folder. The file-system cache type needs an experimental
   switch; the stable `persistent` type takes a storage directory.
3. **A compilation is freed when the next build starts.** Reading it later
   throws. What serving needs (each entry's files, runtime files, CSS chunks
   and errors) is copied when each build finishes.
4. **Hot updates do not follow a module that moves to another chunk.** Adding
   an entry can move shared modules into a new split chunk; a document that
   loaded the old layout then receives updates only for a chunk it never
   loaded, and silently applies nothing. Plain Rspack reproduces the move.
   The entry set must stay fixed while documents are open.
5. **Content-hashed stylesheet names break CSS hot updates.** Rspack's CSS
   runtime fetches a chunk's new stylesheet by the name it already knows
   and finds links by URL prefix or `data-rspack`. Stylesheets keep stable
   names, served without caching, and the preview module tags its links.
   Scripts keep content-hashed names, so a preview module's file list always
   matches one build.
6. **CSS modules imported as a default object** need
   `parser: { namedExports: false }`.
7. **Vue Loader 17 style blocks do not work with Rspack's native CSS**: the
   block's JavaScript proxy is treated as CSS and the page loses its scoped
   styles. The classic `style-loader` and `css-loader` work, with hot
   updates. Vue Loader also needs pointing at Workbench's own Vue SFC
   compiler.
8. **The warm host needs these changes for shared runtimes:**
   - it keeps the bundler's globals;
   - project modules are evaluated again for each preview, and
     dependencies are kept;
   - `style-loader`'s registry starts over with them;
   - build errors go only to the preview whose graph contains them.
9. **Lazy compilation is not yet reliable in the prototype.**
   - Workbench's server forwards only GET requests to the worker, and
     Rspack's middleware expects a POST, so the lab uses a copy of Rspack's
     client that asks with GET.
   - Most previews compiled on first request in 0.2–0.3 s.
   - The first preview's request stalled, a compile error did not show for
     a lazily built entry, and pages stopped loading after a config change.
   - Whether lazy compilation keeps finding 4 away is not established.
10. **A config change replaces the compilers.** Each set writes to its own
    folder; documents from the old set reload into a fresh warm host; quick
    successive changes are applied one at a time.
11. **esbuild stays.** Preview definitions and config are still evaluated
    with esbuild, export and portable output still build with it, and the
    sidebar bundle is unchanged. The VSIX would ship both compilers.

### Not covered by stage 1

- docs pages declared in `workbench.yaml` rather than in a definition;
- the readiness fixes' and pre-warming's effect on timing (stage 2);
- the VS Code webview, and platforms other than Intel macOS;
- a preview added while pages are open, which changes the entry set (finding 4).

### Result against the stage gate

The pipeline's parts work together on every native format with the changes
above: hot updates with React and Vue state kept, CSS and docs updated in
place, HTML and Astro re-rendered in place, edits that cannot apply in place
reloading, errors contained, nothing written to the project, and no polling.
Lazy compilation is the exception (finding 9). The recommendation is to
accept stage 1 and run stage 2 with the fixed entry set. Lazy compilation
would be a separate question, needed only if building every preview at
start is too slow on large projects, which stage 2's huge fixture measures.

## Stage 2 report

Two run sets on 2026-10-13, at Workbench commit `4208f0d` plus this lab's
uncommitted files (hashed in each report under `measured`), on the host
above, under the lab lock, each cell with 3 warmups and 10 samples:

| Set | Time (UTC) | Load at start | Evidence |
| --- | --- | --- | --- |
| 1 | 00:31–02:13 | one-minute load average 7.0; a terminal, an agent and a backup agent busy | [report](results/2026-10-13-stage-2-set-1/report.json), [summary](results/2026-10-13-stage-2-set-1/summary.md) |
| 2 | 03:14–05:03 | load average 4.2; another process at 100% of one core | [report](results/2026-10-13-stage-2-set-2/report.json), [summary](results/2026-10-13-stage-2-set-2/summary.md) |

Each sample records the load average. Values below are p50 / p95 in
milliseconds, set 1 then set 2.

### Selecting a page not yet opened

| Fixture | Sequence | 0 control | 1 readiness only | 2 warm spare | 3 warm document | D3 ready on mount |
| --- | --- | --- | --- | --- | --- | --- |
| Large, system fonts | N2 paced | 396/435, 404/427 | 411/450, 434/463 | 236/284, 238/326 | 179/257, 144/209 | 205/228, 219/243 |
| Large, system fonts | N3 back to back | 1398/1493, 1424/1508 | 1445/1615, 1491/1642 | 230/310, 226/263 | 137/164, 141/170 | 195/220, 215/273 |
| Large, web fonts | N2 paced | 1276/1309, 1272/1379 | 492/520, 481/515 | 365/992, 257/332 | 164/180, 161/194 | 237/344, 241/309 |
| Large, web fonts | N3 back to back | 2617/4963, 2485/2735 | 2348/2600, 2386/2638 | 266/402, 263/287 | 176/203, 177/190 | 237/263, 240/295 |
| Large, web fonts | N2, +150 ms per font and image | 1564, 1602 | 596, 589 | 492, 396 | 354, 303 | 228, 231 |
| Large, web fonts | N8 a new font | 2523, 2546 | 1767, 1860 | 377, 378 | 244, 225 | 362, 339 |
| Large, web fonts | N8, +150 ms | 1581, 1626 | 711, 793 | 649, 633 | 518, 495 | 663, 655 |
| Huge | N2 paced | 402, 400 | — | 233, 244 | 155, 156 | — |
| Huge | N3 back to back | 1367, 1417 | — | 231, 230 | 140, 144 | — |
| Formats | N7 between adapters | 189, 194 | 186, 197 | 186, 194 | 151, 154 | — |

Bytes loaded per N2 selection (median, both sets):

- large system-font fixture: 4.98 MB in 5 scripts for the control and
  variant 1; 0.42 MB in 12 scripts for variant 2; 0.36 MB in 2 scripts for
  variant 3;
- large web-font fixture: 13.8 MB for the control, 7.8 MB for variant 1,
  3.0 MB for variants 2 and 3, of which the page's photo is 2.5 MB.

### Other navigation

| Fixture | Sequence | 0 | 2 | 3 |
| --- | --- | --- | --- | --- |
| Large, system fonts | N1 second page | 1377, 1431 | 321, 325 | 172, 168 |
| Large, web fonts | N1 second page | 2673, 2517 | 386, 385 | 250, 233 |
| Large, system fonts | N4 retained | 55, 60 | 52, 44 | 98/118, 94/105 |
| Large, web fonts | N4 retained | 60, 58 | 46, 48 | 114/141, 109/125 |
| Large, system fonts | N5 state | 53, 52 | 46, 49 | 143, 143 |
| Large, web fonts | N5 state | 50, 54 | 47, 48 | 162, 168 |
| Large fixtures | N5 size | 17–21 | 17–24 | 16–29 |
| Large fixtures | N6 reload | 165–203 | 177–214 | 90–107 |

### Start

| Fixture | Sequence | 0 control | 2 pipeline | D2 no pre-warm |
| --- | --- | --- | --- | --- |
| Small | A1 cold, from server start | 4528, 4119 | 4263, 4158 | 4310, 4114 |
| Small | A3 hot, from URL | 1310, 1186 | 1154, 1206 | 1429, 1492 |
| Small | A4 after pre-warm | 352, 313 | 306, 296 | 603, 610 |
| Large, system fonts | A3-cold, from URL | 5306, 5776 | 6052, 5986 | 5780, 6459 |
| Large, system fonts | A3 hot, from URL | 1528/1705, 1756/2152 | 3322/3684, 3406/3533 | 3260, 3852 |
| Large, system fonts | A4 after pre-warm | 671, 700 | 812, 826 | 2377, 2736 |
| Huge | A3-cold, from URL | 10467, 7106 | 9954, 17311 | 9371, 12553 |
| Huge | A3 hot, from URL | 4714, 3211 | 7764, 13350 | 7155, 9392 |
| Huge | A4 after pre-warm | 1911, 832 | 985, 2075 | 5356, 6331 |

A1 and A2 (from the server process starting) add the server's own start,
about 0.4 s, to A3-cold and A3.

### Edits

| Fixture | Edit | 0 control | 2 warm spare | 3 warm document |
| --- | --- | --- | --- | --- |
| Small | E2 React component | 836, 874 | 60, 62 | 59, 66 |
| Large, system fonts | E2 React component (state kept every sample in 2 and 3, lost in 0) | 1924, 1916 | 159, 160 | 151, 157 |
| Large, system fonts | E3 module exporting more than components | 2051, 2054 | 174, 195 | 179, 178 |
| Large, system fonts | E8 preview definition | 2063, 2129 | 350, 368 | 354, 345 |
| Large, system fonts | E8 environment | 2045, 2126 | 454, 453 | 437, 459 |
| Large, system fonts | E9 module shared by every page | 2035, 2208 | 171, 203 | 169, 172 |
| Formats | E1 stylesheets (global, module, Vue scoped) | 619–706 | 61–99 | 65–95 |
| Formats | E2–E4 React and Vue | 578–710 | 70–109 | 64–107 |
| Formats | E5 HTML markup, stylesheet | 583–594 | 146–173, 70–90 | 149–170, 63–93 |
| Formats | E6 Astro component | 589, 592 | 173, 176 | 168, 170 |
| Formats | E7 docs Markdown, example | 415–593 | 48–108 | 49–115 |
| Formats | E8 definition (set 2 only) | 547 | 254 | 266 |
| Formats | E8 environment | 724, 581 | 231, 266 | 236, 269 |
| Formats | E8 `workbench.config.ts` | 619, 588 | 580, 594 | 547, 551 |
| Formats | E9 shared module | 710, 701 | 109, 122 | 104, 105 |

### Resources

After the N2 and N3 walks:

| Fixture | Value | 0 | 2 | 3 |
| --- | --- | --- | --- | --- |
| Large, system fonts | canvas heap | 155 MB | 295 MB | 162 MB |
| Large, web fonts | canvas heap | 146 MB | 297 MB | 138 MB |
| Huge | canvas heap | 149 MB | 268 MB | 148 MB |
| Large, system fonts | server and worker peak memory | 1.8 GB | 1.1 GB | 1.1 GB |
| Large, web fonts | server and worker peak memory | 2.6 GB | 1.5 GB | 1.6 GB |
| Huge | server and worker peak memory | 1.7 GB | 2.0 GB | 2.0 GB |

The compile cache after a cold start held 3, 16 and 16 MB for the control on
the small, large and huge fixtures, and 2, 142 and 389 MB for the pipeline.

### Correctness

| Check | 0 | 1 | 2 | 3 | D2 | D3 |
| --- | --- | --- | --- | --- | --- | --- |
| Render parity (40 large renders, 15 formats renders) | reference | pass | pass | pass | — | — |
| Isolation sentinels (N2 pages, N7 pages) | pass | pass | pass | pass | — | pass |
| Readiness: fonts loaded and visible images complete when shown | pass | **fail: 26 of 26 web-font pages, both sets** | pass | pass | — | exempt |
| Broken previews show errors; working page after | pass | pass | pass | pass | — | pass |
| No polling | fail | fail | pass | pass | — | pass |
| Freshness after edits | pass | — | pass | pass (see amendment) | — | — |
| Syntax error, then fix | fail | — | pass | pass | — | — |
| Two saves 150 ms apart | pass | — | pass | pass | — | — |
| No project writes | pass, all fixtures, both sets | | | | | |

Two N8 samples failed to open a browser tab (Chrome timed out), one in
variant 0 in set 1 and one in variant 2 in set 2. They are reported as errors and left
out of the aggregates.

### Analysis

- **Navigation.** The hypothesis holds for selecting pages. A new page costs
  its own chunks: 0.36–0.42 MB against 5 MB, and on the web-font fixture
  about 0.5 MB plus its photo. Both pipeline variants are about 2 to 15 times
  faster than the control, and the two sets agree within a few percent except
  variant 2's web-font N2 tail in set 1.
- **Readiness fixes alone are not safe.** Variant 1's speed on the web-font
  fixture came from becoming ready while fonts were still loading. The
  font set reports `loaded` when the check runs, and the page's own text
  starts more downloads afterwards. Variants 2 and 3 passed because their
  fonts were already loaded. On the system-font fixture, variant 1 is no
  faster than the control. D3 puts the remaining cost of readiness in the
  pipeline at about 20–30 ms on system fonts and up to 130 ms on web fonts.
- **The warm document is slower to return and to change state.** It
  re-evaluates the preview's modules when the address changes, including a
  state change on the same page or a return to it. This is the prototype's
  canvas flag, not a limit of the approach: those changes can re-render in
  place, as they do in variant 2.
- **Start costs more with the pipeline.** With the cache warm, the first page
  after a restart waits for every entry to rebuild: 3.3–3.4 s against
  1.5–1.8 s on the large fixture. Pre-warming hides this when it has time to
  finish (A4: 0.81–0.83 s, against 2.4–2.7 s without it), but not when a
  page is opened at once. On the huge fixture the sets disagree by up to two
  times in both directions, so its start cells are inconclusive.
- **Memory.** The warm spare holds about twice the canvas heap of the
  control or the warm document. The pipeline's worker uses less memory than
  the control on the large fixtures and more on the huge one, and its
  persistent cache is 9–24 times the control's.
- **Threats to validity.** One Intel host; headless Chrome, not the VS Code
  webview; background load in both sets; variants alternate per group, not
  per sample; ten samples, so p95 values are exploratory; the small fixture
  has four pages to walk, so its N2 has one sample and N3 none; the
  prototype is not the product.

### Result against the decision rule

- **Variant 1 is rejected:** it fails the readiness check. Its readiness rule
  is not worth shipping on its own as written.
- **Variants 2 and 3 pass every correctness check** in both sets.
- **The plan is not confirmed as it stands.** Against the proposed targets
  on both large fixtures in both sets:

  | Target | Variant 2 | Variant 3 |
  | --- | --- | --- |
  | New page p50 ≤ 300 ms, p95 ≤ 600 ms | misses: web-font p95 992 ms in set 1 | meets |
  | Retained page p50 ≤ 100 ms | meets | misses on web fonts (109–114 ms) |
  | State or inputs ≤ 50 ms of overhead | meets (46–49 ms, total) | misses (143–168 ms) |
  | Size ≤ 50 ms, no reload | meets | meets |
  | Reload p50 ≤ 600 ms | meets | meets |
  | React edit with state kept p50 ≤ 300 ms | meets | meets |
  | Own-source edit that reloads p50 ≤ 1 s | meets | meets |
  | Shared-code edit p50 ≤ 2 s | meets | meets |
  | First page, cache warm, p50 ≤ 1.5 s | misses if opened at once (3.3–3.4 s); meets after pre-warming (0.81–0.83 s) | as variant 2 |
  | First page, cache cold, p50 ≤ 5 s | misses (6.0 s; the control's 5.3–5.8 s misses too) | as variant 2 |
  | List pages ≤ 500 ms | not measured | not measured |

- **The parts to change before implementation:**
  1. readiness waits for fonts that the page starts loading, not only those
     loading when it checks;
  2. the warm document re-renders in place for state, input and size
     changes, and when returning to the page it shows;
  3. the first page after a restart does not wait for every entry to
     rebuild. That means either serving it before the full build finishes,
     which stage 1's finding 4 rules out for open documents, or lazy
     compilation, which stage 1 found unreliable.
- **Warm spare or warm document:** variant 3 passes every isolation check,
  and its N2 p50 is 24–40% faster than variant 2's on the system-font fixture
  and 37–55% faster on the web-font fixture, in both sets. By the rule,
  **variant 3 is chosen**, with change 2.
- **Pre-warming is kept:** it saves 1.6–1.9 s on the first page when it can
  finish, and no sequence is slower with it.
- **Lazy compilation was not measured** (D1 dropped). The start results make
  it, or another way to show the first page early, the open question that
  remains.
- **Small fixture:** no sequence is consistently more than 10% slower than
  the control. Cells of 20–50 ms move by more than 10% between sets in both
  directions, so those comparisons are inconclusive by the rule.
- The owner decides. Implementation still needs its own confirmation run
  through the shipping server and the VS Code webview.

## Confirmation run, 2026-10-13

The [labs methodology](../../specs/system/labs.md#reports-and-decisions) asks for the
lab's sequences to be run again through the shipping product once the winning
change is built in. Run on 2026-10-13, 16:35–17:11 UTC, with the product as
variant 0 and the stage 2 runner unchanged, on the host above in headless
Chrome ([report](results/2026-10-13-confirmation/report.json),
[summary](results/2026-10-13-confirmation/summary.md)).

**Inconclusive for start and edits.** Test suites from other work were
running on the host throughout. The one-minute load average had a median of
50 during the start cells, peaking at 528, and 30 during the edits, on 16
hardware threads. The start and edit samples follow the load, so they are not
evidence about the product. The run is to be repeated on a quiet host.

The navigation cells ran at a median load of 11. They match the prototype's
warm document within its spread, and two improve on it:

- state changes on the same page take 95–106 ms at the median, against 143–168
  ms in the prototype, because the product re-renders them in place;
- a reload takes about 60 ms, against 94–107 ms.

Every correctness check passed.
