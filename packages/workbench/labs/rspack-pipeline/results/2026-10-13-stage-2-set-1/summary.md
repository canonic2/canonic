# Rspack pipeline stage 2, run set 1

Samples per cell: 10, warmups: 3.

| Fixture | Sequence | Cell | Variant | n | errors | p50 ms | p95 ms | max ms |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| formats | N7 | between adapters | 0 | 23 | 0 | 189 | 207 | 467 |
| formats | N7 | between adapters | 1 | 23 | 0 | 186 | 204 | 467 |
| formats | N7 | between adapters | 2 | 23 | 0 | 186 | 217 | 252 |
| formats | N7 | between adapters | 3 | 23 | 0 | 151 | 192 | 232 |
| formats | E1 | cssGlobal | 0 | 10 | 0 | 699 | 719 | 719 |
| formats | E1 | cssGlobal | 2 | 10 | 0 | 69 | 97 | 97 |
| formats | E1 | cssGlobal | 3 | 10 | 0 | 66 | 116 | 116 |
| formats | E1 | cssModule | 0 | 10 | 0 | 699 | 753 | 753 |
| formats | E1 | cssModule | 2 | 10 | 0 | 61 | 73 | 73 |
| formats | E1 | cssModule | 3 | 10 | 0 | 65 | 99 | 99 |
| formats | E1 | vueScoped | 0 | 10 | 0 | 619 | 636 | 636 |
| formats | E1 | vueScoped | 2 | 10 | 0 | 75 | 80 | 80 |
| formats | E1 | vueScoped | 3 | 10 | 0 | 66 | 83 | 83 |
| formats | E2 | reactComponent | 0 | 10 | 0 | 698 | 721 | 721 |
| formats | E2 | reactComponent | 2 | 10 | 0 | 90 | 97 | 97 |
| formats | E2 | reactComponent | 3 | 10 | 0 | 89 | 123 | 123 |
| formats | E3 | nonComponentModule | 0 | 10 | 0 | 703 | 734 | 734 |
| formats | E3 | nonComponentModule | 2 | 10 | 0 | 94 | 113 | 113 |
| formats | E3 | nonComponentModule | 3 | 10 | 0 | 93 | 124 | 124 |
| formats | E4 | vueScript | 0 | 10 | 0 | 616 | 632 | 632 |
| formats | E4 | vueScript | 2 | 10 | 0 | 76 | 99 | 99 |
| formats | E4 | vueScript | 3 | 10 | 0 | 64 | 79 | 79 |
| formats | E4 | vueTemplate | 0 | 10 | 0 | 629 | 697 | 697 |
| formats | E4 | vueTemplate | 2 | 10 | 0 | 70 | 97 | 97 |
| formats | E4 | vueTemplate | 3 | 10 | 0 | 68 | 85 | 85 |
| formats | E5 | htmlMarkup | 0 | 10 | 0 | 583 | 649 | 649 |
| formats | E5 | htmlMarkup | 2 | 10 | 0 | 146 | 183 | 183 |
| formats | E5 | htmlMarkup | 3 | 10 | 0 | 149 | 185 | 185 |
| formats | E5 | htmlStylesheet | 0 | 10 | 0 | 590 | 605 | 605 |
| formats | E5 | htmlStylesheet | 2 | 10 | 0 | 70 | 92 | 92 |
| formats | E5 | htmlStylesheet | 3 | 10 | 0 | 63 | 100 | 100 |
| formats | E6 | astroComponent | 0 | 10 | 0 | 589 | 614 | 614 |
| formats | E6 | astroComponent | 2 | 10 | 0 | 173 | 180 | 180 |
| formats | E6 | astroComponent | 3 | 10 | 0 | 168 | 178 | 178 |
| formats | E7 | docsExample | 0 | 10 | 0 | 593 | 625 | 625 |
| formats | E7 | docsExample | 2 | 10 | 0 | 94 | 98 | 98 |
| formats | E7 | docsExample | 3 | 10 | 0 | 92 | 122 | 122 |
| formats | E7 | docsMarkdown | 0 | 10 | 0 | 489 | 516 | 516 |
| formats | E7 | docsMarkdown | 2 | 10 | 0 | 48 | 61 | 61 |
| formats | E7 | docsMarkdown | 3 | 10 | 0 | 49 | 64 | 64 |
| formats | E8 | config | 0 | 10 | 0 | 619 | 744 | 744 |
| formats | E8 | config | 2 | 10 | 0 | 580 | 647 | 647 |
| formats | E8 | config | 3 | 10 | 0 | 547 | 620 | 620 |
| formats | E8 | definition | 0 | 9 | 1 | -17 | 1106 | 1106 |
| formats | E8 | definition | 2 | 10 | 0 | -13 | 244 | 244 |
| formats | E8 | definition | 3 | 10 | 0 | -17 | 252 | 252 |
| formats | E8 | environment | 0 | 10 | 0 | 724 | 772 | 772 |
| formats | E8 | environment | 2 | 10 | 0 | 231 | 264 | 264 |
| formats | E8 | environment | 3 | 10 | 0 | 236 | 249 | 249 |
| formats | E9 | shared | 0 | 10 | 0 | 710 | 759 | 759 |
| formats | E9 | shared | 2 | 10 | 0 | 109 | 146 | 146 |
| formats | E9 | shared | 3 | 10 | 0 | 104 | 112 | 112 |
| huge | A1 | cold, from server start | 0 | 10 | 0 | 10899 | 16713 | 16713 |
| huge | A1 | cold, from server start | 2 | 10 | 0 | 10379 | 17002 | 17002 |
| huge | A1 | cold, from server start | D2 | 10 | 0 | 9800 | 10454 | 10454 |
| huge | A2 | hot, from server start | 0 | 10 | 0 | 5147 | 9601 | 9601 |
| huge | A2 | hot, from server start | 2 | 10 | 0 | 8197 | 13373 | 13373 |
| huge | A2 | hot, from server start | D2 | 10 | 0 | 7597 | 8039 | 8039 |
| huge | A3 | from URL | 0 | 10 | 0 | 4714 | 9159 | 9159 |
| huge | A3 | from URL | 2 | 10 | 0 | 7764 | 12869 | 12869 |
| huge | A3 | from URL | D2 | 10 | 0 | 7155 | 7597 | 7597 |
| huge | A3-cold | from URL | 0 | 10 | 0 | 10467 | 16259 | 16259 |
| huge | A3-cold | from URL | 2 | 10 | 0 | 9954 | 16494 | 16494 |
| huge | A3-cold | from URL | D2 | 10 | 0 | 9371 | 10032 | 10032 |
| huge | A4 | after pre-warm | 0 | 10 | 0 | 1911 | 3284 | 3284 |
| huge | A4 | after pre-warm | 2 | 10 | 0 | 985 | 2847 | 2847 |
| huge | A4 | after pre-warm | D2 | 10 | 0 | 5356 | 5586 | 5586 |
| huge | N1 | second page | 0 | 10 | 0 | 1364 | 1460 | 1460 |
| huge | N1 | second page | 2 | 10 | 0 | 322 | 348 | 348 |
| huge | N1 | second page | 3 | 10 | 0 | 182 | 228 | 228 |
| huge | N2 | paced | 0 | 10 | 0 | 402 | 440 | 440 |
| huge | N2 | paced | 2 | 10 | 0 | 233 | 268 | 268 |
| huge | N2 | paced | 3 | 10 | 0 | 155 | 181 | 181 |
| huge | N3 | back to back | 0 | 10 | 0 | 1367 | 1484 | 1484 |
| huge | N3 | back to back | 2 | 10 | 0 | 231 | 257 | 257 |
| huge | N3 | back to back | 3 | 10 | 0 | 140 | 161 | 161 |
| huge | N4 | retained | 0 | 10 | 0 | 54 | 63 | 63 |
| huge | N4 | retained | 2 | 10 | 0 | 53 | 59 | 59 |
| huge | N4 | retained | 3 | 10 | 0 | 105 | 114 | 114 |
| huge | N5 | size | 0 | 10 | 0 | 24 | 35 | 35 |
| huge | N5 | size | 2 | 10 | 0 | 36 | 39 | 39 |
| huge | N5 | size | 3 | 10 | 0 | 32 | 47 | 47 |
| huge | N5 | state | 0 | 10 | 0 | 55 | 60 | 60 |
| huge | N5 | state | 2 | 10 | 0 | 53 | 58 | 58 |
| huge | N5 | state | 3 | 10 | 0 | 136 | 152 | 152 |
| huge | N6 | reload | 0 | 10 | 0 | 172 | 290 | 290 |
| huge | N6 | reload | 2 | 10 | 0 | 191 | 208 | 208 |
| huge | N6 | reload | 3 | 10 | 0 | 84 | 99 | 99 |
| large-system | A1 | cold, from server start | 0 | 10 | 0 | 5684 | 6077 | 6077 |
| large-system | A1 | cold, from server start | 2 | 10 | 0 | 6438 | 9265 | 9265 |
| large-system | A1 | cold, from server start | D2 | 10 | 0 | 6167 | 9021 | 9021 |
| large-system | A2 | hot, from server start | 0 | 10 | 0 | 1893 | 2081 | 2081 |
| large-system | A2 | hot, from server start | 2 | 10 | 0 | 3709 | 4188 | 4188 |
| large-system | A2 | hot, from server start | D2 | 10 | 0 | 3655 | 10912 | 10912 |
| large-system | A3 | from URL | 0 | 10 | 0 | 1528 | 1705 | 1705 |
| large-system | A3 | from URL | 2 | 10 | 0 | 3322 | 3684 | 3684 |
| large-system | A3 | from URL | D2 | 10 | 0 | 3260 | 9810 | 9810 |
| large-system | A3-cold | from URL | 0 | 10 | 0 | 5306 | 5701 | 5701 |
| large-system | A3-cold | from URL | 2 | 10 | 0 | 6052 | 8824 | 8824 |
| large-system | A3-cold | from URL | D2 | 10 | 0 | 5780 | 8632 | 8632 |
| large-system | A4 | after pre-warm | 0 | 10 | 0 | 671 | 703 | 703 |
| large-system | A4 | after pre-warm | 2 | 10 | 0 | 812 | 959 | 959 |
| large-system | A4 | after pre-warm | D2 | 10 | 0 | 2377 | 2587 | 2587 |
| large-system | N1 | second page | 0 | 10 | 0 | 1377 | 1575 | 1575 |
| large-system | N1 | second page | 1 | 10 | 0 | 1374 | 1432 | 1432 |
| large-system | N1 | second page | 2 | 10 | 0 | 321 | 350 | 350 |
| large-system | N1 | second page | 3 | 10 | 0 | 172 | 555 | 555 |
| large-system | N1 | second page | D3 | 10 | 0 | 335 | 395 | 395 |
| large-system | N2 | paced | 0 | 10 | 0 | 396 | 435 | 435 |
| large-system | N2 | paced | 1 | 10 | 0 | 411 | 450 | 450 |
| large-system | N2 | paced | 2 | 10 | 0 | 236 | 284 | 284 |
| large-system | N2 | paced | 3 | 10 | 0 | 179 | 257 | 257 |
| large-system | N2 | paced | D3 | 10 | 0 | 205 | 228 | 228 |
| large-system | N3 | back to back | 0 | 10 | 0 | 1398 | 1493 | 1493 |
| large-system | N3 | back to back | 1 | 10 | 0 | 1445 | 1615 | 1615 |
| large-system | N3 | back to back | 2 | 10 | 0 | 230 | 310 | 310 |
| large-system | N3 | back to back | 3 | 10 | 0 | 137 | 164 | 164 |
| large-system | N3 | back to back | D3 | 10 | 0 | 195 | 220 | 220 |
| large-system | N4 | retained | 0 | 10 | 0 | 55 | 66 | 66 |
| large-system | N4 | retained | 1 | 10 | 0 | 62 | 65 | 65 |
| large-system | N4 | retained | 2 | 10 | 0 | 52 | 60 | 60 |
| large-system | N4 | retained | 3 | 10 | 0 | 98 | 118 | 118 |
| large-system | N4 | retained | D3 | 10 | 0 | 51 | 56 | 56 |
| large-system | N5 | size | 0 | 10 | 0 | 17 | 34 | 34 |
| large-system | N5 | size | 1 | 10 | 0 | 29 | 46 | 46 |
| large-system | N5 | size | 2 | 10 | 0 | 22 | 31 | 31 |
| large-system | N5 | size | 3 | 10 | 0 | 16 | 29 | 29 |
| large-system | N5 | size | D3 | 10 | 0 | 17 | 27 | 27 |
| large-system | N5 | state | 0 | 10 | 0 | 53 | 64 | 64 |
| large-system | N5 | state | 1 | 10 | 0 | 51 | 62 | 62 |
| large-system | N5 | state | 2 | 10 | 0 | 46 | 52 | 52 |
| large-system | N5 | state | 3 | 10 | 0 | 143 | 153 | 153 |
| large-system | N5 | state | D3 | 10 | 0 | 51 | 56 | 56 |
| large-system | N6 | reload | 0 | 10 | 0 | 170 | 287 | 287 |
| large-system | N6 | reload | 1 | 10 | 0 | 173 | 204 | 204 |
| large-system | N6 | reload | 2 | 10 | 0 | 193 | 213 | 213 |
| large-system | N6 | reload | 3 | 10 | 0 | 90 | 122 | 122 |
| large-system | N6 | reload | D3 | 10 | 0 | 188 | 195 | 195 |
| large-system | E2 | React component | 0 | 10 | 0 | 1924 | 1997 | 1997 |
| large-system | E2 | React component | 2 | 10 | 0 | 159 | 218 | 218 |
| large-system | E2 | React component | 3 | 10 | 0 | 151 | 200 | 200 |
| large-system | E3 | module exporting more than components | 0 | 10 | 0 | 2051 | 2173 | 2173 |
| large-system | E3 | module exporting more than components | 2 | 10 | 0 | 174 | 183 | 183 |
| large-system | E3 | module exporting more than components | 3 | 10 | 0 | 179 | 267 | 267 |
| large-system | E8 | environment | 0 | 10 | 0 | 2045 | 2143 | 2143 |
| large-system | E8 | environment | 2 | 10 | 0 | 454 | 607 | 607 |
| large-system | E8 | environment | 3 | 10 | 0 | 437 | 490 | 490 |
| large-system | E8 | preview definition | 0 | 10 | 0 | 2063 | 2200 | 2200 |
| large-system | E8 | preview definition | 2 | 10 | 0 | 350 | 434 | 434 |
| large-system | E8 | preview definition | 3 | 10 | 0 | 354 | 461 | 461 |
| large-system | E9 | module shared by every page | 0 | 10 | 0 | 2035 | 2142 | 2142 |
| large-system | E9 | module shared by every page | 2 | 10 | 0 | 171 | 200 | 200 |
| large-system | E9 | module shared by every page | 3 | 10 | 0 | 169 | 184 | 184 |
| large-web | N1 | second page | 0 | 10 | 0 | 2673 | 3197 | 3197 |
| large-web | N1 | second page | 1 | 10 | 0 | 2523 | 2592 | 2592 |
| large-web | N1 | second page | 2 | 10 | 0 | 386 | 593 | 593 |
| large-web | N1 | second page | 3 | 10 | 0 | 250 | 323 | 323 |
| large-web | N1 | second page | D3 | 10 | 0 | 468 | 817 | 817 |
| large-web | N2 | paced | 0 | 10 | 0 | 1276 | 1309 | 1309 |
| large-web | N2 | paced | 1 | 10 | 0 | 492 | 520 | 520 |
| large-web | N2 | paced | 2 | 10 | 0 | 365 | 992 | 992 |
| large-web | N2 | paced | 3 | 10 | 0 | 164 | 180 | 180 |
| large-web | N2 | paced | D3 | 10 | 0 | 237 | 344 | 344 |
| large-web | N2 | paced, +150 ms | 0 | 10 | 0 | 1564 | 1636 | 1636 |
| large-web | N2 | paced, +150 ms | 1 | 10 | 0 | 596 | 634 | 634 |
| large-web | N2 | paced, +150 ms | 2 | 10 | 0 | 492 | 557 | 557 |
| large-web | N2 | paced, +150 ms | 3 | 10 | 0 | 354 | 434 | 434 |
| large-web | N2 | paced, +150 ms | D3 | 10 | 0 | 228 | 351 | 351 |
| large-web | N3 | back to back | 0 | 10 | 0 | 2617 | 4963 | 4963 |
| large-web | N3 | back to back | 1 | 10 | 0 | 2348 | 2600 | 2600 |
| large-web | N3 | back to back | 2 | 10 | 0 | 266 | 402 | 402 |
| large-web | N3 | back to back | 3 | 10 | 0 | 176 | 203 | 203 |
| large-web | N3 | back to back | D3 | 10 | 0 | 237 | 263 | 263 |
| large-web | N4 | retained | 0 | 10 | 0 | 60 | 65 | 65 |
| large-web | N4 | retained | 1 | 10 | 0 | 62 | 68 | 68 |
| large-web | N4 | retained | 2 | 10 | 0 | 46 | 63 | 63 |
| large-web | N4 | retained | 3 | 10 | 0 | 114 | 141 | 141 |
| large-web | N4 | retained | D3 | 10 | 0 | 47 | 59 | 59 |
| large-web | N5 | size | 0 | 10 | 0 | 21 | 35 | 35 |
| large-web | N5 | size | 1 | 10 | 0 | 18 | 33 | 33 |
| large-web | N5 | size | 2 | 10 | 0 | 21 | 33 | 33 |
| large-web | N5 | size | 3 | 10 | 0 | 16 | 35 | 35 |
| large-web | N5 | size | D3 | 10 | 0 | 20 | 36 | 36 |
| large-web | N5 | state | 0 | 10 | 0 | 50 | 67 | 67 |
| large-web | N5 | state | 1 | 10 | 0 | 53 | 69 | 69 |
| large-web | N5 | state | 2 | 10 | 0 | 47 | 54 | 54 |
| large-web | N5 | state | 3 | 10 | 0 | 162 | 194 | 194 |
| large-web | N5 | state | D3 | 10 | 0 | 46 | 58 | 58 |
| large-web | N6 | reload | 0 | 10 | 0 | 182 | 199 | 199 |
| large-web | N6 | reload | 1 | 10 | 0 | 184 | 217 | 217 |
| large-web | N6 | reload | 2 | 10 | 0 | 214 | 356 | 356 |
| large-web | N6 | reload | 3 | 10 | 0 | 107 | 123 | 123 |
| large-web | N6 | reload | D3 | 10 | 0 | 227 | 283 | 283 |
| large-web | N8 | new font | 0 | 10 | 0 | 2523 | 3944 | 3944 |
| large-web | N8 | new font | 1 | 10 | 0 | 1767 | 1977 | 1977 |
| large-web | N8 | new font | 2 | 10 | 0 | 377 | 662 | 662 |
| large-web | N8 | new font | 3 | 10 | 0 | 244 | 410 | 410 |
| large-web | N8 | new font | D3 | 10 | 0 | 362 | 579 | 579 |
| large-web | N8 | new font, +150 ms | 0 | 9 | 1 | 1581 | 1665 | 1665 |
| large-web | N8 | new font, +150 ms | 1 | 10 | 0 | 711 | 797 | 797 |
| large-web | N8 | new font, +150 ms | 2 | 10 | 0 | 649 | 814 | 814 |
| large-web | N8 | new font, +150 ms | 3 | 10 | 0 | 518 | 593 | 593 |
| large-web | N8 | new font, +150 ms | D3 | 10 | 0 | 663 | 881 | 881 |
| small | A1 | cold, from server start | 0 | 10 | 0 | 4528 | 5246 | 5246 |
| small | A1 | cold, from server start | 2 | 10 | 0 | 4263 | 10140 | 10140 |
| small | A1 | cold, from server start | D2 | 10 | 0 | 4310 | 12279 | 12279 |
| small | A2 | hot, from server start | 0 | 10 | 0 | 1692 | 1938 | 1938 |
| small | A2 | hot, from server start | 2 | 10 | 0 | 1495 | 4088 | 4088 |
| small | A2 | hot, from server start | D2 | 10 | 0 | 1770 | 1939 | 1939 |
| small | A3 | from URL | 0 | 10 | 0 | 1310 | 1530 | 1530 |
| small | A3 | from URL | 2 | 10 | 0 | 1154 | 2350 | 2350 |
| small | A3 | from URL | D2 | 10 | 0 | 1429 | 1579 | 1579 |
| small | A3-cold | from URL | 0 | 10 | 0 | 4152 | 4865 | 4865 |
| small | A3-cold | from URL | 2 | 10 | 0 | 3921 | 9722 | 9722 |
| small | A3-cold | from URL | D2 | 10 | 0 | 3973 | 11465 | 11465 |
| small | A4 | after pre-warm | 0 | 10 | 0 | 352 | 398 | 398 |
| small | A4 | after pre-warm | 2 | 10 | 0 | 306 | 398 | 398 |
| small | A4 | after pre-warm | D2 | 10 | 0 | 603 | 1592 | 1592 |
| small | N1 | second page | 0 | 10 | 0 | 197 | 446 | 446 |
| small | N1 | second page | 1 | 10 | 0 | 128 | 290 | 290 |
| small | N1 | second page | 2 | 10 | 0 | 82 | 162 | 162 |
| small | N1 | second page | 3 | 10 | 0 | 62 | 113 | 113 |
| small | N1 | second page | D3 | 10 | 0 | 79 | 83 | 83 |
| small | N2 | paced | 0 | 1 | 0 | 222 | 222 | 222 |
| small | N2 | paced | 1 | 1 | 0 | 150 | 150 | 150 |
| small | N2 | paced | 2 | 1 | 0 | 103 | 103 | 103 |
| small | N2 | paced | 3 | 1 | 0 | 41 | 41 | 41 |
| small | N2 | paced | D3 | 1 | 0 | 104 | 104 | 104 |
| small | N4 | retained | 0 | 10 | 0 | 44 | 56 | 56 |
| small | N4 | retained | 1 | 10 | 0 | 45 | 58 | 58 |
| small | N4 | retained | 2 | 10 | 0 | 50 | 58 | 58 |
| small | N4 | retained | 3 | 10 | 0 | 50 | 145 | 145 |
| small | N4 | retained | D3 | 10 | 0 | 46 | 49 | 49 |
| small | N5 | size | 0 | 10 | 0 | 25 | 30 | 30 |
| small | N5 | size | 1 | 10 | 0 | 30 | 32 | 32 |
| small | N5 | size | 2 | 10 | 0 | 20 | 35 | 35 |
| small | N5 | size | 3 | 10 | 0 | 24 | 32 | 32 |
| small | N5 | size | D3 | 10 | 0 | 25 | 29 | 29 |
| small | N5 | state | 0 | 10 | 0 | 48 | 67 | 67 |
| small | N5 | state | 1 | 10 | 0 | 45 | 53 | 53 |
| small | N5 | state | 2 | 10 | 0 | 45 | 54 | 54 |
| small | N5 | state | 3 | 10 | 0 | 46 | 158 | 158 |
| small | N5 | state | D3 | 10 | 0 | 43 | 56 | 56 |
| small | N6 | reload | 0 | 10 | 0 | 77 | 108 | 108 |
| small | N6 | reload | 1 | 10 | 0 | 78 | 96 | 96 |
| small | N6 | reload | 2 | 10 | 0 | 74 | 83 | 83 |
| small | N6 | reload | 3 | 10 | 0 | 37 | 72 | 72 |
| small | N6 | reload | D3 | 10 | 0 | 139 | 256 | 256 |
| small | E2 | React component | 0 | 10 | 0 | 836 | 877 | 877 |
| small | E2 | React component | 2 | 10 | 0 | 60 | 71 | 71 |
| small | E2 | React component | 3 | 10 | 0 | 59 | 72 | 72 |

