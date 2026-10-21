# Highlighting performance campaign

All three performance sets completed. These are standalone shipping-canvas measurements, with the release server and pinned local worker assets. Correctness findings remain separate; this is not an adoption decision or an installed-VS-Code latency claim.

Neither worker prototype meets the agreed overall performance rule. highlight.js improves dense first-page colouring by 22–26%, exceeding both the 10 ms / 20% gain thresholds and control variation in both cache conditions across all three sets. It also consistently slows dense second-page visibility and small-page first-use colouring. TextMate is slower on the motivating and dense first-page cases. The lab recommendation was to retain asynchronous Syntect.

Owner decision, 2026-10-21: adopt asynchronous highlight.js. Integration defers worker creation until after document load and applies CSS Highlight ranges over unchanged text nodes. The prototype results below remain unchanged and do not measure that integration. The decision accepts the engine despite the prototype's overall guardrail result; it does not relax correctness requirements or performance thresholds.

Cold-startup Code-heavy p50s, in milliseconds:

| Set | Syntect coloured | highlight.js coloured | Gain | TextMate coloured | Syntect second page visible | highlight.js second page visible |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| set1 | 874.1 | 649.8 | 25.7% | 1670.3 | 230.6 | 407.9 |
| set2 | 910.2 | 671.9 | 26.2% | 1769.5 | 248.5 | 414.7 |
| confirmation | 812.0 | 633.6 | 22.0% | — | 212.2 | 402.2 |

All table cells have 20 successful observations except TextMate/set2 colouring, which has 19. Failures remain in the evidence. Control first-colour p50 variation was 98.2 ms for startup and 85.6 ms for compiled-first; highlight.js gains were 177.0–238.3 ms across those six cells. The second-page visibility regressions exceed the larger-of-5-ms-or-10% allowance in every set/cache condition.

Initial dense-page visibility itself was similar: Syntect/highlight.js were 254.1/251.0 ms in confirmation. First Show code was readable at 18.3/19.8 ms, with colours at 109.3/50.2 ms. These are separate clocks; faster colouring does not imply faster readable content. Overview first colouring went from 174.7 to 209.2 ms in confirmation, and Small's second-page JSON colouring went from 99.7 to 115.1 ms. The same smaller-page regressions occurred in the full sets. Retained visibility stayed about 45–49 ms in the cold-startup cells. Plain diagnostic D had second-page dense visibility around 139 ms in both full sets, with no colour-completion metric.

highlight.js stayed within the proposed settled combined-RSS allowance in all 18 measured cells. TextMate exceeded it in Set 2's cold Code-heavy cell: +407.5 MiB, or 20.95%, versus the 20% allowance (19 memory observations). RSS sums cover the owned server/helper and browser process groups, including retained documents; they are not isolated library allocations.

The [assessment](assessment.json) contains every compared latency/memory cell, its allowance, actual n and control variation, plus independently recomputed aggregates, source/fixture identity checks and per-variant correctness coverage. Latency exceedance counts include related derived timers and secondary edit-settling timers; the repeated second-page and small-page regressions above independently fail the rule. No threshold was relaxed.

## Conditions, phases and reliability

Measurements used the release binary `5663597d8c69f14a9dbebc1e97c3e30176c01791285955865494db15bd1897d2`, tracing enabled identically, Node 26.12.0 and Chrome 155.0.8059.85 on macOS x86_64 / Darwin 24.6.0, a 16-logical-CPU Xeon W-2140B and 64 GiB RAM. Every trial recorded the same Chrome version. Product and fixture hashes match across sets; the second and third sets inherited the first set's frozen inputs. Warm-up, readiness blockers and three-idle-document retention were unchanged. Runtime/grammar/font assets were local. The native runtime was extracted once per set before timing, while each trial started fresh processes and browser/compiler caches. Installed VS Code 1.142.0 / Workbench 0.14.0 compatibility and asynchronous behavior were checked in the [preflight evidence](../2026-10-20-preflight/README.md); installed-webview latency confirmation remains necessary for any integrated replacement.

