# Page listing measurements

Dated observations and comparison evidence. [Performance](../system/performance.md)
owns requirements and measurement semantics; [research ownership](README.md)
defines this record's evidence limits. Measurements below retain their original
conditions and do not establish acceptance for an untested build or host.

## Page listing

The server's [native page index](../preview-pipeline/page-index.md) lists pages independently of
the compilers and the Node evaluator. Its saved-state, evaluation and failure contracts are
owned there. The measurements below distinguish the lab prototype from the
shipping-server boundary.

The [instant first page lab](../../labs/instant-first-page/README.md#stage-1-report)
holds the index design comparison. Process startup, adapter ordering and
configuration assembly belong to the shipping-server boundary below.
A newly placed macOS executable may also incur a one-time system launch check;
report that state separately from subsequent launches.

Shipping-server probe, 2026-10-15 at about 20:54 EDT: Node 24.12.0, Intel
macOS, release native executable, Acme seed 1. Each fixture had one cold read
and three warm reads, with a fresh server instance per read and no eager
pre-warming. Framework dependencies were not installed: listing evaluates
definitions and config, not their preview sources. The bundled runtime was
already extracted. One-minute host load was 7.79–8.05; these exploratory
samples are not a lab run set or a canvas-paint measurement.

| Definitions | Cold HTTP config | Warm native work | Warm `previews.indexed` | Warm `config.resolved` | Warm HTTP config |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | 349 ms | 2 ms | 9 ms | 9 ms | 12 ms |
| 42 | 251 ms | 3 ms | 10 ms | 10 ms | 13 ms |
| 602 | 329 ms | 15 ms | 27 ms | 29 ms | 40 ms |
| 10,002 | 2,435 ms | 162 ms | 243 ms | 295 ms | 467 ms |

Warm columns are medians of the three reads. The large fixtures include two
broken previews, whose definitions still list. The 10,002-definition warm
HTTP responses ranged from 393 to 532 ms. Every config listed all definitions
without starting a compile worker; unchanged reads parsed and evaluated none.
Cold reads also evaluated `workbench.config.ts`, and the 10,002-definition
fixture evaluated 500 computed definitions. Runtime preparation uses a second
native pass, reusing literal results saved by the first pass.

Measured source: public base `6d9d0b5`, private base `bd86d29b`, plus the
uncommitted page-index implementation. SHA-256 prefixes identify the measured
files: server `aac7f3b3951f`, compiler `4e9f7f3cadc7`, evaluator
`e3c7efaaacb7`, TypeScript adapter `7a464917b48e`, native index application
`0b467d3c834e`, release executable `b5459121478c`.

A loaded-host repeat at about 21:00 EDT used the installed release's executable. One-minute host load was
13.27–20.16. Warm HTTP medians were 35, 44, 48 and 1,198 ms for the same four
fixtures; the 10,002-definition native work had a median of 181 ms and its
HTTP responses ranged from 596 to 1,499 ms. No compile worker started.
Single cold HTTP reads were 4,062, 3,373, 391 and 9,074 ms. Scheduling and
process startup dominate some samples; this loaded-host repeat is correctness
and diagnostic evidence, not a controlled speed comparison. Its measured
SHA-256 prefixes are adapter `9c0efd4704a6`, native application `c0189a1d7c12`
and executable `4ea1d096abc4`; server, compiler and evaluator match the first
probe.

The 200 ms complete-list target is not met on the 10,002-definition fixture.
Process startup, saved-answer transport, collection assembly and the full
config response add to native work. Canvas first paint and the VS Code
webview boundary require a separate measurement; these server probes do not
establish that target. The server's resident index removes the process
per request and, while nothing changed, the disk check; it has not been
measured yet ([server](../system/server.md#the-resident-index-and-watcher)).