## Values

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
- huge D2 cache compile cache bytes after cold start #3: 389.2 MB
- huge D2 cache compile cache bytes after cold start #4: 389.2 MB
- huge D2 cache compile cache bytes after cold start #5: 389.2 MB
- huge D2 cache compile cache bytes after cold start #6: 389.2 MB
- huge D2 cache compile cache bytes after cold start #7: 389.2 MB
- huge D2 cache compile cache bytes after cold start #8: 389.2 MB
- huge D2 cache compile cache bytes after cold start #9: 389.2 MB
- huge D2 cache compile cache bytes after cold start #10: 389.2 MB
- huge D2 cache compile cache bytes after cold start #11: 389.2 MB
- huge D2 cache compile cache bytes after cold start #12: 389.2 MB
- huge 2 cache compile cache bytes after cold start #3: 389.2 MB
- huge 2 cache compile cache bytes after cold start #4: 389.2 MB
- huge 2 cache compile cache bytes after cold start #5: 389.2 MB
- huge 2 cache compile cache bytes after cold start #6: 389.2 MB
- huge 2 cache compile cache bytes after cold start #7: 389.2 MB
- huge 2 cache compile cache bytes after cold start #8: 389.2 MB
- huge 2 cache compile cache bytes after cold start #9: 389.2 MB
- huge 2 cache compile cache bytes after cold start #10: 389.2 MB
- huge 2 cache compile cache bytes after cold start #11: 389.2 MB
- huge 2 cache compile cache bytes after cold start #12: 389.2 MB
- small D3 heap after N2 and N3: 9.5 MB
- small D3 worker peak RSS: 547.3 MB
- small 0 heap after N2 and N3: 11.1 MB
- small 0 worker peak RSS: 540.4 MB
- small 1 heap after N2 and N3: 11.1 MB
- small 1 worker peak RSS: 561.5 MB
- small 3 heap after N2 and N3: 7.7 MB
- small 3 worker peak RSS: 541.5 MB
- small 2 heap after N2 and N3: 8.9 MB
- small 2 worker peak RSS: 545.4 MB
- large-system 3 heap after N2 and N3: 162.2 MB
- large-system 3 worker peak RSS: 1121.1 MB
- large-system 2 heap after N2 and N3: 295.4 MB
- large-system 2 worker peak RSS: 1131.5 MB
- large-system D3 heap after N2 and N3: 286.8 MB
- large-system D3 worker peak RSS: 1135.8 MB
- large-system 0 heap after N2 and N3: 154.7 MB
- large-system 0 worker peak RSS: 1798.3 MB
- large-system 1 heap after N2 and N3: 154.9 MB
- large-system 1 worker peak RSS: 1688.8 MB
- large-web 3 heap after N2 and N3: 138.2 MB
- large-web 3 worker peak RSS: 1557.2 MB
- large-web D3 heap after N2 and N3: 227.1 MB
- large-web D3 worker peak RSS: 1530.2 MB
- large-web 2 heap after N2 and N3: 297.3 MB
- large-web 2 worker peak RSS: 1526.6 MB
- large-web 0 heap after N2 and N3: 145.8 MB
- large-web 0 worker peak RSS: 2568.1 MB
- large-web 1 heap after N2 and N3: 153.6 MB
- large-web 1 worker peak RSS: 2525.0 MB
- huge 2 heap after N2 and N3: 268.2 MB
- huge 2 worker peak RSS: 2038.4 MB
- huge 0 heap after N2 and N3: 149.5 MB
- huge 0 worker peak RSS: 1649.6 MB
- huge 3 heap after N2 and N3: 147.6 MB
- huge 3 worker peak RSS: 2021.9 MB
- formats 3 worker peak RSS: 704.5 MB
- formats 0 worker peak RSS: 695.9 MB
- formats 1 worker peak RSS: 665.3 MB
- formats 2 worker peak RSS: 719.6 MB

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
- 3 large-system freshness: 0 pass, 1 fail
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
