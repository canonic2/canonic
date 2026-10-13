# Rspack pipeline stage 2, run set 2

Samples per cell: 10, warmups: 3.

| Fixture | Sequence | Cell | Variant | n | errors | p50 ms | p95 ms | max ms |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| formats | N7 | between adapters | 0 | 23 | 0 | 194 | 211 | 486 |
| formats | N7 | between adapters | 1 | 23 | 0 | 197 | 218 | 492 |
| formats | N7 | between adapters | 2 | 23 | 0 | 194 | 276 | 277 |
| formats | N7 | between adapters | 3 | 23 | 0 | 154 | 197 | 231 |
| formats | E1 | cssGlobal | 0 | 10 | 0 | 702 | 746 | 746 |
| formats | E1 | cssGlobal | 2 | 10 | 0 | 99 | 114 | 114 |
| formats | E1 | cssGlobal | 3 | 10 | 0 | 93 | 392 | 392 |
| formats | E1 | cssModule | 0 | 10 | 0 | 706 | 744 | 744 |
| formats | E1 | cssModule | 2 | 10 | 0 | 79 | 209 | 209 |
| formats | E1 | cssModule | 3 | 10 | 0 | 95 | 537 | 537 |
| formats | E1 | vueScoped | 0 | 10 | 0 | 629 | 669 | 669 |
| formats | E1 | vueScoped | 2 | 10 | 0 | 80 | 143 | 143 |
| formats | E1 | vueScoped | 3 | 10 | 0 | 86 | 126 | 126 |
| formats | E2 | reactComponent | 0 | 10 | 0 | 710 | 723 | 723 |
| formats | E2 | reactComponent | 2 | 10 | 0 | 109 | 127 | 127 |
| formats | E2 | reactComponent | 3 | 10 | 0 | 107 | 147 | 147 |
| formats | E3 | nonComponentModule | 0 | 10 | 0 | 707 | 772 | 772 |
| formats | E3 | nonComponentModule | 2 | 10 | 0 | 95 | 132 | 132 |
| formats | E3 | nonComponentModule | 3 | 10 | 0 | 98 | 151 | 151 |
| formats | E4 | vueScript | 0 | 10 | 0 | 601 | 637 | 637 |
| formats | E4 | vueScript | 2 | 10 | 0 | 74 | 102 | 102 |
| formats | E4 | vueScript | 3 | 10 | 0 | 96 | 142 | 142 |
| formats | E4 | vueTemplate | 0 | 10 | 0 | 578 | 654 | 654 |
| formats | E4 | vueTemplate | 2 | 10 | 0 | 76 | 131 | 131 |
| formats | E4 | vueTemplate | 3 | 10 | 0 | 88 | 120 | 120 |
| formats | E5 | htmlMarkup | 0 | 10 | 0 | 594 | 612 | 612 |
| formats | E5 | htmlMarkup | 2 | 10 | 0 | 173 | 191 | 191 |
| formats | E5 | htmlMarkup | 3 | 10 | 0 | 170 | 426 | 426 |
| formats | E5 | htmlStylesheet | 0 | 10 | 0 | 594 | 611 | 611 |
| formats | E5 | htmlStylesheet | 2 | 10 | 0 | 90 | 126 | 126 |
| formats | E5 | htmlStylesheet | 3 | 10 | 0 | 93 | 177 | 177 |
| formats | E6 | astroComponent | 0 | 10 | 0 | 592 | 618 | 618 |
| formats | E6 | astroComponent | 2 | 10 | 0 | 176 | 183 | 183 |
| formats | E6 | astroComponent | 3 | 10 | 0 | 170 | 181 | 181 |
| formats | E7 | docsExample | 0 | 10 | 0 | 415 | 538 | 538 |
| formats | E7 | docsExample | 2 | 10 | 0 | 108 | 133 | 133 |
| formats | E7 | docsExample | 3 | 10 | 0 | 115 | 141 | 141 |
| formats | E7 | docsMarkdown | 0 | 10 | 0 | 438 | 507 | 507 |
| formats | E7 | docsMarkdown | 2 | 10 | 0 | 61 | 62 | 62 |
| formats | E7 | docsMarkdown | 3 | 10 | 0 | 53 | 57 | 57 |
| formats | E8 | config | 0 | 10 | 0 | 588 | 1453 | 1453 |
| formats | E8 | config | 2 | 10 | 0 | 594 | 668 | 668 |
| formats | E8 | config | 3 | 10 | 0 | 551 | 585 | 585 |
| formats | E8 | definition | 0 | 10 | 0 | 547 | 650 | 650 |
| formats | E8 | definition | 2 | 10 | 0 | 254 | 297 | 297 |
| formats | E8 | definition | 3 | 10 | 0 | 266 | 346 | 346 |
| formats | E8 | environment | 0 | 10 | 0 | 581 | 802 | 802 |
| formats | E8 | environment | 2 | 10 | 0 | 266 | 453 | 453 |
| formats | E8 | environment | 3 | 10 | 0 | 269 | 298 | 298 |
| formats | E9 | shared | 0 | 10 | 0 | 701 | 730 | 730 |
| formats | E9 | shared | 2 | 10 | 0 | 122 | 148 | 148 |
| formats | E9 | shared | 3 | 10 | 0 | 105 | 140 | 140 |
| huge | A1 | cold, from server start | 0 | 10 | 0 | 7536 | 13539 | 13539 |
| huge | A1 | cold, from server start | 2 | 10 | 0 | 17810 | 29495 | 29495 |
| huge | A1 | cold, from server start | D2 | 10 | 0 | 13108 | 24332 | 24332 |
| huge | A2 | hot, from server start | 0 | 10 | 0 | 3655 | 5937 | 5937 |
| huge | A2 | hot, from server start | 2 | 10 | 0 | 13894 | 18074 | 18074 |
| huge | A2 | hot, from server start | D2 | 10 | 0 | 9886 | 16895 | 16895 |
| huge | A3 | from URL | 0 | 10 | 0 | 3211 | 5484 | 5484 |
| huge | A3 | from URL | 2 | 10 | 0 | 13350 | 17543 | 17543 |
| huge | A3 | from URL | D2 | 10 | 0 | 9392 | 16313 | 16313 |
| huge | A3-cold | from URL | 0 | 10 | 0 | 7106 | 13029 | 13029 |
| huge | A3-cold | from URL | 2 | 10 | 0 | 17311 | 28803 | 28803 |
| huge | A3-cold | from URL | D2 | 10 | 0 | 12553 | 23849 | 23849 |
| huge | A4 | after pre-warm | 0 | 10 | 0 | 832 | 1395 | 1395 |
| huge | A4 | after pre-warm | 2 | 10 | 0 | 2075 | 6508 | 6508 |
| huge | A4 | after pre-warm | D2 | 10 | 0 | 6331 | 11075 | 11075 |
| huge | N1 | second page | 0 | 10 | 0 | 1421 | 1690 | 1690 |
| huge | N1 | second page | 2 | 10 | 0 | 337 | 364 | 364 |
| huge | N1 | second page | 3 | 10 | 0 | 198 | 228 | 228 |
| huge | N2 | paced | 0 | 10 | 0 | 400 | 437 | 437 |
| huge | N2 | paced | 2 | 10 | 0 | 244 | 270 | 270 |
| huge | N2 | paced | 3 | 10 | 0 | 156 | 205 | 205 |
| huge | N3 | back to back | 0 | 10 | 0 | 1417 | 1520 | 1520 |
| huge | N3 | back to back | 2 | 10 | 0 | 230 | 259 | 259 |
| huge | N3 | back to back | 3 | 10 | 0 | 144 | 234 | 234 |
| huge | N4 | retained | 0 | 10 | 0 | 63 | 68 | 68 |
| huge | N4 | retained | 2 | 10 | 0 | 57 | 70 | 70 |
| huge | N4 | retained | 3 | 10 | 0 | 109 | 122 | 122 |
| huge | N5 | size | 0 | 10 | 0 | 29 | 47 | 47 |
| huge | N5 | size | 2 | 10 | 0 | 33 | 50 | 50 |
| huge | N5 | size | 3 | 10 | 0 | 25 | 31 | 31 |
| huge | N5 | state | 0 | 10 | 0 | 58 | 65 | 65 |
| huge | N5 | state | 2 | 10 | 0 | 46 | 59 | 59 |
| huge | N5 | state | 3 | 10 | 0 | 154 | 283 | 283 |
| huge | N6 | reload | 0 | 10 | 0 | 174 | 313 | 313 |
| huge | N6 | reload | 2 | 10 | 0 | 203 | 216 | 216 |
| huge | N6 | reload | 3 | 10 | 0 | 95 | 112 | 112 |
| large-system | A1 | cold, from server start | 0 | 10 | 0 | 6184 | 7018 | 7018 |
| large-system | A1 | cold, from server start | 2 | 10 | 0 | 6424 | 7047 | 7047 |
| large-system | A1 | cold, from server start | D2 | 10 | 0 | 6992 | 17968 | 17968 |
| large-system | A2 | hot, from server start | 0 | 10 | 0 | 2153 | 2597 | 2597 |
| large-system | A2 | hot, from server start | 2 | 10 | 0 | 3797 | 3943 | 3943 |
| large-system | A2 | hot, from server start | D2 | 10 | 0 | 4246 | 9286 | 9286 |
| large-system | A3 | from URL | 0 | 10 | 0 | 1756 | 2152 | 2152 |
| large-system | A3 | from URL | 2 | 10 | 0 | 3406 | 3533 | 3533 |
| large-system | A3 | from URL | D2 | 10 | 0 | 3852 | 8878 | 8878 |
| large-system | A3-cold | from URL | 0 | 10 | 0 | 5776 | 6616 | 6616 |
| large-system | A3-cold | from URL | 2 | 10 | 0 | 5986 | 6653 | 6653 |
| large-system | A3-cold | from URL | D2 | 10 | 0 | 6459 | 17565 | 17565 |
| large-system | A4 | after pre-warm | 0 | 10 | 0 | 700 | 814 | 814 |
| large-system | A4 | after pre-warm | 2 | 10 | 0 | 826 | 886 | 886 |
| large-system | A4 | after pre-warm | D2 | 10 | 0 | 2736 | 6118 | 6118 |
| large-system | N1 | second page | 0 | 10 | 0 | 1431 | 1543 | 1543 |
| large-system | N1 | second page | 1 | 10 | 0 | 1698 | 3781 | 3781 |
| large-system | N1 | second page | 2 | 10 | 0 | 325 | 350 | 350 |
| large-system | N1 | second page | 3 | 10 | 0 | 168 | 198 | 198 |
| large-system | N1 | second page | D3 | 10 | 0 | 346 | 387 | 387 |
| large-system | N2 | paced | 0 | 10 | 0 | 404 | 427 | 427 |
| large-system | N2 | paced | 1 | 10 | 0 | 434 | 463 | 463 |
| large-system | N2 | paced | 2 | 10 | 0 | 238 | 326 | 326 |
| large-system | N2 | paced | 3 | 10 | 0 | 144 | 209 | 209 |
| large-system | N2 | paced | D3 | 10 | 0 | 219 | 243 | 243 |
| large-system | N3 | back to back | 0 | 10 | 0 | 1424 | 1508 | 1508 |
| large-system | N3 | back to back | 1 | 10 | 0 | 1491 | 1642 | 1642 |
| large-system | N3 | back to back | 2 | 10 | 0 | 226 | 263 | 263 |
| large-system | N3 | back to back | 3 | 10 | 0 | 141 | 170 | 170 |
| large-system | N3 | back to back | D3 | 10 | 0 | 215 | 273 | 273 |
| large-system | N4 | retained | 0 | 10 | 0 | 60 | 67 | 67 |
| large-system | N4 | retained | 1 | 10 | 0 | 62 | 71 | 71 |
| large-system | N4 | retained | 2 | 10 | 0 | 44 | 63 | 63 |
| large-system | N4 | retained | 3 | 10 | 0 | 94 | 105 | 105 |
| large-system | N4 | retained | D3 | 10 | 0 | 51 | 55 | 55 |
| large-system | N5 | size | 0 | 10 | 0 | 20 | 33 | 33 |
| large-system | N5 | size | 1 | 10 | 0 | 30 | 34 | 34 |
| large-system | N5 | size | 2 | 10 | 0 | 24 | 32 | 32 |
| large-system | N5 | size | 3 | 10 | 0 | 29 | 34 | 34 |
| large-system | N5 | size | D3 | 10 | 0 | 18 | 27 | 27 |
| large-system | N5 | state | 0 | 10 | 0 | 52 | 56 | 56 |
| large-system | N5 | state | 1 | 10 | 0 | 55 | 70 | 70 |
| large-system | N5 | state | 2 | 10 | 0 | 49 | 58 | 58 |
| large-system | N5 | state | 3 | 10 | 0 | 143 | 169 | 169 |
| large-system | N5 | state | D3 | 10 | 0 | 48 | 57 | 57 |
| large-system | N6 | reload | 0 | 10 | 0 | 165 | 286 | 286 |
| large-system | N6 | reload | 1 | 10 | 0 | 178 | 299 | 299 |
| large-system | N6 | reload | 2 | 10 | 0 | 177 | 199 | 199 |
| large-system | N6 | reload | 3 | 10 | 0 | 94 | 105 | 105 |
| large-system | N6 | reload | D3 | 10 | 0 | 198 | 211 | 211 |
| large-system | E2 | React component | 0 | 10 | 0 | 1916 | 2041 | 2041 |
| large-system | E2 | React component | 2 | 10 | 0 | 160 | 197 | 197 |
| large-system | E2 | React component | 3 | 10 | 0 | 157 | 183 | 183 |
| large-system | E3 | module exporting more than components | 0 | 10 | 0 | 2054 | 2137 | 2137 |
| large-system | E3 | module exporting more than components | 2 | 10 | 0 | 195 | 208 | 208 |
| large-system | E3 | module exporting more than components | 3 | 10 | 0 | 178 | 191 | 191 |
| large-system | E8 | environment | 0 | 10 | 0 | 2126 | 2243 | 2243 |
| large-system | E8 | environment | 2 | 10 | 0 | 453 | 1186 | 1186 |
| large-system | E8 | environment | 3 | 10 | 0 | 459 | 567 | 567 |
| large-system | E8 | preview definition | 0 | 10 | 0 | 2129 | 2267 | 2267 |
| large-system | E8 | preview definition | 2 | 10 | 0 | 368 | 503 | 503 |
| large-system | E8 | preview definition | 3 | 10 | 0 | 345 | 456 | 456 |
| large-system | E9 | module shared by every page | 0 | 10 | 0 | 2208 | 2587 | 2587 |
| large-system | E9 | module shared by every page | 2 | 10 | 0 | 203 | 228 | 228 |
| large-system | E9 | module shared by every page | 3 | 10 | 0 | 172 | 218 | 218 |
| large-web | N1 | second page | 0 | 10 | 0 | 2517 | 2597 | 2597 |
| large-web | N1 | second page | 1 | 10 | 0 | 2561 | 2688 | 2688 |
| large-web | N1 | second page | 2 | 10 | 0 | 385 | 429 | 429 |
| large-web | N1 | second page | 3 | 10 | 0 | 233 | 278 | 278 |
| large-web | N1 | second page | D3 | 10 | 0 | 449 | 478 | 478 |
| large-web | N2 | paced | 0 | 10 | 0 | 1272 | 1379 | 1379 |
| large-web | N2 | paced | 1 | 10 | 0 | 481 | 515 | 515 |
| large-web | N2 | paced | 2 | 10 | 0 | 257 | 332 | 332 |
| large-web | N2 | paced | 3 | 10 | 0 | 161 | 194 | 194 |
| large-web | N2 | paced | D3 | 10 | 0 | 241 | 309 | 309 |
| large-web | N2 | paced, +150 ms | 0 | 10 | 0 | 1602 | 1991 | 1991 |
| large-web | N2 | paced, +150 ms | 1 | 10 | 0 | 589 | 808 | 808 |
| large-web | N2 | paced, +150 ms | 2 | 10 | 0 | 396 | 412 | 412 |
| large-web | N2 | paced, +150 ms | 3 | 10 | 0 | 303 | 330 | 330 |
| large-web | N2 | paced, +150 ms | D3 | 10 | 0 | 231 | 309 | 309 |
| large-web | N3 | back to back | 0 | 10 | 0 | 2485 | 2735 | 2735 |
| large-web | N3 | back to back | 1 | 10 | 0 | 2386 | 2638 | 2638 |
| large-web | N3 | back to back | 2 | 10 | 0 | 263 | 287 | 287 |
| large-web | N3 | back to back | 3 | 10 | 0 | 177 | 190 | 190 |
| large-web | N3 | back to back | D3 | 10 | 0 | 240 | 295 | 295 |
| large-web | N4 | retained | 0 | 10 | 0 | 58 | 73 | 73 |
| large-web | N4 | retained | 1 | 10 | 0 | 58 | 67 | 67 |
| large-web | N4 | retained | 2 | 10 | 0 | 48 | 65 | 65 |
| large-web | N4 | retained | 3 | 10 | 0 | 109 | 125 | 125 |
| large-web | N4 | retained | D3 | 10 | 0 | 48 | 66 | 66 |
| large-web | N5 | size | 0 | 10 | 0 | 19 | 28 | 28 |
| large-web | N5 | size | 1 | 10 | 0 | 18 | 44 | 44 |
| large-web | N5 | size | 2 | 10 | 0 | 17 | 33 | 33 |
| large-web | N5 | size | 3 | 10 | 0 | 23 | 35 | 35 |
| large-web | N5 | size | D3 | 10 | 0 | 22 | 30 | 30 |
| large-web | N5 | state | 0 | 10 | 0 | 54 | 57 | 57 |
| large-web | N5 | state | 1 | 10 | 0 | 51 | 67 | 67 |
| large-web | N5 | state | 2 | 10 | 0 | 48 | 58 | 58 |
| large-web | N5 | state | 3 | 10 | 0 | 168 | 202 | 202 |
| large-web | N5 | state | D3 | 10 | 0 | 47 | 57 | 57 |
| large-web | N6 | reload | 0 | 10 | 0 | 203 | 322 | 322 |
| large-web | N6 | reload | 1 | 10 | 0 | 196 | 207 | 207 |
| large-web | N6 | reload | 2 | 10 | 0 | 214 | 391 | 391 |
| large-web | N6 | reload | 3 | 10 | 0 | 101 | 124 | 124 |
| large-web | N6 | reload | D3 | 10 | 0 | 214 | 220 | 220 |
| large-web | N8 | new font | 0 | 10 | 0 | 2546 | 2966 | 2966 |
| large-web | N8 | new font | 1 | 10 | 0 | 1860 | 1932 | 1932 |
| large-web | N8 | new font | 2 | 9 | 1 | 378 | 432 | 432 |
| large-web | N8 | new font | 3 | 10 | 0 | 225 | 296 | 296 |
| large-web | N8 | new font | D3 | 10 | 0 | 339 | 378 | 378 |
| large-web | N8 | new font, +150 ms | 0 | 10 | 0 | 1626 | 1682 | 1682 |
| large-web | N8 | new font, +150 ms | 1 | 10 | 0 | 793 | 909 | 909 |
| large-web | N8 | new font, +150 ms | 2 | 10 | 0 | 633 | 730 | 730 |
| large-web | N8 | new font, +150 ms | 3 | 10 | 0 | 495 | 676 | 676 |
| large-web | N8 | new font, +150 ms | D3 | 10 | 0 | 655 | 674 | 674 |
| small | A1 | cold, from server start | 0 | 10 | 0 | 4119 | 5736 | 5736 |
| small | A1 | cold, from server start | 2 | 10 | 0 | 4158 | 4626 | 4626 |
| small | A1 | cold, from server start | D2 | 10 | 0 | 4114 | 7324 | 7324 |
| small | A2 | hot, from server start | 0 | 10 | 0 | 1546 | 2954 | 2954 |
| small | A2 | hot, from server start | 2 | 10 | 0 | 1560 | 2522 | 2522 |
| small | A2 | hot, from server start | D2 | 10 | 0 | 1854 | 2802 | 2802 |
| small | A3 | from URL | 0 | 10 | 0 | 1186 | 2595 | 2595 |
| small | A3 | from URL | 2 | 10 | 0 | 1206 | 2169 | 2169 |
| small | A3 | from URL | D2 | 10 | 0 | 1492 | 2429 | 2429 |
| small | A3-cold | from URL | 0 | 10 | 0 | 3754 | 5336 | 5336 |
| small | A3-cold | from URL | 2 | 10 | 0 | 3816 | 4280 | 4280 |
| small | A3-cold | from URL | D2 | 10 | 0 | 3765 | 6599 | 6599 |
| small | A4 | after pre-warm | 0 | 10 | 0 | 313 | 693 | 693 |
| small | A4 | after pre-warm | 2 | 10 | 0 | 296 | 489 | 489 |
| small | A4 | after pre-warm | D2 | 10 | 0 | 610 | 712 | 712 |
| small | N1 | second page | 0 | 10 | 0 | 127 | 265 | 265 |
| small | N1 | second page | 1 | 10 | 0 | 127 | 287 | 287 |
| small | N1 | second page | 2 | 10 | 0 | 82 | 92 | 92 |
| small | N1 | second page | 3 | 10 | 0 | 48 | 63 | 63 |
| small | N1 | second page | D3 | 10 | 0 | 82 | 125 | 125 |
| small | N2 | paced | 0 | 1 | 0 | 146 | 146 | 146 |
| small | N2 | paced | 1 | 1 | 0 | 142 | 142 | 142 |
| small | N2 | paced | 2 | 1 | 0 | 101 | 101 | 101 |
| small | N2 | paced | 3 | 1 | 0 | 56 | 56 | 56 |
| small | N2 | paced | D3 | 1 | 0 | 92 | 92 | 92 |
| small | N4 | retained | 0 | 10 | 0 | 51 | 57 | 57 |
| small | N4 | retained | 1 | 10 | 0 | 46 | 57 | 57 |
| small | N4 | retained | 2 | 10 | 0 | 43 | 56 | 56 |
| small | N4 | retained | 3 | 10 | 0 | 38 | 50 | 50 |
| small | N4 | retained | D3 | 10 | 0 | 47 | 56 | 56 |
| small | N5 | size | 0 | 10 | 0 | 26 | 35 | 35 |
| small | N5 | size | 1 | 10 | 0 | 26 | 34 | 34 |
| small | N5 | size | 2 | 10 | 0 | 22 | 31 | 31 |
| small | N5 | size | 3 | 10 | 0 | 24 | 34 | 34 |
| small | N5 | size | D3 | 10 | 0 | 28 | 35 | 35 |
| small | N5 | state | 0 | 10 | 0 | 47 | 55 | 55 |
| small | N5 | state | 1 | 10 | 0 | 45 | 57 | 57 |
| small | N5 | state | 2 | 10 | 0 | 46 | 56 | 56 |
| small | N5 | state | 3 | 10 | 0 | 40 | 51 | 51 |
| small | N5 | state | D3 | 10 | 0 | 49 | 55 | 55 |
| small | N6 | reload | 0 | 10 | 0 | 72 | 103 | 103 |
| small | N6 | reload | 1 | 10 | 0 | 65 | 78 | 78 |
| small | N6 | reload | 2 | 10 | 0 | 75 | 79 | 79 |
| small | N6 | reload | 3 | 10 | 0 | 35 | 43 | 43 |
| small | N6 | reload | D3 | 10 | 0 | 74 | 89 | 89 |
| small | E2 | React component | 0 | 10 | 0 | 874 | 914 | 914 |
| small | E2 | React component | 2 | 10 | 0 | 62 | 106 | 106 |
| small | E2 | React component | 3 | 10 | 0 | 66 | 81 | 81 |

