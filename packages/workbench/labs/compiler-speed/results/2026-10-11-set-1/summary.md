# Compiler speed run 2026-10-11T07:02:59.061Z

Samples per cell: 10, warmups: 2.

| Fixture | Shape | Variant | p50 ms | p95 ms | max ms | Tree RSS p50 MB | JS MB | Edit visible | Failures |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| large | B1 | bun | 145 | 151 | 151 | 162 | 5.21 | — | 0 |
| large | B1 | esbuild | 328 | 348 | 348 | 284 | 5.17 | — | 0 |
| large | B1 | rolldown | 476 | 496 | 496 | 341 | 4.89 | — | 0 |
| large | B1 | rspack | 252 | 263 | 263 | 208 | 9.35 | — | 0 |
| large | B1 | shipping | 1424 | 1571 | 1571 | 649 | 5.17 | — | 0 |
| large | B1 | utoo | 1505 | 1566 | 1566 | 434 | 12.00 | — | 0 |
| large | B2 | bun | 333 | 353 | 353 | 475 | 7.87 | — | 0 |
| large | B2 | esbuild | 625 | 659 | 659 | 561 | 7.15 | — | 0 |
| large | B2 | rolldown | 634 | 641 | 641 | 550 | 6.77 | — | 0 |
| large | B2 | rspack | 457 | 486 | 486 | 316 | 21.92 | — | 0 |
| large | B2 | utoo | 7322 | 8222 | 8222 | 844 | 32.73 | — | 0 |
| large | B3 | bun | 236 | 254 | 254 | 668 | — | yes | 0 |
| large | B3 | esbuild | 489 | 553 | 553 | 771 | — | yes | 0 |
| large | B3 | rolldown | 598 | 622 | 622 | 983 | — | yes | 0 |
| large | B3 | rspack | 342 | 354 | 354 | 421 | — | yes | 0 |
| large | B3 | shipping | 1251 | 1298 | 1298 | 783 | — | yes | 0 |
| large | B3 | utoo | 7287 | 8003 | 8003 | 961 | — | yes | 0 |
| large | B4 | bun | 247 | 281 | 281 | 664 | — | yes | 0 |
| large | B4 | esbuild | 480 | 535 | 535 | 773 | — | yes | 0 |
| large | B4 | rolldown | 590 | 620 | 620 | 988 | — | yes | 0 |
| large | B4 | rspack | 349 | 361 | 361 | 424 | — | yes | 0 |
| large | B4 | shipping | 1239 | 1315 | 1315 | 765 | — | yes | 0 |
| large | B4 | utoo | 7637 | 8225 | 8225 | 954 | — | yes | 0 |
| large | B5 | rspack | 480 | 490 | 490 | 717 | — | yes | 0 |
| large | B5 | utoo | 15945 | 16636 | 16636 | 3448 | — | yes | 0 |
| large | B6 | rolldown | 27 | 29 | 29 | 943 | — | yes | 0 |
| large | B6 | rspack | 343 | 358 | 358 | 423 | — | yes | 0 |
| small | B1 | bun | 57 | 59 | 59 | 0 | 1.08 | — | 0 |
| small | B1 | esbuild | 117 | 128 | 128 | 69 | 1.20 | — | 0 |
| small | B1 | rolldown | 158 | 174 | 174 | 83 | 0.98 | — | 0 |
| small | B1 | rspack | 117 | 134 | 134 | 51 | 1.30 | — | 0 |
| small | B1 | shipping | 286 | 297 | 297 | 100 | 1.20 | — | 0 |
| small | B1 | utoo | 644 | 671 | 671 | 199 | 1.27 | — | 0 |
| small | B2 | bun | 58 | 72 | 72 | 0 | 1.10 | — | 0 |
| small | B2 | esbuild | 120 | 124 | 124 | 70 | 1.21 | — | 0 |
| small | B2 | rolldown | 169 | 181 | 181 | 88 | 1.00 | — | 0 |
| small | B2 | rspack | 122 | 128 | 128 | 50 | 1.37 | — | 0 |
| small | B2 | utoo | 665 | 695 | 695 | 205 | 1.81 | — | 0 |
| small | B3 | bun | 32 | 35 | 35 | 53 | — | yes | 0 |
| small | B3 | esbuild | 43 | 46 | 46 | 101 | — | yes | 0 |
| small | B3 | rolldown | 131 | 148 | 148 | 109 | — | yes | 0 |
| small | B3 | rspack | 15 | 17 | 17 | 103 | — | yes | 0 |
| small | B3 | shipping | 181 | 188 | 188 | 164 | — | yes | 0 |
| small | B3 | utoo | 484 | 507 | 507 | 267 | — | yes | 0 |
| small | B4 | bun | 33 | 34 | 34 | 51 | — | yes | 0 |
| small | B4 | esbuild | 42 | 45 | 45 | 102 | — | yes | 0 |
| small | B4 | rolldown | 125 | 146 | 146 | 113 | — | yes | 0 |
| small | B4 | rspack | 16 | 19 | 19 | 103 | — | yes | 0 |
| small | B4 | shipping | 181 | 190 | 190 | 155 | — | yes | 0 |
| small | B4 | utoo | 483 | 509 | 509 | 267 | — | yes | 0 |
| small | B5 | rspack | 38 | 41 | 41 | 50 | — | yes | 0 |
| small | B5 | utoo | 963 | 1021 | 1021 | 296 | — | yes | 0 |
| small | B6 | rolldown | 24 | 27 | 27 | 120 | — | yes | 0 |
| small | B6 | rspack | 18 | 19 | 19 | 102 | — | yes | 0 |

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
- utoo: 0 match, 80 differ; locale chunks 1, locale in an entry: no