The [pipeline phase samples and aggregates](pipeline-phases.json) and [CSV](pipeline-phases.csv) add the native request/server split and worker time outside the engine. Native initialization and tokenization share one server phase. In Set 2's cold dense cell its p50 was 339.1 ms, with a 346.7 ms request and 2.0 ms outside-server residual. highlight.js initialization/tokenization p50s were 28.8/93.6 ms, request-to-reply 132.4 ms, and outside-engine work 10.3 ms. TextMate initialization/grammar/tokenization were 21.7/92.3/904.6 ms, request-to-reply 1021.6 ms and outside-engine work 13.9 ms. The latter residual includes worker startup, queueing, cloning and message handling. These independent medians do not add to end-to-end p50s. DOM parsing/insertion, resource response bytes, long tasks, CPU observations and memory are in each set's complete tables. Set 2's dense first-page main-thread long-task total p50 was 0/0/169 ms for Syntect/highlight.js/TextMate. Response-byte counts exclude worker message bytes; Show code's retained resource records lack Server-Timing, so its server phase is not separately reconstructed.

All three sets passed the declared 60-second quiet admission. Subsequent one-minute load observations ranged 5.1–381.6, 6.9–156.6 and 4.3–50.2 respectively on 16 logical CPUs. Every trial was retained. Host load substantially limits precise tail and budget claims; nearest-rank p95 at n=20 is exploratory. The quieter confirmation repeats the important gain and regression rather than removing noisy samples from the earlier sets. Power state was not inspected because host policy disallowed the query.

There were six execution failures: five TextMate trials (two first-fence colour fallbacks, two missing-document copy probes and one Chromium evaluation timeout), and one Syntect fault-injection check. Normal action outcomes additionally record one Syntect evicted-page plain fallback, in that same failed trial. The two TextMate normal fallbacks and the Syntect fallback are excluded from successful-colour quantiles while their independently completed visibility remains. All failures and missing downstream checks remain explicit. Confirmation had no execution failures or normal colour fallbacks. All sets reported zero cleanup failures and restored every fixture.

| Variant | Trials | Initial text / outer geometry checks | Language-role corpus | Copy / lifecycle / capture observations | Raster / character geometry |
| --- | ---: | --- | --- | --- | --- |
| Syntect | 360 | 360 passed | 360 SCSS-variable failures | 360 / 360 / 360 | Control raster reference; character delta up to 0.140625 px |
| highlight.js | 360 | 360 passed | 360 passed | 360 / 360 / 360 | 359 failed raster comparisons, 1 unobserved; character delta up to 0.1875 px |
| TextMate | 240 | 239 passed, 1 unobserved | 237 passed, 3 unobserved | 235 / 235 / 235 | 155 raster comparisons passed, 80 failed, 5 unobserved; character delta up to 0.171875 px |
| Plain D | 240 | 240 passed | Diagnostic | 240 / 240 / 240 | 239 differing raster comparisons, 1 unobserved; zero character delta |

Copy, lifecycle, example-failure containment, fault fallbacks and absence of console/external-request errors passed wherever their completed checks were recorded; the failed Syntect injection and unobserved checks above remain exceptions. D has no highlighter failure injection. Every capture count denotes an observed docs image, not a separate latency or pixel-parity pass. Source/copy correctness and outer block bounds do not resolve the character/raster findings. The preflight highlight.js edit timeout also remains recorded despite all 360 campaign trials completing. These findings independently prevent claiming a fully correct replacement; the performance guardrail rejection does not depend on them. Portable/export and installed-VS-Code compatibility retain their separate preflight evidence. No product code, warm-docs strategy or acceptance tolerance changed.

| Set | Attempted trials | Complete navigation sequences | Execution failures |
| --- | ---: | ---: | ---: |
| set1 | 480 | 478 | 4 |
| set2 | 480 | 479 | 2 |
| confirmation | 240 | 240 | 0 |

Performance confirmation repeated control 0 and worker 1, selected by the predefined Code-heavy first-colour rule.