## Values

- small D2 cache compile cache bytes after cold start #3: 1.9 MB
- small D2 cache compile cache bytes after cold start #4: 1.9 MB
- small D2 cache compile cache bytes after cold start #5: 1.9 MB
- small D2 cache compile cache bytes after cold start #6: 1.9 MB
- small D2 cache compile cache bytes after cold start #7: 1.9 MB
- small D2 cache compile cache bytes after cold start #8: 1.9 MB
- small D2 cache compile cache bytes after cold start #9: 1.9 MB
- small D2 cache compile cache bytes after cold start #10: 1.9 MB
- small D2 cache compile cache bytes after cold start #11: 1.9 MB
- small D2 cache compile cache bytes after cold start #12: 1.9 MB
- small 0 cache compile cache bytes after cold start #3: 3.1 MB
- small 0 cache compile cache bytes after cold start #4: 3.1 MB
- small 0 cache compile cache bytes after cold start #5: 3.1 MB
- small 0 cache compile cache bytes after cold start #6: 3.1 MB
- small 0 cache compile cache bytes after cold start #7: 3.1 MB
- small 0 cache compile cache bytes after cold start #8: 3.1 MB
- small 0 cache compile cache bytes after cold start #9: 3.1 MB
- small 0 cache compile cache bytes after cold start #10: 3.1 MB
- small 0 cache compile cache bytes after cold start #11: 3.1 MB
- small 0 cache compile cache bytes after cold start #12: 3.1 MB
- small 2 cache compile cache bytes after cold start #3: 1.9 MB
- small 2 cache compile cache bytes after cold start #4: 1.9 MB
- small 2 cache compile cache bytes after cold start #5: 1.9 MB
- small 2 cache compile cache bytes after cold start #6: 1.9 MB
- small 2 cache compile cache bytes after cold start #7: 1.9 MB
- small 2 cache compile cache bytes after cold start #8: 1.9 MB
- small 2 cache compile cache bytes after cold start #9: 1.9 MB
- small 2 cache compile cache bytes after cold start #10: 1.9 MB
- small 2 cache compile cache bytes after cold start #11: 1.9 MB
- small 2 cache compile cache bytes after cold start #12: 1.9 MB
- large-system 2 cache compile cache bytes after cold start #3: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #4: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #5: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #6: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #7: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #8: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #9: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #10: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #11: 142.2 MB
- large-system 2 cache compile cache bytes after cold start #12: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #3: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #4: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #5: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #6: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #7: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #8: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #9: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #10: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #11: 142.2 MB
- large-system D2 cache compile cache bytes after cold start #12: 142.2 MB
- large-system 0 cache compile cache bytes after cold start #3: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #4: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #5: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #6: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #7: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #8: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #9: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #10: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #11: 16.2 MB
- large-system 0 cache compile cache bytes after cold start #12: 16.2 MB
- huge 2 cache compile cache bytes after cold start #3: 389.2 MB
- huge 2 cache compile cache bytes after cold start #4: 443.5 MB
- huge 2 cache compile cache bytes after cold start #5: 389.2 MB
- huge 2 cache compile cache bytes after cold start #6: 389.2 MB
- huge 2 cache compile cache bytes after cold start #7: 465.8 MB
- huge 2 cache compile cache bytes after cold start #8: 389.2 MB
- huge 2 cache compile cache bytes after cold start #9: 389.2 MB
- huge 2 cache compile cache bytes after cold start #10: 389.2 MB
- huge 2 cache compile cache bytes after cold start #11: 389.2 MB
- huge 2 cache compile cache bytes after cold start #12: 389.2 MB
- huge D2 cache compile cache bytes after cold start #3: 389.2 MB
- huge D2 cache compile cache bytes after cold start #4: 389.2 MB
- huge D2 cache compile cache bytes after cold start #5: 389.2 MB
- huge D2 cache compile cache bytes after cold start #6: 389.2 MB
- huge D2 cache compile cache bytes after cold start #7: 389.2 MB
- huge D2 cache compile cache bytes after cold start #8: 436.1 MB
- huge D2 cache compile cache bytes after cold start #9: 389.2 MB
- huge D2 cache compile cache bytes after cold start #10: 389.2 MB
- huge D2 cache compile cache bytes after cold start #11: 389.2 MB
- huge D2 cache compile cache bytes after cold start #12: 389.2 MB
- huge 0 cache compile cache bytes after cold start #3: 16.1 MB
- huge 0 cache compile cache bytes after cold start #4: 16.1 MB
- huge 0 cache compile cache bytes after cold start #5: 16.1 MB
- huge 0 cache compile cache bytes after cold start #6: 16.1 MB
- huge 0 cache compile cache bytes after cold start #7: 16.1 MB
- huge 0 cache compile cache bytes after cold start #8: 16.1 MB
- huge 0 cache compile cache bytes after cold start #9: 16.1 MB
- huge 0 cache compile cache bytes after cold start #10: 16.1 MB
- huge 0 cache compile cache bytes after cold start #11: 16.1 MB
- huge 0 cache compile cache bytes after cold start #12: 16.1 MB
- small 3 heap after N2 and N3: 7.4 MB
- small 3 worker peak RSS: 553.1 MB
- small D3 heap after N2 and N3: 9.3 MB
- small D3 worker peak RSS: 538.5 MB
- small 2 heap after N2 and N3: 9.3 MB
- small 2 worker peak RSS: 569.7 MB
- small 1 heap after N2 and N3: 10.9 MB
- small 1 worker peak RSS: 596.6 MB
- small 0 heap after N2 and N3: 10.9 MB
- small 0 worker peak RSS: 563.4 MB
- large-system 1 heap after N2 and N3: 135.0 MB
- large-system 1 worker peak RSS: 1683.2 MB
- large-system 0 heap after N2 and N3: 149.1 MB
- large-system 0 worker peak RSS: 1724.2 MB
- large-system D3 heap after N2 and N3: 288.7 MB
- large-system D3 worker peak RSS: 1168.2 MB
- large-system 2 heap after N2 and N3: 293.1 MB
- large-system 2 worker peak RSS: 1119.6 MB
- large-system 3 heap after N2 and N3: 176.4 MB
- large-system 3 worker peak RSS: 1148.1 MB
- large-web D3 heap after N2 and N3: 227.1 MB
- large-web D3 worker peak RSS: 1582.7 MB
- large-web 2 heap after N2 and N3: 227.1 MB
- large-web 2 worker peak RSS: 1566.1 MB
- large-web 0 heap after N2 and N3: 159.1 MB
- large-web 0 worker peak RSS: 2499.9 MB
- large-web 1 heap after N2 and N3: 161.0 MB
- large-web 1 worker peak RSS: 2460.0 MB
- large-web 3 heap after N2 and N3: 124.3 MB
- large-web 3 worker peak RSS: 1586.7 MB
- huge 0 heap after N2 and N3: 156.5 MB
- huge 0 worker peak RSS: 1768.3 MB
- huge 2 heap after N2 and N3: 268.7 MB
- huge 2 worker peak RSS: 2049.3 MB
- huge 3 heap after N2 and N3: 145.9 MB
- huge 3 worker peak RSS: 2043.6 MB
- formats 3 worker peak RSS: 675.1 MB
- formats 2 worker peak RSS: 722.0 MB
- formats 1 worker peak RSS: 658.0 MB
- formats 0 worker peak RSS: 634.2 MB

