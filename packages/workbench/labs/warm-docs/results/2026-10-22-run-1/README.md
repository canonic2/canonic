# Warm docs run set: run-1

Started 2026-10-22T23:45:41.944Z, finished 2026-10-23T03:05:24.431Z.
Host: darwin 24.6.0 x64, Intel(R) Xeon(R) W-2140B CPU @ 3.20GHz ×16, 64 GB, Now drawing from 'AC Power'; Node v24.12.0; Chrome/155.0.8059.85. Source revision d6f5dc4da170465b5795f8360bd0b56fd76276e3; file hashes in report.json.
Load average (1 min) at session start: p50 16.61, max 295.42.

Times are milliseconds from selection to the runner's own observation of the page, nearest-rank percentiles. Measured sessions only.

## acme

| Cell | control | early | single | pool | nofonts | unisolated |
| --- | --- | --- | --- | --- | --- | --- |
| first | 131 / 192 / 1164 (n=160) | 120 / 213 / 406 (n=160) | 80 / 117 / 162 (n=160) | 79 / 162 / 212 (n=160) | 129 / 187 / 363 (n=160) | 80 / 115 / 304 (n=160) |
| retained | 26 / 34 / 41 (n=80) | 25 / 37 / 39 (n=80) | 79 / 97 / 131 (n=80) | 61 / 77 / 225 (n=80) | 27 / 35 / 37 (n=80) | 79 / 113 / 114 (n=80) |
| evicted | 124 / 163 / 477 (n=80) | 113 / 194 / 305 (n=80) | 79 / 97 / 112 (n=80) | 78 / 111 / 416 (n=80) | 129 / 183 / 278 (n=80) | 79 / 97 / 116 (n=80) |
| design-return | 32 / 32 / 33 (n=20) | 31 / 35 / 42 (n=20) | 64 / 66 / 79 (n=20) | 65 / 79 / 88 (n=20) | 31 / 33 / 42 (n=20) | 65 / 66 / 66 (n=20) |
| edit-markdown | 60 / 176 / 178 (n=20) | 61 / 324 / 359 (n=20) | 49 / 232 / 479 (n=20) | 61 / 113 / 278 (n=20) | 62 / 111 / 161 (n=20) | 60 / 196 / 281 (n=20) |
| edit-example | 150 / 182 / 253 (n=20) | 136 / 719 / 755 (n=20) | 149 / 168 / 266 (n=20) | 150 / 243 / 267 (n=20) | 147 / 531 / 832 (n=20) | 149 / 169 / 196 (n=20) |
| to-design | 78 / 128 / 140 (n=20) | 72 / 139 / 143 (n=20) | 61 / 114 / 181 (n=20) | 111 / 158 / 204 (n=20) | 106 / 152 / 164 (n=20) | 95 / 114 / 123 (n=20) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 131 | 102 | 61 | 134 | 0 | 22 | 159 |
| early | 120 | 91 | 83 | 124 | 0 | 21 | 160 |
| single | 80 | 64 | 47 | 81 | 0 | 4 | 13 |
| pool | 79 | 63 | 47 | 80 | 0 | 5 | 13 |
| nofonts | 116 | 93 | 63 | 134 | 0 | 22 | 159 |
| unisolated | 80 | 65 | 47 | 81 | 0 | 4 | 13 |

## small

| Cell | control | early | single | pool | nofonts | unisolated |
| --- | --- | --- | --- | --- | --- | --- |
| first | 82 / 170 / 182 (n=50) | 80 / 114 / 119 (n=50) | 48 / 58 / 83 (n=50) | 80 / 114 / 210 (n=50) | 97 / 269 / 332 (n=50) | 47 / 64 / 96 (n=50) |
| retained | 30 / 31 / 38 (n=40) | 29 / 31 / 39 (n=40) | 48 / 61 / 65 (n=40) | 45 / 47 / 47 (n=40) | 30 / 34 / 39 (n=40) | 48 / 56 / 60 (n=40) |
| evicted | 91 / 132 / 159 (n=20) | 79 / 110 / 113 (n=20) | 48 / 57 / 61 (n=20) | 46 / 53 / 56 (n=20) | 97 / 177 / 604 (n=20) | 48 / 56 / 61 (n=20) |
| edit-markdown | 49 / 80 / 113 (n=20) | 63 / 75 / 168 (n=20) | 48 / 52 / 64 (n=20) | 48 / 77 / 82 (n=20) | 49 / 80 / 82 (n=20) | 47 / 58 / 74 (n=20) |
| edit-example | 115 / 152 / 332 (n=20) | 100 / 265 / 398 (n=20) | 102 / 281 / 313 (n=20) | 115 / 442 / 583 (n=20) | 118 / 449 / 665 (n=20) | 115 / 264 / 395 (n=20) |
| cold | 454 / 587 / 594 (n=20) | 468 / 604 / 616 (n=20) | 450 / 545 / 604 (n=20) | 463 / 634 / 640 (n=20) | 434 / 582 / 591 (n=20) | 444 / 513 / 557 (n=20) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 82 | 65 | 48 | 82 | 0 | 16 | 88 |
| early | 80 | 64 | 61 | 81 | 0 | 15 | 89 |
| single | 48 | 31 | 17 | 48 | 0 | 4 | 9 |
| pool | 80 | 63 | 46 | 80 | 0 | 18 | 96 |
| nofonts | 97 | 80 | 60 | 97 | 0 | 17 | 88 |
| unisolated | 47 | 31 | 22 | 48 | 0 | 4 | 9 |

