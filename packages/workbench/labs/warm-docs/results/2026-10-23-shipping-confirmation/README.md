# Warm docs run set: shipping-confirmation

Started 2026-10-23T14:52:42.608Z, finished 2026-10-23T15:02:19.323Z.
Host: darwin 24.6.0 x64, Intel(R) Xeon(R) W-2140B CPU @ 3.20GHz ×16, 64 GB, Now drawing from 'AC Power'; Node v24.12.0; Chrome/155.0.8059.85. Source revision ece48b46ba38cc7e076e1a1346f1fe9b57a9d80e; file hashes in report.json.
Load average (1 min) at session start: p50 6.53, max 8.29.

Times are milliseconds from selection to the runner's own observation of the page, nearest-rank percentiles. Measured sessions only.

## acme

| Cell | control |
| --- | --- |
| first | 63 / 116 / 138 (n=48) |
| retained | 28 / 30 / 30 (n=24) |
| evicted | 62 / 63 / 77 (n=24) |
| design-return | 31 / 32 / 32 (n=6) |
| edit-markdown | 47 / 129 / 129 (n=6) |
| edit-example | 148 / 167 / 167 (n=6) |
| to-design | 46 / 112 / 112 (n=6) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 63 | 46 | 30 | 64 | 0 | 4 | 13 |

## heavy

| Cell | control |
| --- | --- |
| first | 79 / 128 / 142 (n=87) |
| retained | 30 / 32 / 37 (n=33) |
| evicted | 65 / 114 / 130 (n=39) |
| lens-switch | 35 / 150 / 150 (n=12) |
| edit-markdown | 63 / 396 / 396 (n=6) |
| edit-example | 113 / 262 / 262 (n=6) |
| walk | 76 / 122 / 344 (n=150) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 79 | 57 | 31 | 231 | 0 | 6 | 2472 |

Long walk, then 10 s idle (p50 per variant): JS heap MB, renderer RSS MB, idle renderer CPU s, docs documents.

| Variant | Heap | RSS | Idle CPU | Documents |
| --- | --- | --- | --- | --- |
| control | 21.8 | 788 | 1.19 | 4 |

## Correctness

| Fixture | Variant | Sessions | Failed checks | Session failures | Cleanup leftovers | Console messages not seen in control | Parity: max differing pixels |
| --- | --- | --- | --- | --- | --- | --- | --- |
| acme | control | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | control | 4 | none | none | none | none | 0.039% (control noise 0.039%) |


## Decision rule


Mechanical application of the agreed rule to this run set; the decision also needs the confirmation set and the owner.