Values below are nearest-rank p50 in milliseconds, rounded for display. Counts are the actual observations per metric, not the attempted trial count. JSON and CSV retain full precision; p95 and maxima are in the linked tables. Failed/timed-out actions are not latencies.

## Initial docs

| Set | Fixture | Cache | Variant | Visible n / p50 | Coloured n / p50 |
| --- | --- | --- | --- | ---: | ---: |
| set1 | Code-heavy | startup | 0 | 20 / 261.6 | 20 / 874.1 |
| set1 | Code-heavy | startup | 1 | 20 / 255.7 | 20 / 649.8 |
| set1 | Code-heavy | startup | 2 | 20 / 268.6 | 20 / 1670.3 |
| set1 | Code-heavy | startup | D | 20 / 264.4 | — |
| set1 | Code-heavy | compiled first | 0 | 20 / 263.2 | 20 / 852.1 |
| set1 | Code-heavy | compiled first | 1 | 20 / 263.7 | 20 / 637.5 |
| set1 | Code-heavy | compiled first | 2 | 20 / 261.5 | 19 / 1703.6 |
| set1 | Code-heavy | compiled first | D | 20 / 244.8 | — |
| set1 | Overview | startup | 0 | 20 / 219.2 | 20 / 194.9 |
| set1 | Overview | startup | 1 | 20 / 212.6 | 20 / 229.5 |
| set1 | Overview | startup | 2 | 20 / 226.3 | 20 / 281.5 |
| set1 | Overview | startup | D | 20 / 216.4 | — |
| set1 | Overview | compiled first | 0 | 20 / 207 | 20 / 188.2 |
| set1 | Overview | compiled first | 1 | 20 / 210.9 | 20 / 229.8 |
| set1 | Overview | compiled first | 2 | 20 / 220.9 | 20 / 269.9 |
| set1 | Overview | compiled first | D | 20 / 223.4 | — |
| set1 | Small | startup | 0 | 20 / 208.3 | — |
| set1 | Small | startup | 1 | 20 / 204.6 | — |
| set1 | Small | startup | 2 | 19 / 224.9 | — |
| set1 | Small | startup | D | 20 / 209.4 | — |
| set1 | Small | compiled first | 0 | 20 / 203.9 | — |
| set1 | Small | compiled first | 1 | 20 / 197.8 | — |
| set1 | Small | compiled first | 2 | 20 / 210.2 | — |
| set1 | Small | compiled first | D | 20 / 205.5 | — |
| set2 | Code-heavy | startup | 0 | 20 / 281 | 20 / 910.2 |
| set2 | Code-heavy | startup | 1 | 20 / 281.3 | 20 / 671.9 |
| set2 | Code-heavy | startup | 2 | 20 / 275.8 | 19 / 1769.5 |
| set2 | Code-heavy | startup | D | 20 / 303.6 | — |
| set2 | Code-heavy | compiled first | 0 | 20 / 281.5 | 20 / 894.2 |
| set2 | Code-heavy | compiled first | 1 | 20 / 288.5 | 20 / 675.1 |
| set2 | Code-heavy | compiled first | 2 | 20 / 276 | 20 / 1782.2 |
| set2 | Code-heavy | compiled first | D | 20 / 294.7 | — |
| set2 | Overview | startup | 0 | 20 / 223.1 | 20 / 205.6 |
| set2 | Overview | startup | 1 | 20 / 244.1 | 20 / 258.1 |
| set2 | Overview | startup | 2 | 20 / 238.2 | 20 / 298.1 |
| set2 | Overview | startup | D | 20 / 237.8 | — |
| set2 | Overview | compiled first | 0 | 20 / 228.8 | 20 / 210.1 |
| set2 | Overview | compiled first | 1 | 20 / 239.7 | 20 / 251.6 |
| set2 | Overview | compiled first | 2 | 20 / 238.1 | 20 / 307.3 |
| set2 | Overview | compiled first | D | 20 / 259.1 | — |
| set2 | Small | startup | 0 | 20 / 245.9 | — |
| set2 | Small | startup | 1 | 20 / 228.8 | — |
| set2 | Small | startup | 2 | 20 / 226.7 | — |
| set2 | Small | startup | D | 20 / 209.2 | — |
| set2 | Small | compiled first | 0 | 20 / 227.1 | — |
| set2 | Small | compiled first | 1 | 20 / 227.8 | — |
| set2 | Small | compiled first | 2 | 20 / 220.4 | — |
| set2 | Small | compiled first | D | 20 / 225.3 | — |
| confirmation | Code-heavy | startup | 0 | 20 / 254.1 | 20 / 812 |
| confirmation | Code-heavy | startup | 1 | 20 / 251 | 20 / 633.6 |
| confirmation | Code-heavy | compiled first | 0 | 20 / 243.6 | 20 / 808.6 |
| confirmation | Code-heavy | compiled first | 1 | 20 / 245.6 | 20 / 631.6 |
| confirmation | Overview | startup | 0 | 20 / 204.6 | 20 / 174.7 |
| confirmation | Overview | startup | 1 | 20 / 199.2 | 20 / 209.2 |
| confirmation | Overview | compiled first | 0 | 20 / 202.4 | 20 / 172.3 |
| confirmation | Overview | compiled first | 1 | 20 / 203.8 | 20 / 210.2 |
| confirmation | Small | startup | 0 | 20 / 185.1 | — |
| confirmation | Small | startup | 1 | 20 / 194.3 | — |
| confirmation | Small | compiled first | 0 | 20 / 195.9 | — |
| confirmation | Small | compiled first | 1 | 20 / 191.5 | — |

