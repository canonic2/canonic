# Warm docs run set: stage2

Started 2026-10-23T04:08:04.389Z, finished 2026-10-23T05:08:11.235Z.
Host: darwin 24.6.0 x64, Intel(R) Xeon(R) W-2140B CPU @ 3.20GHz ×16, 64 GB, Now drawing from 'AC Power'; Node v24.12.0; Chrome/155.0.8059.85. Source revision d6f5dc4da170465b5795f8360bd0b56fd76276e3; file hashes in report.json.
Load average (1 min) at session start: p50 12.88, max 58.83.

Times are milliseconds from selection to the runner's own observation of the page, nearest-rank percentiles. Measured sessions only.

## acme

| Cell | control | pool | pool-r | pool-rp | pool-rpm | pool-rpmc |
| --- | --- | --- | --- | --- | --- | --- |
| first | 126 / 178 / 191 (n=48) | 78 / 170 / 258 (n=48) | 93 / 188 / 535 (n=48) | 78 / 138 / 160 (n=48) | 78 / 138 / 159 (n=48) | 73 / 265 / 417 (n=48) |
| retained | 26 / 34 / 38 (n=24) | 59 / 63 / 74 (n=24) | 27 / 29 / 39 (n=24) | 27 / 30 / 30 (n=24) | 28 / 29 / 30 (n=24) | 12 / 26 / 32 (n=24) |
| evicted | 110 / 160 / 161 (n=24) | 62 / 79 / 112 (n=24) | 77 / 112 / 126 (n=24) | 62 / 95 / 112 (n=24) | 62 / 80 / 125 (n=24) | 63 / 165 / 618 (n=24) |
| design-return | 31 / 33 / 33 (n=6) | 48 / 65 / 65 (n=6) | 31 / 32 / 32 (n=6) | 31 / 32 / 32 (n=6) | 31 / 33 / 33 (n=6) | 15 / 31 / 31 (n=6) |
| edit-markdown | 62 / 344 / 344 (n=6) | 64 / 178 / 178 (n=6) | 65 / 197 / 197 (n=6) | 66 / 326 / 326 (n=6) | 62 / 362 / 362 (n=6) | 92 / 395 / 395 (n=6) |
| edit-example | 133 / 170 / 170 (n=6) | 133 / 166 / 166 (n=6) | 149 / 182 / 182 (n=6) | 132 / 199 / 199 (n=6) | 149 / 181 / 181 (n=6) | 312 / 736 / 736 (n=6) |
| to-design | 61 / 111 / 111 (n=6) | 47 / 111 / 111 (n=6) | 62 / 112 / 112 (n=6) | 60 / 109 / 109 (n=6) | 54 / 111 / 111 (n=6) | 93 / 243 / 243 (n=6) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 126 | 96 | 58 | 129 | 0 | 22 | 159 |
| pool | 78 | 62 | 45 | 79 | 0 | 4 | 13 |
| pool-r | 93 | 67 | 46 | 94 | 0 | 5 | 13 |
| pool-rp | 78 | 60 | 46 | 78 | 0 | 4 | 13 |
| pool-rpm | 78 | 57 | 54 | 79 | 0 | 5 | 13 |
| pool-rpmc | 73 | 61 | 50 | 74 | 0 | 4 | 13 |

## heavy