## heavy

| Cell | control | early | single | pool | nofonts | unisolated |
| --- | --- | --- | --- | --- | --- | --- |
| first | 148 / 348 / 1049 (n=290) | 141 / 313 / 914 (n=290) | 93 / 133 / 292 (n=290) | 94 / 161 / 333 (n=290) | 146 / 255 / 521 (n=290) | 93 / 166 / 389 (n=290) |
| retained | 29 / 34 / 57 (n=110) | 29 / 34 / 42 (n=110) | 88 / 130 / 216 (n=110) | 46 / 75 / 97 (n=110) | 29 / 33 / 55 (n=110) | 89 / 161 / 618 (n=110) |
| evicted | 154 / 355 / 669 (n=130) | 132 / 222 / 622 (n=130) | 95 / 130 / 166 (n=130) | 85 / 141 / 360 (n=130) | 146 / 231 / 396 (n=130) | 95 / 177 / 421 (n=130) |
| lens-switch | 41 / 212 / 373 (n=40) | 32 / 175 / 257 (n=40) | 63 / 247 / 495 (n=40) | 57 / 228 / 305 (n=40) | 42 / 230 / 344 (n=40) | 60 / 242 / 347 (n=40) |
| edit-markdown | 80 / 214 / 349 (n=20) | 94 / 279 / 600 (n=20) | 64 / 128 / 128 (n=20) | 80 / 129 / 363 (n=20) | 79 / 295 / 353 (n=20) | 89 / 160 / 407 (n=20) |
| edit-example | 146 / 592 / 645 (n=20) | 146 / 484 / 561 (n=20) | 157 / 845 / 963 (n=20) | 143 / 196 / 328 (n=20) | 146 / 196 / 212 (n=20) | 156 / 176 / 181 (n=20) |
| walk | 146 / 297 / 861 (n=500) | 142 / 290 / 553 (n=500) | 81 / 145 / 477 (n=500) | 81 / 159 / 529 (n=500) | 146 / 304 / 910 (n=500) | 111 / 162 / 490 (n=500) |
| cold | 988 / 1199 / 1379 (n=20) | 995 / 1233 / 1708 (n=20) | 1007 / 1234 / 1278 (n=20) | 1003 / 1433 / 1435 (n=20) | 957 / 1127 / 1208 (n=20) | 1058 / 1232 / 1565 (n=20) |

Cells read p50 / p95 / max.

Secondary, first visits (p50): text visible, examples settled, fonts loaded, colours settled, long-task time, requests, kB.

| Variant | Text | Examples | Fonts | Colours | Long tasks | Requests | kB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| control | 148 | 131 | 63 | 302 | 0 | 26 | 2673 |
| early | 141 | 118 | 78 | 295 | 0 | 24 | 2675 |
| single | 93 | 67 | 30 | 245 | 0 | 6 | 2469 |
| pool | 94 | 69 | 30 | 246 | 0 | 6 | 2471 |
| nofonts | 146 | 129 | 64 | 298 | 0 | 26 | 2673 |
| unisolated | 93 | 74 | 30 | 246 | 0 | 6 | 2469 |

Long walk, then 10 s idle (p50 per variant): JS heap MB, renderer RSS MB, idle renderer CPU s, docs documents.

