# Compiler speed run 2026-10-11T08:41:19.936Z

Samples per cell: 10, warmups: 2.

| Fixture | Shape | Variant | p50 ms | p95 ms | max ms | Tree RSS p50 MB | JS MB | Edit visible | Failures |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| large | B1 | bun | 139 | 146 | 146 | 209 | 5.21 | — | 0 |
| large | B1 | esbuild | 332 | 339 | 339 | 287 | 5.17 | — | 0 |
| large | B1 | rolldown | 456 | 474 | 474 | 341 | 4.89 | — | 0 |
| large | B1 | rspack | 241 | 252 | 252 | 217 | 9.35 | — | 0 |
| large | B1 | shipping | 1407 | 1471 | 1471 | 643 | 5.17 | — | 0 |
| large | B1 | utoo | 1463 | 1525 | 1525 | 431 | 12.00 | — | 0 |
| large | B2 | bun | 330 | 347 | 347 | 488 | 7.87 | — | 0 |
| large | B2 | esbuild | 600 | 643 | 643 | 566 | 7.15 | — | 0 |
| large | B2 | rolldown | 613 | 633 | 633 | 553 | 6.77 | — | 0 |
| large | B2 | rspack | 454 | 510 | 510 | 320 | 21.92 | — | 0 |
| large | B2 | utoo | 7624 | 8018 | 8018 | 845 | 32.73 | — | 0 |
| large | B3 | bun | 234 | 263 | 263 | 674 | — | yes | 0 |
| large | B3 | esbuild | 475 | 487 | 487 | 773 | — | yes | 0 |
| large | B3 | rolldown | 583 | 620 | 620 | 1005 | — | yes | 0 |
| large | B3 | rspack | 338 | 358 | 358 | 449 | — | yes | 0 |
| large | B3 | shipping | 1239 | 1306 | 1306 | 788 | — | yes | 0 |
| large | B3 | utoo | 7016 | 7514 | 7514 | 982 | — | yes | 0 |
| large | B4 | bun | 242 | 253 | 253 | 678 | — | yes | 0 |
| large | B4 | esbuild | 462 | 505 | 505 | 760 | — | yes | 0 |
| large | B4 | rolldown | 586 | 624 | 624 | 1003 | — | yes | 0 |
| large | B4 | rspack | 344 | 350 | 350 | 443 | — | yes | 0 |
| large | B4 | shipping | 1277 | 1321 | 1321 | 752 | — | yes | 0 |
| large | B4 | utoo | 6792 | 8022 | 8022 | 952 | — | yes | 0 |
| large | B5 | rspack | 472 | 485 | 485 | 763 | — | yes | 0 |
| large | B5 | utoo | 15353 | 16335 | 16335 | 3453 | — | yes | 0 |
| large | B6 | rolldown | 27 | 30 | 30 | 944 | — | yes | 0 |
| large | B6 | rspack | 341 | 353 | 353 | 428 | — | yes | 0 |
| small | B1 | bun | 56 | 58 | 58 | 39 | 1.08 | — | 0 |
| small | B1 | esbuild | 116 | 117 | 117 | 69 | 1.20 | — | 0 |
| small | B1 | rolldown | 159 | 168 | 168 | 87 | 0.98 | — | 0 |
| small | B1 | rspack | 112 | 115 | 115 | 92 | 1.30 | — | 0 |
| small | B1 | shipping | 282 | 310 | 310 | 101 | 1.20 | — | 0 |
| small | B1 | utoo | 639 | 673 | 673 | 203 | 1.27 | — | 0 |
| small | B2 | bun | 58 | 61 | 61 | 41 | 1.10 | — | 0 |
| small | B2 | esbuild | 117 | 120 | 120 | 70 | 1.21 | — | 0 |
| small | B2 | rolldown | 160 | 171 | 171 | 91 | 1.00 | — | 0 |
| small | B2 | rspack | 117 | 120 | 120 | 98 | 1.37 | — | 0 |
| small | B2 | utoo | 650 | 665 | 665 | 213 | 1.81 | — | 0 |
| small | B3 | bun | 32 | 33 | 33 | 56 | — | yes | 0 |
| small | B3 | esbuild | 43 | 45 | 45 | 101 | — | yes | 0 |
| small | B3 | rolldown | 123 | 138 | 138 | 129 | — | yes | 0 |
| small | B3 | rspack | 16 | 16 | 16 | 104 | — | yes | 0 |
| small | B3 | shipping | 180 | 194 | 194 | 167 | — | yes | 0 |
| small | B3 | utoo | 476 | 498 | 498 | 266 | — | yes | 0 |
| small | B4 | bun | 32 | 33 | 33 | 55 | — | yes | 0 |
| small | B4 | esbuild | 42 | 44 | 44 | 101 | — | yes | 0 |
| small | B4 | rolldown | 121 | 138 | 138 | 128 | — | yes | 0 |
| small | B4 | rspack | 16 | 17 | 17 | 104 | — | yes | 0 |
| small | B4 | shipping | 179 | 183 | 183 | 168 | — | yes | 0 |
| small | B4 | utoo | 475 | 494 | 494 | 270 | — | yes | 0 |
| small | B5 | rspack | 37 | 39 | 39 | 88 | — | yes | 0 |
| small | B5 | utoo | 941 | 950 | 950 | 298 | — | yes | 0 |
| small | B6 | rolldown | 24 | 27 | 27 | 124 | — | yes | 0 |
| small | B6 | rspack | 18 | 19 | 19 | 108 | — | yes | 0 |

## Render parity

### small

- esbuild: 10 match, 0 differ; locale chunks 0, locale in an entry: no
- rolldown: 10 match, 0 differ; locale chunks 0, locale in an entry: no
- bun: 10 match, 0 differ; locale chunks 0, locale in an entry: no
- rspack: 10 match, 0 differ; locale chunks 0, locale in an entry: no
- utoo: 10 match, 0 differ; locale chunks 0, locale in an entry: no

### large

- esbuild: 80 match, 0 differ; locale chunks 1, locale in an entry: no
- rolldown: 80 match, 0 differ; locale chunks 1, locale in an entry: no
- bun: 80 match, 0 differ; locale chunks 1, locale in an entry: no
- rspack: 80 match, 0 differ; locale chunks 1, locale in an entry: no
- utoo: 80 match, 0 differ; locale chunks 1, locale in an entry: no