## First Show code

| Set | Fixture | Cache | Variant | Readable n / p50 | Coloured n / p50 |
| --- | --- | --- | --- | ---: | ---: |
| set1 | Code-heavy | startup | 0 | 20 / 18 | 20 / 129.7 |
| set1 | Code-heavy | startup | 1 | 20 / 19.4 | 20 / 55.6 |
| set1 | Code-heavy | startup | 2 | 20 / 22.6 | 20 / 112.7 |
| set1 | Code-heavy | startup | D | 20 / 10.1 | — |
| set1 | Code-heavy | compiled first | 0 | 20 / 20.6 | 20 / 124.7 |
| set1 | Code-heavy | compiled first | 1 | 20 / 18.5 | 20 / 56 |
| set1 | Code-heavy | compiled first | 2 | 20 / 21.6 | 20 / 107.1 |
| set1 | Code-heavy | compiled first | D | 20 / 10 | — |
| set1 | Overview | startup | 0 | 20 / 14.4 | 20 / 58.2 |
| set1 | Overview | startup | 1 | 20 / 12.4 | 20 / 39.9 |
| set1 | Overview | startup | 2 | 20 / 12.5 | 20 / 111.3 |
| set1 | Overview | startup | D | 20 / 14.2 | — |
| set1 | Overview | compiled first | 0 | 20 / 14.1 | 20 / 57.9 |
| set1 | Overview | compiled first | 1 | 20 / 12.1 | 20 / 40.2 |
| set1 | Overview | compiled first | 2 | 20 / 11.9 | 20 / 108.9 |
| set1 | Overview | compiled first | D | 20 / 14.5 | — |
| set1 | Small | startup | 0 | 20 / 14.5 | 20 / 60.6 |
| set1 | Small | startup | 1 | 20 / 14.4 | 20 / 79.8 |
| set1 | Small | startup | 2 | 19 / 14.4 | 19 / 163.3 |
| set1 | Small | startup | D | 20 / 14.2 | — |
| set1 | Small | compiled first | 0 | 20 / 14.1 | 20 / 59.8 |
| set1 | Small | compiled first | 1 | 20 / 14.3 | 20 / 75.7 |
| set1 | Small | compiled first | 2 | 20 / 14.3 | 20 / 162.5 |
| set1 | Small | compiled first | D | 20 / 14.3 | — |
| set2 | Code-heavy | startup | 0 | 20 / 18.9 | 20 / 134.3 |
| set2 | Code-heavy | startup | 1 | 20 / 20.7 | 20 / 59.1 |
| set2 | Code-heavy | startup | 2 | 20 / 26.9 | 20 / 110.3 |
| set2 | Code-heavy | startup | D | 20 / 10.7 | — |
| set2 | Code-heavy | compiled first | 0 | 20 / 18.3 | 20 / 133.2 |
| set2 | Code-heavy | compiled first | 1 | 20 / 21 | 20 / 55 |
| set2 | Code-heavy | compiled first | 2 | 20 / 22.8 | 20 / 117.9 |
| set2 | Code-heavy | compiled first | D | 20 / 9.5 | — |
| set2 | Overview | startup | 0 | 20 / 14.2 | 20 / 60.8 |
| set2 | Overview | startup | 1 | 20 / 11.9 | 20 / 40.1 |
| set2 | Overview | startup | 2 | 20 / 12.5 | 20 / 116.8 |
| set2 | Overview | startup | D | 20 / 13.8 | — |
| set2 | Overview | compiled first | 0 | 20 / 14.4 | 20 / 60.5 |
| set2 | Overview | compiled first | 1 | 20 / 12.1 | 20 / 40.4 |
| set2 | Overview | compiled first | 2 | 20 / 12.6 | 20 / 111.8 |
| set2 | Overview | compiled first | D | 20 / 14.3 | — |
| set2 | Small | startup | 0 | 20 / 14.2 | 20 / 65.3 |
| set2 | Small | startup | 1 | 20 / 14.1 | 20 / 79.5 |
| set2 | Small | startup | 2 | 20 / 14.4 | 20 / 174.5 |
| set2 | Small | startup | D | 20 / 14.2 | — |
| set2 | Small | compiled first | 0 | 20 / 14.3 | 20 / 61.4 |
| set2 | Small | compiled first | 1 | 20 / 14.2 | 20 / 79.4 |
| set2 | Small | compiled first | 2 | 20 / 14 | 20 / 172.8 |
| set2 | Small | compiled first | D | 20 / 14.4 | — |
| confirmation | Code-heavy | startup | 0 | 20 / 18.3 | 20 / 109.3 |
| confirmation | Code-heavy | startup | 1 | 20 / 19.8 | 20 / 50.2 |
| confirmation | Code-heavy | compiled first | 0 | 20 / 9 | 20 / 108.7 |
| confirmation | Code-heavy | compiled first | 1 | 20 / 17.7 | 20 / 53.3 |
| confirmation | Overview | startup | 0 | 20 / 14.4 | 20 / 57.1 |
| confirmation | Overview | startup | 1 | 20 / 12.3 | 20 / 39.9 |
| confirmation | Overview | compiled first | 0 | 20 / 14.1 | 20 / 57.2 |
| confirmation | Overview | compiled first | 1 | 20 / 12.1 | 20 / 39.8 |
| confirmation | Small | startup | 0 | 20 / 14.6 | 20 / 61.4 |
| confirmation | Small | startup | 1 | 20 / 14.4 | 20 / 73.5 |
| confirmation | Small | compiled first | 0 | 20 / 14.4 | 20 / 59.5 |
| confirmation | Small | compiled first | 1 | 20 / 14.1 | 20 / 74.2 |

