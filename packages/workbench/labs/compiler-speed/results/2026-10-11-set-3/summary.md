# Compiler speed run 2026-10-11T14:25:20.934Z

Samples per cell: 10, warmups: 2.

One-minute load average at sample start: 4.53–15.98 (threshold 16; samples started above it after waiting 10 minutes: 0).

| Fixture | Shape | Variant | p50 ms | p95 ms | max ms | Tree RSS p50 MB | JS MB | Edit visible | Failures |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| large | B1 | esbuild | 307 | 324 | 324 | 288 | 5.17 | — | 0 |
| large | B1 | rolldown | 457 | 501 | 501 | 342 | 4.89 | — | 0 |
| large | B1 | rspack | 243 | 357 | 357 | 215 | 9.35 | — | 0 |
| large | B1 | shipping | 1344 | 1505 | 1505 | 626 | 5.17 | — | 0 |
| large | B1 | utoo | 1461 | 1907 | 1907 | 424 | 12.00 | — | 0 |
| large | B2 | esbuild | 596 | 650 | 650 | 551 | 7.15 | — | 0 |
| large | B2 | rolldown | 611 | 1231 | 1231 | 550 | 6.77 | — | 0 |
| large | B2 | rspack | 471 | 503 | 503 | 321 | 21.92 | — | 0 |
| large | B2 | utoo | 7422 | 8341 | 8341 | 846 | 32.73 | — | 0 |
| large | B3 | esbuild | 459 | 793 | 793 | 756 | — | yes | 0 |
| large | B3 | rolldown | 600 | 675 | 675 | 1009 | — | yes | 0 |
| large | B3 | rspack | 135 | 139 | 139 | 400 | — | yes | 0 |
| large | B3 | shipping | 1193 | 1418 | 1418 | 764 | — | yes | 0 |
| large | B3 | utoo | 6963 | 8294 | 8294 | 966 | — | yes | 0 |
| large | B4 | esbuild | 451 | 565 | 565 | 761 | — | yes | 0 |
| large | B4 | rolldown | 595 | 681 | 681 | 1014 | — | yes | 0 |
| large | B4 | rspack | 141 | 188 | 188 | 396 | — | yes | 0 |
| large | B4 | shipping | 1216 | 1292 | 1292 | 772 | — | yes | 0 |
| large | B4 | utoo | 7250 | 8201 | 8201 | 948 | — | yes | 0 |
| large | B5 | rspack | 468 | 670 | 670 | 764 | — | yes | 0 |
| large | B5 | utoo | 15037 | 17319 | 17319 | 3456 | — | yes | 0 |
| large | B6 | rolldown | 28 | 30 | 30 | 948 | — | yes | 0 |
| large | B6 | rspack | 140 | 143 | 143 | 383 | — | yes | 0 |
| small | B1 | esbuild | 96 | 103 | 103 | 76 | 1.20 | — | 0 |
| small | B1 | rolldown | 159 | 177 | 177 | 86 | 0.98 | — | 0 |
| small | B1 | rspack | 111 | 119 | 119 | 92 | 1.30 | — | 0 |
| small | B1 | shipping | 179 | 205 | 205 | 105 | 1.20 | — | 0 |
| small | B1 | utoo | 641 | 771 | 771 | 202 | 1.27 | — | 0 |
| small | B2 | esbuild | 96 | 105 | 105 | 79 | 1.21 | — | 0 |
| small | B2 | rolldown | 159 | 208 | 208 | 91 | 1.00 | — | 0 |
| small | B2 | rspack | 118 | 125 | 125 | 98 | 1.37 | — | 0 |
| small | B2 | utoo | 649 | 717 | 717 | 213 | 1.81 | — | 0 |
| small | B3 | esbuild | 23 | 39 | 39 | 78 | — | yes | 0 |
| small | B3 | rolldown | 126 | 154 | 154 | 129 | — | yes | 0 |
| small | B3 | rspack | 33 | 71 | 71 | 110 | — | yes | 0 |
| small | B3 | shipping | 121 | 177 | 177 | 134 | — | yes | 0 |
| small | B3 | utoo | 482 | 506 | 506 | 267 | — | yes | 0 |
| small | B4 | esbuild | 22 | 45 | 45 | 79 | — | yes | 0 |
| small | B4 | rolldown | 124 | 136 | 136 | 128 | — | yes | 0 |
| small | B4 | rspack | 34 | 35 | 35 | 110 | — | yes | 0 |
| small | B4 | shipping | 119 | 131 | 131 | 135 | — | yes | 0 |
| small | B4 | utoo | 477 | 528 | 528 | 268 | — | yes | 0 |
| small | B5 | rspack | 38 | 42 | 42 | 89 | — | yes | 0 |
| small | B5 | utoo | 926 | 1031 | 1031 | 289 | — | yes | 0 |
| small | B6 | rolldown | 25 | 28 | 28 | 124 | — | yes | 0 |
| small | B6 | rspack | 35 | 66 | 66 | 111 | — | yes | 0 |

## Render parity

### small

- esbuild: 10 match, 0 differ; locale chunks 0, locale in an entry: no
- rolldown: 10 match, 0 differ; locale chunks 0, locale in an entry: no
- rspack: 10 match, 0 differ; locale chunks 0, locale in an entry: no
- utoo: 10 match, 0 differ; locale chunks 0, locale in an entry: no

### large

- esbuild: 80 match, 0 differ; locale chunks 1, locale in an entry: no
- rolldown: 80 match, 0 differ; locale chunks 1, locale in an entry: no
- rspack: 80 match, 0 differ; locale chunks 1, locale in an entry: no
- utoo: 80 match, 0 differ; locale chunks 1, locale in an entry: no