| Variant | Heap | RSS | Idle CPU | Documents |
| --- | --- | --- | --- | --- |
| control | 10.2 | 797 | 1.12 | 4 |
| early | 10.2 | 811 | 1.14 | 4 |
| single | 22.2 | 778 | 1.25 | 3 |
| pool | 22.8 | 786 | 1.25 | 4 |
| nofonts | 10.2 | 805 | 1.16 | 4 |
| unisolated | 61.5 | 1043 | 5.25 | 3 |

## Correctness

| Fixture | Variant | Sessions | Failed checks | Session failures | Cleanup leftovers | Console messages not seen in control | Parity: max differing pixels |
| --- | --- | --- | --- | --- | --- | --- | --- |
| acme | control | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | early | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | single | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | pool | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | nofonts | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| acme | unisolated | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| small | control | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| small | early | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| small | single | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| small | pool | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| small | nofonts | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| small | unisolated | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | control | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | early | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | single | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | pool | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | nofonts | 11 | none | none | none | none | 0.039% (control noise 0.039%) |
| heavy | unisolated | 11 | first docs/p02.md: leaks listener:p01 interval:p01 body-class:p01 portal:p01 style:p01 global:p01; first docs/p03.md: leaks listener:p01 listener:p02 interval:p01 interval:p02 body-class:p01 body-class:p02 portal:p01 portal:p02 style:p01 style:p02 global:p01 global:p02; first docs/p04.md: leaks listener:p01 listener:p02 listener:p03 interval:p01 interval:p02 interval:p03 body-class:p01 body-class:p02 body-class:p03 portal:p01 portal:p02 portal:p03 style:p01 style:p02 style:p03 global:p01 global:p02 global:p03; retained docs/p02.md: leaks listener:p01 listener:p03 listener:p04 interval:p01 interval:p03 interval:p04 body-class:p01 body-class:p03 body-class:p04 portal:p01 portal:p03 portal:p04 style:p01 style:p03 style:p04 global:p01 global:p03 global:p04; first docs/p05.md: leaks listener:p01 listener:p02 listener:p03 listener:p04 interval:p01 interval:p02 interval:p03 interval:p04 body-class:p01 body-class:p02 body-class:p03 body-class:p04 portal:p01 portal:p02 portal:p03 portal:p04 style:p01 style:p02 style:p03 style:p04 global:p01 global:p02 global:p03 global:p04; first docs/p06.md: leaks listener:p01 listener:p02 listener:p03 listener:p04 listener:p05 interval:p01 interval:p02 interval:p03 interval:p04 interval:p05 body-class:p01 body-class:p02 body-class:p03 body-class:p04 body-class:p05 portal:p01 portal:p02 portal:p03 portal:p04 portal:p05 style:p01 style:p02 style:p03 style:p04 style:p05 global:p01 global:p02 global:p03 global:p04 global:p05 (+1193) | none | none | none | 0.039% (control noise 0.039%) |


## Decision rule

- **early**: does not meet the rule: acme first: p50 120 vs 131 (gain 11 ms, 8%); acme evicted: p50 113 vs 124 (gain 11 ms, 9%); heavy first: p50 141 vs 148 (gain 6 ms, 4%); heavy evicted: p50 132 vs 154 (gain 22 ms, 14%); acme first: p95 213 vs 192; acme evicted: p95 194 vs 163; acme edit-markdown: p95 324 vs 176; acme edit-example: p95 719 vs 182; small edit-example: p95 265 vs 152; heavy edit-markdown: p95 279 vs 214.
- **single**: does not meet the rule: acme retained: p95 97 vs 34; acme design-return: p95 66 vs 32; acme edit-markdown: p95 232 vs 176; small retained: p95 61 vs 31; small edit-example: p95 281 vs 152; heavy retained: p95 130 vs 34; heavy lens-switch: p95 247 vs 212; heavy edit-example: p95 845 vs 592; long-walk heap 22.2 MB > control 10.2 MB.
- **pool**: does not meet the rule: acme retained: p95 77 vs 34; acme design-return: p95 79 vs 32; acme edit-example: p95 243 vs 182; small retained: p95 47 vs 31; small edit-example: p95 442 vs 152; heavy retained: p95 75 vs 34; heavy lens-switch: p95 228 vs 212; long-walk heap 22.8 MB > control 10.2 MB.
- early's first-visit gain relative to single: 21%, 12% (variant 1 wins instead at two thirds or more on both).
- early's first-visit gain relative to pool: 21%, 12% (variant 1 wins instead at two thirds or more on both).

Mechanical application of the agreed rule to this run set; the decision also needs the confirmation set and the owner.