## Retained and evicted returns

| Set | Fixture | Cache | Variant | Retained visible n / p50 | Evicted visible n / p50 |
| --- | --- | --- | --- | ---: | ---: |
| set1 | Code-heavy | startup | 0 | 20 / 46 | 20 / 148.4 |
| set1 | Code-heavy | startup | 1 | 20 / 48.2 | 20 / 148.1 |
| set1 | Code-heavy | startup | 2 | 20 / 45.3 | 20 / 148.6 |
| set1 | Code-heavy | startup | D | 20 / 46.5 | 20 / 131.9 |
| set1 | Code-heavy | compiled first | 0 | 20 / 45.7 | 20 / 133.1 |
| set1 | Code-heavy | compiled first | 1 | 20 / 43.9 | 20 / 148.4 |
| set1 | Code-heavy | compiled first | 2 | 19 / 45.2 | 19 / 148.1 |
| set1 | Code-heavy | compiled first | D | 20 / 46.2 | 20 / 148.4 |
| set1 | Overview | startup | 0 | 20 / 48.9 | 20 / 131.9 |
| set1 | Overview | startup | 1 | 20 / 48.8 | 20 / 131.9 |
| set1 | Overview | startup | 2 | 20 / 48.9 | 20 / 131.4 |
| set1 | Overview | startup | D | 20 / 48.8 | 20 / 131.8 |
| set1 | Overview | compiled first | 0 | 20 / 48.8 | 20 / 132 |
| set1 | Overview | compiled first | 1 | 20 / 48.7 | 20 / 131.7 |
| set1 | Overview | compiled first | 2 | 20 / 48.5 | 20 / 131.4 |
| set1 | Overview | compiled first | D | 20 / 48.7 | 20 / 131.5 |
| set1 | Small | startup | 0 | 20 / 49 | 20 / 132.1 |
| set1 | Small | startup | 1 | 20 / 48.7 | 20 / 132.3 |
| set1 | Small | startup | 2 | 19 / 48.5 | 19 / 132.1 |
| set1 | Small | startup | D | 20 / 48.6 | 20 / 132 |
| set1 | Small | compiled first | 0 | 20 / 48.9 | 20 / 131.9 |
| set1 | Small | compiled first | 1 | 20 / 48.6 | 20 / 132 |
| set1 | Small | compiled first | 2 | 20 / 48.6 | 20 / 131.9 |
| set1 | Small | compiled first | D | 20 / 49 | 20 / 132.2 |
| set2 | Code-heavy | startup | 0 | 20 / 45.8 | 20 / 148.6 |
| set2 | Code-heavy | startup | 1 | 20 / 46 | 20 / 149.7 |
| set2 | Code-heavy | startup | 2 | 19 / 45.5 | 19 / 148.5 |
| set2 | Code-heavy | startup | D | 20 / 46.2 | 20 / 148.4 |
| set2 | Code-heavy | compiled first | 0 | 20 / 45.9 | 20 / 148.8 |
| set2 | Code-heavy | compiled first | 1 | 20 / 45.8 | 20 / 148.3 |
| set2 | Code-heavy | compiled first | 2 | 20 / 45 | 20 / 132.3 |
| set2 | Code-heavy | compiled first | D | 20 / 46.8 | 20 / 148.2 |
| set2 | Overview | startup | 0 | 20 / 48.4 | 20 / 148.1 |
| set2 | Overview | startup | 1 | 20 / 48.7 | 20 / 131.9 |
| set2 | Overview | startup | 2 | 20 / 48.1 | 20 / 131.8 |
| set2 | Overview | startup | D | 20 / 48.9 | 20 / 132.4 |
| set2 | Overview | compiled first | 0 | 20 / 48.3 | 20 / 148 |
| set2 | Overview | compiled first | 1 | 20 / 48.9 | 20 / 149.4 |
| set2 | Overview | compiled first | 2 | 20 / 48.4 | 20 / 148 |
| set2 | Overview | compiled first | D | 20 / 48.8 | 20 / 131.8 |
| set2 | Small | startup | 0 | 20 / 48.8 | 20 / 148 |
| set2 | Small | startup | 1 | 20 / 48.7 | 20 / 132 |
| set2 | Small | startup | 2 | 20 / 48.7 | 20 / 132.9 |
| set2 | Small | startup | D | 20 / 48.4 | 20 / 132.3 |
| set2 | Small | compiled first | 0 | 20 / 48.7 | 20 / 132.6 |
| set2 | Small | compiled first | 1 | 20 / 48.5 | 20 / 132 |
| set2 | Small | compiled first | 2 | 20 / 48.6 | 20 / 131.9 |
| set2 | Small | compiled first | D | 20 / 48.8 | 20 / 132 |
| confirmation | Code-heavy | startup | 0 | 20 / 45.7 | 20 / 133.3 |
| confirmation | Code-heavy | startup | 1 | 20 / 46.2 | 20 / 132.2 |
| confirmation | Code-heavy | compiled first | 0 | 20 / 45.9 | 20 / 132.5 |
| confirmation | Code-heavy | compiled first | 1 | 20 / 266.5 | 20 / 132.5 |
| confirmation | Overview | startup | 0 | 20 / 48.3 | 20 / 132.1 |
| confirmation | Overview | startup | 1 | 20 / 49 | 20 / 116.2 |
| confirmation | Overview | compiled first | 0 | 20 / 48.9 | 20 / 132 |
| confirmation | Overview | compiled first | 1 | 20 / 48.9 | 20 / 116.4 |
| confirmation | Small | startup | 0 | 20 / 48.9 | 20 / 132 |
| confirmation | Small | startup | 1 | 20 / 48.8 | 20 / 132 |
| confirmation | Small | compiled first | 0 | 20 / 48.7 | 20 / 131.9 |
| confirmation | Small | compiled first | 1 | 20 / 48.7 | 20 / 131.9 |