## Checks

- 0 formats console: 2 pass, 0 fail
- 0 formats double-save: 1 pass, 0 fail
- 0 formats freshness: 1 pass, 0 fail
- 0 formats isolation: 22 pass, 0 fail
- 0 formats no-polling: 0 pass, 1 fail
- 0 formats syntax-error: 0 pass, 1 fail
- 0 huge console: 1 pass, 0 fail
- 0 huge console-start: 1 pass, 0 fail
- 0 huge error-recovery: 1 pass, 0 fail
- 0 huge error-shown: 2 pass, 0 fail
- 0 huge isolation: 13 pass, 0 fail
- 0 huge no-polling: 0 pass, 1 fail
- 0 huge readiness: 26 pass, 0 fail
- 0 large-system console: 2 pass, 0 fail
- 0 large-system console-start: 1 pass, 0 fail
- 0 large-system error-recovery: 1 pass, 0 fail
- 0 large-system error-shown: 2 pass, 0 fail
- 0 large-system freshness: 1 pass, 0 fail
- 0 large-system isolation: 13 pass, 0 fail
- 0 large-system no-polling: 0 pass, 1 fail
- 0 large-system readiness: 26 pass, 0 fail
- 0 large-web console: 1 pass, 0 fail
- 0 large-web error-recovery: 1 pass, 0 fail
- 0 large-web error-shown: 2 pass, 0 fail
- 0 large-web isolation: 13 pass, 0 fail
- 0 large-web no-polling: 0 pass, 1 fail
- 0 large-web readiness: 26 pass, 0 fail
- 0 small console: 2 pass, 0 fail
- 0 small console-start: 1 pass, 0 fail
- 0 small error-recovery: 1 pass, 0 fail
- 0 small freshness: 1 pass, 0 fail
- 0 small no-polling: 0 pass, 1 fail
- 0 small readiness: 4 pass, 0 fail
- 1 formats console: 1 pass, 0 fail
- 1 formats isolation: 22 pass, 0 fail
- 1 formats no-polling: 0 pass, 1 fail
- 1 formats render-parity: 1 pass, 0 fail
- 1 large-system console: 1 pass, 0 fail
- 1 large-system error-recovery: 1 pass, 0 fail
- 1 large-system error-shown: 2 pass, 0 fail
- 1 large-system isolation: 13 pass, 0 fail
- 1 large-system no-polling: 0 pass, 1 fail
- 1 large-system readiness: 26 pass, 0 fail
- 1 large-system render-parity: 1 pass, 0 fail
- 1 large-web console: 1 pass, 0 fail
- 1 large-web error-recovery: 1 pass, 0 fail
- 1 large-web error-shown: 2 pass, 0 fail
- 1 large-web isolation: 13 pass, 0 fail
- 1 large-web no-polling: 0 pass, 1 fail
- 1 large-web readiness: 0 pass, 26 fail
- 1 small console: 1 pass, 0 fail
- 1 small error-recovery: 1 pass, 0 fail
- 1 small no-polling: 0 pass, 1 fail
- 1 small readiness: 4 pass, 0 fail
- 2 formats console: 2 pass, 0 fail
- 2 formats double-save: 1 pass, 0 fail
- 2 formats freshness: 1 pass, 0 fail
- 2 formats isolation: 22 pass, 0 fail
- 2 formats no-polling: 1 pass, 0 fail
- 2 formats render-parity: 1 pass, 0 fail
- 2 formats syntax-error: 1 pass, 0 fail
- 2 huge console: 1 pass, 0 fail
- 2 huge console-start: 1 pass, 0 fail
- 2 huge error-recovery: 1 pass, 0 fail
- 2 huge error-shown: 2 pass, 0 fail
- 2 huge isolation: 13 pass, 0 fail
- 2 huge no-polling: 1 pass, 0 fail
- 2 huge readiness: 26 pass, 0 fail
- 2 large-system console: 2 pass, 0 fail
- 2 large-system console-start: 1 pass, 0 fail
- 2 large-system error-recovery: 1 pass, 0 fail
- 2 large-system error-shown: 2 pass, 0 fail
- 2 large-system freshness: 1 pass, 0 fail
- 2 large-system isolation: 13 pass, 0 fail
- 2 large-system no-polling: 1 pass, 0 fail
- 2 large-system readiness: 26 pass, 0 fail
- 2 large-system render-parity: 1 pass, 0 fail
- 2 large-web console: 1 pass, 0 fail
- 2 large-web error-recovery: 1 pass, 0 fail
- 2 large-web error-shown: 2 pass, 0 fail
- 2 large-web isolation: 13 pass, 0 fail
- 2 large-web no-polling: 1 pass, 0 fail
- 2 large-web readiness: 26 pass, 0 fail
- 2 small console: 2 pass, 0 fail
- 2 small console-start: 1 pass, 0 fail
- 2 small error-recovery: 1 pass, 0 fail
- 2 small freshness: 1 pass, 0 fail
- 2 small no-polling: 1 pass, 0 fail
- 2 small readiness: 4 pass, 0 fail
- 3 formats console: 2 pass, 0 fail
- 3 formats double-save: 1 pass, 0 fail
- 3 formats freshness: 1 pass, 0 fail
- 3 formats isolation: 22 pass, 0 fail
- 3 formats no-polling: 1 pass, 0 fail
- 3 formats render-parity: 1 pass, 0 fail
- 3 formats syntax-error: 1 pass, 0 fail
- 3 huge console: 1 pass, 0 fail
- 3 huge error-recovery: 1 pass, 0 fail
- 3 huge error-shown: 2 pass, 0 fail
- 3 huge isolation: 13 pass, 0 fail
- 3 huge no-polling: 1 pass, 0 fail
- 3 huge readiness: 26 pass, 0 fail
- 3 large-system console: 2 pass, 0 fail
- 3 large-system error-recovery: 1 pass, 0 fail
- 3 large-system error-shown: 2 pass, 0 fail
- 3 large-system freshness: 1 pass, 0 fail
- 3 large-system isolation: 13 pass, 0 fail
- 3 large-system no-polling: 1 pass, 0 fail
- 3 large-system readiness: 26 pass, 0 fail
- 3 large-system render-parity: 1 pass, 0 fail
- 3 large-web console: 1 pass, 0 fail
- 3 large-web error-recovery: 1 pass, 0 fail
- 3 large-web error-shown: 2 pass, 0 fail
- 3 large-web isolation: 13 pass, 0 fail
- 3 large-web no-polling: 1 pass, 0 fail
- 3 large-web readiness: 26 pass, 0 fail
- 3 small console: 2 pass, 0 fail
- 3 small error-recovery: 1 pass, 0 fail
- 3 small freshness: 1 pass, 0 fail
- 3 small no-polling: 1 pass, 0 fail
- 3 small readiness: 4 pass, 0 fail
- D2 huge console-start: 1 pass, 0 fail
- D2 large-system console-start: 1 pass, 0 fail
- D2 small console-start: 1 pass, 0 fail
- D3 large-system console: 1 pass, 0 fail
- D3 large-system error-recovery: 1 pass, 0 fail
- D3 large-system error-shown: 2 pass, 0 fail
- D3 large-system isolation: 13 pass, 0 fail
- D3 large-system no-polling: 1 pass, 0 fail
- D3 large-web console: 1 pass, 0 fail
- D3 large-web error-recovery: 1 pass, 0 fail
- D3 large-web error-shown: 2 pass, 0 fail
- D3 large-web isolation: 13 pass, 0 fail
- D3 large-web no-polling: 1 pass, 0 fail
- D3 small console: 1 pass, 0 fail
- D3 small error-recovery: 1 pass, 0 fail
- D3 small no-polling: 1 pass, 0 fail
- all formats no-project-writes: 1 pass, 0 fail
- all huge no-project-writes: 1 pass, 0 fail
- all large-system no-project-writes: 1 pass, 0 fail
- all large-web no-project-writes: 1 pass, 0 fail
- all small no-project-writes: 1 pass, 0 fail
