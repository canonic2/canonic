# Compiler speed run 2026-10-11T16:26:57.774Z

Samples per cell: 10, warmups: 2.

One-minute load average at sample start: 4.10–13.55 (threshold 16; samples started above it after waiting 10 minutes: 0).

| Fixture | Shape | Variant | p50 ms | p95 ms | max ms | Tree RSS p50 MB | JS MB | Edit visible | Failures |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| large | B1 | esbuild | 289 | 300 | 300 | 299 | 5.17 | — | 0 |
| large | B1 | rolldown | 442 | 464 | 464 | 338 | 4.89 | — | 0 |
| large | B1 | rspack | 227 | 236 | 236 | 215 | 9.35 | — | 0 |
| large | B1 | shipping | 1298 | 1324 | 1324 | 636 | 5.17 | — | 0 |
| large | B1 | utoo | 1392 | 1486 | 1486 | 433 | 12.00 | — | 0 |
| large | B2 | esbuild | 549 | 601 | 601 | 518 | 7.15 | — | 0 |
| large | B2 | rolldown | 585 | 604 | 604 | 551 | 6.77 | — | 0 |
| large | B2 | rspack | 424 | 474 | 474 | 320 | 21.92 | — | 0 |
| large | B2 | utoo | 6978 | 7928 | 7928 | 844 | 32.73 | — | 0 |
| large | B3 | esbuild | 435 | 479 | 479 | 768 | — | yes | 0 |
| large | B3 | rolldown | 560 | 591 | 591 | 1013 | — | yes | 0 |
| large | B3 | rspack | 145 | 150 | 150 | 399 | — | yes | 0 |
| large | B3 | shipping | 1152 | 1198 | 1198 | 795 | — | yes | 0 |
| large | B3 | utoo | 6737 | 7157 | 7157 | 970 | — | yes | 0 |
| large | B4 | esbuild | 446 | 469 | 469 | 744 | — | yes | 0 |
| large | B4 | rolldown | 567 | 625 | 625 | 1005 | — | yes | 0 |
| large | B4 | rspack | 148 | 154 | 154 | 398 | — | yes | 0 |
| large | B4 | shipping | 1175 | 1235 | 1235 | 816 | — | yes | 0 |
| large | B4 | utoo | 6553 | 7854 | 7854 | 935 | — | yes | 0 |
| large | B5 | rspack | 449 | 462 | 462 | 760 | — | yes | 0 |
| large | B5 | utoo | 14312 | 15339 | 15339 | 3453 | — | yes | 0 |
| large | B6 | rolldown | 29 | 31 | 31 | 946 | — | yes | 0 |
| large | B6 | rspack | 149 | 154 | 154 | 386 | — | yes | 0 |
| small | B1 | esbuild | 93 | 95 | 95 | 77 | 1.20 | — | 0 |
| small | B1 | rolldown | 147 | 152 | 152 | 87 | 0.98 | — | 0 |
| small | B1 | rspack | 107 | 114 | 114 | 92 | 1.30 | — | 0 |
| small | B1 | shipping | 179 | 188 | 188 | 111 | 1.20 | — | 0 |
| small | B1 | utoo | 615 | 642 | 642 | 201 | 1.27 | — | 0 |
| small | B2 | esbuild | 95 | 97 | 97 | 79 | 1.21 | — | 0 |
| small | B2 | rolldown | 149 | 157 | 157 | 91 | 1.00 | — | 0 |
| small | B2 | rspack | 114 | 120 | 120 | 98 | 1.37 | — | 0 |
| small | B2 | utoo | 627 | 653 | 653 | 208 | 1.81 | — | 0 |
| small | B3 | esbuild | 22 | 24 | 24 | 79 | — | yes | 0 |
| small | B3 | rolldown | 115 | 127 | 127 | 127 | — | yes | 0 |
| small | B3 | rspack | 43 | 45 | 45 | 112 | — | yes | 0 |
| small | B3 | shipping | 119 | 125 | 125 | 134 | — | yes | 0 |
| small | B3 | utoo | 462 | 487 | 487 | 266 | — | yes | 0 |
| small | B4 | esbuild | 22 | 25 | 25 | 78 | — | yes | 0 |
| small | B4 | rolldown | 115 | 124 | 124 | 129 | — | yes | 0 |
| small | B4 | rspack | 43 | 45 | 45 | 113 | — | yes | 0 |
| small | B4 | shipping | 120 | 125 | 125 | 133 | — | yes | 0 |
| small | B4 | utoo | 465 | 487 | 487 | 268 | — | yes | 0 |
| small | B5 | rspack | 37 | 37 | 37 | 89 | — | yes | 0 |
| small | B5 | utoo | 906 | 935 | 935 | 290 | — | yes | 0 |
| small | B6 | rolldown | 24 | 28 | 28 | 123 | — | yes | 0 |
| small | B6 | rspack | 45 | 47 | 47 | 114 | — | yes | 0 |

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