## Evicted colouring and cached Show code

| Set | Fixture | Cache | Variant | Evicted coloured n / p50 | Reopened readable n / p50 |
| --- | --- | --- | --- | ---: | ---: |
| set1 | Code-heavy | startup | 0 | 20 / 716.2 | 20 / 13.8 |
| set1 | Code-heavy | startup | 1 | 20 / 616 | 20 / 14 |
| set1 | Code-heavy | startup | 2 | 20 / 1366.4 | 20 / 12.8 |
| set1 | Code-heavy | startup | D | — | 20 / 14.1 |
| set1 | Code-heavy | compiled first | 0 | 19 / 665.9 | 20 / 14.3 |
| set1 | Code-heavy | compiled first | 1 | 20 / 648.8 | 20 / 13.7 |
| set1 | Code-heavy | compiled first | 2 | 19 / 1364.8 | 20 / 13 |
| set1 | Code-heavy | compiled first | D | — | 20 / 14.3 |
| set1 | Overview | startup | 0 | 20 / 102.4 | 20 / 15.4 |
| set1 | Overview | startup | 1 | 20 / 132.4 | 20 / 15.3 |
| set1 | Overview | startup | 2 | 20 / 155.2 | 20 / 15.3 |
| set1 | Overview | startup | D | — | 20 / 14.9 |
| set1 | Overview | compiled first | 0 | 20 / 102.3 | 20 / 15.4 |
| set1 | Overview | compiled first | 1 | 20 / 132.4 | 20 / 15.2 |
| set1 | Overview | compiled first | 2 | 20 / 160.5 | 20 / 15.2 |
| set1 | Overview | compiled first | D | — | 20 / 14.9 |
| set1 | Small | startup | 0 | — | 20 / 15.6 |
| set1 | Small | startup | 1 | — | 20 / 15.4 |
| set1 | Small | startup | 2 | — | 19 / 15.3 |
| set1 | Small | startup | D | — | 20 / 15.5 |
| set1 | Small | compiled first | 0 | — | 20 / 15.5 |
| set1 | Small | compiled first | 1 | — | 20 / 15.4 |
| set1 | Small | compiled first | 2 | — | 20 / 15.5 |
| set1 | Small | compiled first | D | — | 20 / 15.7 |
| set2 | Code-heavy | startup | 0 | 20 / 732.3 | 20 / 13.6 |
| set2 | Code-heavy | startup | 1 | 20 / 665.1 | 20 / 13.8 |
| set2 | Code-heavy | startup | 2 | 19 / 1420.4 | 20 / 12.2 |
| set2 | Code-heavy | startup | D | — | 20 / 14.2 |
| set2 | Code-heavy | compiled first | 0 | 20 / 781.8 | 20 / 13.6 |
| set2 | Code-heavy | compiled first | 1 | 20 / 648.8 | 20 / 13.8 |
| set2 | Code-heavy | compiled first | 2 | 20 / 1349.5 | 20 / 12 |
| set2 | Code-heavy | compiled first | D | — | 20 / 14.3 |
| set2 | Overview | startup | 0 | 20 / 103.5 | 20 / 15.2 |
| set2 | Overview | startup | 1 | 20 / 132.5 | 20 / 15.1 |
| set2 | Overview | startup | 2 | 20 / 161.9 | 20 / 15.1 |
| set2 | Overview | startup | D | — | 20 / 14.7 |
| set2 | Overview | compiled first | 0 | 20 / 118.6 | 20 / 15.4 |
| set2 | Overview | compiled first | 1 | 20 / 150 | 20 / 15.1 |
| set2 | Overview | compiled first | 2 | 20 / 182.3 | 20 / 15.2 |
| set2 | Overview | compiled first | D | — | 20 / 14.9 |
| set2 | Small | startup | 0 | — | 20 / 15.5 |
| set2 | Small | startup | 1 | — | 20 / 15.2 |
| set2 | Small | startup | 2 | — | 20 / 15.4 |
| set2 | Small | startup | D | — | 20 / 15.6 |
| set2 | Small | compiled first | 0 | — | 20 / 15.5 |
| set2 | Small | compiled first | 1 | — | 20 / 15.3 |
| set2 | Small | compiled first | 2 | — | 20 / 15.3 |
| set2 | Small | compiled first | D | — | 20 / 15.5 |
| confirmation | Code-heavy | startup | 0 | 20 / 664.6 | 20 / 14.6 |
| confirmation | Code-heavy | startup | 1 | 20 / 599.1 | 20 / 13.9 |
| confirmation | Code-heavy | compiled first | 0 | 20 / 697.5 | 20 / 14.3 |
| confirmation | Code-heavy | compiled first | 1 | 20 / 549.7 | 20 / 13.8 |
| confirmation | Overview | startup | 0 | 20 / 102 | 20 / 15.3 |
| confirmation | Overview | startup | 1 | 20 / 116 | 20 / 15.3 |
| confirmation | Overview | compiled first | 0 | 20 / 82.5 | 20 / 15.5 |
| confirmation | Overview | compiled first | 1 | 20 / 116.1 | 20 / 15.5 |
| confirmation | Small | startup | 0 | — | 20 / 15.7 |
| confirmation | Small | startup | 1 | — | 20 / 15.6 |
| confirmation | Small | compiled first | 0 | — | 20 / 15.7 |
| confirmation | Small | compiled first | 1 | — | 20 / 15.6 |

