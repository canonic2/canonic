# Warm docs run set: shipping-final

Started 2026-10-23T15:10:22.191Z, finished 2026-10-23T15:15:12.243Z.
Host: darwin 24.6.0 x64, Intel(R) Xeon(R) W-2140B CPU @ 3.20GHz ×16, 64 GB, Now drawing from 'AC Power'; Node v24.12.0; Chrome/155.0.8059.85. Source revision ece48b46ba38cc7e076e1a1346f1fe9b57a9d80e; file hashes in report.json.
Load average (1 min) at session start: p50 6.83, max 8.82.

Times are milliseconds from selection to the runner's own observation of the page, nearest-rank percentiles. Measured sessions only.

## acme

| Cell | control |
| --- | --- |
| first | 63 / 123 / 123 (n=16) |
| retained | 28 / 29 / 29 (n=8) |
| evicted | 62 / 63 / 63 (n=8) |
| design-return | 32 / 33 / 33 (n=2) |
| edit-markdown | 46 / 112 / 112 (n=2) |
| edit-example | 132 / 149 / 149 (n=2) |
| to-design | 45 / 96 / 96 (n=2) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 63 | 46 | 30 | 64 | 0 | 4 | 13 |

## heavy

| Cell | control |
| --- | --- |
| first | 79 / 110 / 128 (n=29) |
| retained | 30 / 47 / 47 (n=11) |
| evicted | 80 / 86 / 86 (n=13) |
| lens-switch | 39 / 123 / 123 (n=4) |
| edit-markdown | 63 / 114 / 114 (n=2) |
| edit-example | 113 / 129 / 129 (n=2) |
| walk | 64 / 103 / 113 (n=50) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 79 | 61 | 31 | 230 | 0 | 6 | 2471 |

Long walk, then 10 s idle (p50 per variant): JS heap MB, renderer RSS MB, idle renderer CPU s, docs documents.

| Variant | Heap | RSS | Idle CPU | Documents |
| --- | --- | --- | --- | --- |
| control | 21.9 | 783 | 1.19 | 4 |

## Correctness

| Fixture | Variant | Sessions | Failed checks | Session failures | Cleanup leftovers | Console messages not seen in control | Parity: max differing pixels |
| --- | --- | --- | --- | --- | --- | --- | --- |
| acme | control | 2 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | control | 2 | none | none | none | none | 0.039% (control noise 0.039%) |


## Decision rule


Mechanical application of the agreed rule to this run set; the decision also needs the confirmation set and the owner.