| Cell | control | pool | pool-r | pool-rp | pool-rpm | pool-rpmc |
| --- | --- | --- | --- | --- | --- | --- |
| first | 134 / 276 / 405 (n=87) | 81 / 130 / 148 (n=87) | 96 / 258 / 441 (n=87) | 80 / 144 / 167 (n=87) | 80 / 127 / 131 (n=87) | 78 / 297 / 549 (n=87) |
| retained | 29 / 30 / 30 (n=33) | 46 / 48 / 50 (n=33) | 30 / 46 / 46 (n=33) | 29 / 38 / 48 (n=33) | 29 / 33 / 45 (n=33) | 14 / 39 / 46 (n=33) |
| evicted | 156 / 211 / 213 (n=39) | 80 / 128 / 131 (n=39) | 81 / 333 / 560 (n=39) | 80 / 113 / 131 (n=39) | 80 / 112 / 115 (n=39) | 64 / 424 / 472 (n=39) |
| lens-switch | 34 / 165 / 165 (n=12) | 47 / 213 / 213 (n=12) | 85 / 651 / 651 (n=12) | 33 / 150 / 150 (n=12) | 32 / 160 / 160 (n=12) | 26 / 155 / 155 (n=12) |
| edit-markdown | 76 / 346 / 346 (n=6) | 79 / 375 / 375 (n=6) | 65 / 143 / 143 (n=6) | 79 / 97 / 97 (n=6) | 64 / 84 / 84 (n=6) | 65 / 263 / 263 (n=6) |
| edit-example | 147 / 231 / 231 (n=6) | 129 / 146 / 146 (n=6) | 133 / 359 / 359 (n=6) | 128 / 144 / 144 (n=6) | 129 / 197 / 197 (n=6) | 129 / 162 / 162 (n=6) |
| walk | 133 / 246 / 308 (n=150) | 80 / 131 / 165 (n=150) | 80 / 145 / 213 (n=150) | 80 / 165 / 384 (n=150) | 79 / 130 / 147 (n=150) | 64 / 128 / 181 (n=150) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 134 | 116 | 57 | 287 | 0 | 26 | 2673 |
| pool | 81 | 64 | 30 | 232 | 0 | 6 | 2471 |
| pool-r | 96 | 65 | 31 | 247 | 0 | 6 | 2471 |
| pool-rp | 80 | 63 | 31 | 233 | 0 | 6 | 2471 |
| pool-rpm | 80 | 63 | 30 | 232 | 0 | 6 | 2471 |
| pool-rpmc | 78 | 65 | 31 | 229 | 0 | 6 | 2471 |

Long walk, then 10 s idle (p50 per variant): JS heap MB, renderer RSS MB, idle renderer CPU s, docs documents.

| Variant | Heap | RSS | Idle CPU | Documents |
| --- | --- | --- | --- | --- |
| control | 10.3 | 808 | 1.15 | 4 |
| pool | 22.9 | 801 | 1.24 | 4 |
| pool-r | 22.9 | 786 | 1.22 | 4 |
| pool-rp | 22.8 | 798 | 1.26 | 4 |
| pool-rpm | 23.0 | 801 | 1.21 | 4 |
| pool-rpmc | 22.9 | 798 | 1.22 | 4 |

## Correctness

| Fixture | Variant | Sessions | Failed checks | Session failures | Cleanup leftovers | Console messages not seen in control | Parity: max differing pixels |
| --- | --- | --- | --- | --- | --- | --- | --- |
| acme | control | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | pool | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | pool-r | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | pool-rp | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | pool-rpm | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | pool-rpmc | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | control | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | pool | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | pool-r | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | pool-rp | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | pool-rpm | 4 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | pool-rpmc | 4 | none | none | none | none | 0.039% (control noise 0.039%) |


## Decision rule

- **pool**: does not meet the rule: acme retained: p95 63 vs 34; acme design-return: p95 65 vs 33; heavy retained: p95 48 vs 30; heavy lens-switch: p95 213 vs 165; heavy edit-markdown: p95 375 vs 346; long-walk heap 22.9 MB > control 10.3 MB.
- **pool-r**: does not meet the rule: acme first: p50 93 vs 126 (gain 33 ms, 26%); heavy first: p50 96 vs 134 (gain 38 ms, 28%); acme first: p95 188 vs 178; acme edit-example: p95 182 vs 170; heavy retained: p95 46 vs 30; heavy evicted: p95 333 vs 211; heavy lens-switch: p95 651 vs 165; heavy edit-example: p95 359 vs 231; long-walk heap 22.9 MB > control 10.3 MB.
- **pool-rp**: does not meet the rule: acme edit-example: p95 199 vs 170; heavy retained: p95 38 vs 30; long-walk heap 22.8 MB > control 10.3 MB.
- **pool-rpm**: does not meet the rule: acme edit-markdown: p95 362 vs 344; acme edit-example: p95 181 vs 170; long-walk heap 23.0 MB > control 10.3 MB.
- **pool-rpmc**: does not meet the rule: acme first: p95 265 vs 178; acme edit-markdown: p95 395 vs 344; acme edit-example: p95 736 vs 170; heavy first: p95 297 vs 276; heavy retained: p95 39 vs 30; heavy evicted: p95 424 vs 211; long-walk heap 22.9 MB > control 10.3 MB.

Mechanical application of the agreed rule to this run set; the decision also needs the confirmation set and the owner.