## Complete measurements

- set1: [all metric tables](set1/metrics.md), [CSV](set1/metrics.csv), [raw metric observations](set1/report.json), [summary](set1/summary.json).
- set2: [all metric tables](set2/metrics.md), [CSV](set2/metrics.csv), [raw metric observations](set2/report.json), [summary](set2/summary.json).
- confirmation: [all metric tables](confirmation/metrics.md), [CSV](confirmation/metrics.csv), [raw metric observations](confirmation/report.json), [summary](confirmation/summary.json).

These include initialization/tokenization, transport, DOM work, long tasks, edits, loader exposure, observed CPU, worker heaps, caches and peak/settled process RSS. Raw load observations and qualifications remain with each trial. Sources and fixtures are frozen from Set 1, and original complete reports/traces remain in the owned temporary run with hashes in provenance.json.

Colour distributions contain completed colours only. Per-action plain fallback outcomes and settling times remain in report.json; do not interpret completed-only p95 as bounding those failures. Independently observed visibility/readability is retained even if colours fail. Edit visibleMs observes exact updated DOM; highlightSettledMs observes the completion/fallback promise and does not independently prove coloured pixels or a subsequent edit paint.

Variant 0 is asynchronous Rust Syntect/Oniguruma; 1 is highlight.js; 2 is TextMate/Oniguruma WASM; D is the plain diagnostic. D and code-free docs have no successful colour-completion metric. See the retained preflight report for geometry/raster, token-role and freshness findings. Faster timing does not make those checks pass.
