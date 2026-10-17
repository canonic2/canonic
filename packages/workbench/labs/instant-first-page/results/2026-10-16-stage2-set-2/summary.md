# Instant first page lab, stage 2, stage2-set-2

Samples per start cell: 10 (warmups 3); per session step: 5. Times in ms from the step's own start (first page and page list from the canvas's navigation), p50 / p95 / max by nearest rank. Missing counts samples with no time: timeouts and errors.

| Fixture | Sequence | Cell | Variant | n | missing | p50 | p95 | max | max load |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| small | S1 | first page | 0 | 10 | 0 | 1243 | 1284 | 1284 | 8.6 |
| small | S1 | first page | R | 10 | 0 | 639 | 738 | 738 | 8.1 |
| small | S1 | first page | RN | 10 | 0 | 653 | 1019 | 1019 | 8.5 |
| small | S1 | page list | 0 | 10 | 0 | 239 | 268 | 268 | 8.6 |
| small | S1 | page list | R | 10 | 0 | 245 | 317 | 317 | 8.1 |
| small | S1 | page list | RN | 10 | 0 | 244 | 357 | 357 | 8.5 |
| small | S2 | first page | 0 | 10 | 0 | 1234 | 1313 | 1313 | 8.0 |
| small | S2 | first page | R | 10 | 0 | 372 | 389 | 389 | 8.0 |
| small | S2 | first page | RN | 10 | 0 | 374 | 420 | 420 | 8.1 |
| small | S2 | page list | 0 | 10 | 0 | 207 | 229 | 229 | 8.0 |
| small | S2 | page list | R | 10 | 0 | 213 | 237 | 237 | 8.0 |
| small | S2 | page list | RN | 10 | 0 | 211 | 248 | 248 | 8.1 |
| small | S2i | first page | 0 | 10 | 0 | 1224 | 1296 | 1296 | 9.3 |
| small | S2i | first page | R | 10 | 0 | 365 | 387 | 387 | 8.0 |
| small | S2i | first page | RN | 10 | 0 | 366 | 416 | 416 | 8.0 |
| small | S2i | page list | 0 | 10 | 0 | 204 | 226 | 226 | 9.3 |
| small | S2i | page list | R | 10 | 0 | 211 | 227 | 227 | 8.0 |
| small | S2i | page list | RN | 10 | 0 | 211 | 262 | 262 | 8.0 |
| small | S3 | first page | 0 | 10 | 0 | 1283 | 1359 | 1359 | 8.0 |
| small | S3 | first page | R | 10 | 0 | 388 | 426 | 426 | 8.0 |
| small | S3 | first page | RN | 10 | 0 | 394 | 845 | 845 | 21.0 |
| small | S3 | page list | 0 | 10 | 0 | 225 | 273 | 273 | 8.0 |
| small | S3 | page list | R | 10 | 0 | 225 | 270 | 270 | 8.0 |
| small | S3 | page list | RN | 10 | 0 | 228 | 407 | 407 | 21.0 |
| small | S4 | page not yet opened | 0 | 10 | 0 | 62.3 | 66.8 | 66.8 | 8.0 |
| small | S4 | page not yet opened | R | 10 | 0 | 63.1 | 78.0 | 78.0 | 8.0 |
| small | S4 | page not yet opened | RN | 10 | 0 | 64.3 | 85.6 | 85.6 | 8.1 |
| small | own | own build | R | 10 | 0 | 61.5 | 64.0 | 64.0 | 8.0 |
| small | own | own build | RN | 10 | 0 | 62.3 | 124 | 124 | 36.3 |
| large-system | S1 | first page | 0 | 10 | 0 | 2627 | 2764 | 2764 | 9.8 |
| large-system | S1 | first page | R | 10 | 0 | 1524 | 1633 | 1633 | 11.3 |
| large-system | S1 | first page | RN | 10 | 0 | 1554 | 1624 | 1624 | 8.1 |
| large-system | S1 | page list | 0 | 10 | 0 | 247 | 260 | 260 | 9.8 |
| large-system | S1 | page list | R | 10 | 0 | 235 | 253 | 253 | 11.3 |
| large-system | S1 | page list | RN | 10 | 0 | 237 | 255 | 255 | 8.1 |
| large-system | S2 | first page | 0 | 10 | 0 | 3219 | 6221 | 6221 | 10.0 |
| large-system | S2 | first page | R | 10 | 0 | 1114 | 1192 | 1192 | 8.7 |
| large-system | S2 | first page | RN | 10 | 0 | 1155 | 1188 | 1188 | 7.8 |
| large-system | S2 | page list | 0 | 10 | 0 | 231 | 3521 | 3521 | 10.0 |
| large-system | S2 | page list | R | 10 | 0 | 217 | 226 | 226 | 8.7 |
| large-system | S2 | page list | RN | 10 | 0 | 224 | 259 | 259 | 7.8 |
| large-system | S2i | first page | 0 | 10 | 0 | 3101 | 3360 | 3360 | 10.3 |
| large-system | S2i | first page | R | 10 | 0 | 1087 | 1121 | 1121 | 8.5 |
| large-system | S2i | first page | RN | 10 | 0 | 1137 | 1174 | 1174 | 8.6 |
| large-system | S2i | page list | 0 | 10 | 0 | 215 | 241 | 241 | 10.3 |
| large-system | S2i | page list | R | 10 | 0 | 218 | 226 | 226 | 8.5 |
| large-system | S2i | page list | RN | 10 | 0 | 224 | 237 | 237 | 8.6 |
| large-system | S3 | first page | 0 | 10 | 0 | 3137 | 3375 | 3375 | 8.8 |
| large-system | S3 | first page | R | 10 | 0 | 1157 | 1230 | 1230 | 9.9 |
| large-system | S3 | first page | RN | 10 | 0 | 1198 | 1332 | 1332 | 8.4 |
| large-system | S3 | page list | 0 | 10 | 0 | 209 | 228 | 228 | 8.8 |
| large-system | S3 | page list | R | 10 | 0 | 235 | 252 | 252 | 9.9 |
| large-system | S3 | page list | RN | 10 | 0 | 239 | 257 | 257 | 8.4 |
| large-system | S4 | page not yet opened | 0 | 10 | 0 | 159 | 276 | 276 | 10.0 |
| large-system | S4 | page not yet opened | R | 10 | 0 | 251 | 291 | 291 | 8.7 |
| large-system | S4 | page not yet opened | RN | 10 | 0 | 246 | 286 | 286 | 7.8 |
| large-system | S5 | selection to visible | 0 | 5 | 0 | 144 | 159 | 159 | 6.2 |
| large-system | S5 | selection to visible | R | 5 | 0 | 217 | 244 | 244 | 7.8 |
| large-system | S5 | selection to visible | RN | 5 | 0 | 251 | 300 | 300 | 7.8 |
| large-system | S5r | selection to visible | 0 | 5 | 0 | 126 | 131 | 131 | 6.0 |
| large-system | S5r | selection to visible | R | 5 | 0 | 107 | 128 | 128 | 7.3 |
| large-system | S5r | selection to visible | RN | 5 | 0 | 150 | 475 | 475 | 7.2 |
| large-system | S6 | component, first page | 0 | 5 | 0 | 162 | 167 | 167 | 6.2 |
| large-system | S6 | component, first page | R | 5 | 0 | 132 | 148 | 148 | 6.9 |
| large-system | S6 | component, first page | RN | 5 | 0 | 214 | 216 | 216 | 6.9 |
| large-system | S6 | component, page opened later | 0 | 5 | 0 | 150 | 166 | 166 | 6.2 |
| large-system | S6 | component, page opened later | R | 5 | 0 | 131 | 147 | 147 | 6.7 |
| large-system | S6 | component, page opened later | RN | 5 | 0 | 205 | 211 | 211 | 6.9 |
| large-system | S6 | shared module, first page | 0 | 5 | 0 | 165 | 191 | 191 | 6.2 |
| large-system | S6 | shared module, first page | R | 5 | 0 | 147 | 151 | 151 | 6.7 |
| large-system | S6 | shared module, first page | RN | 5 | 0 | 214 | 351 | 351 | 6.9 |
| large-system | S7 | config edit | 0 | 5 | 0 | 1962 | 2113 | 2113 | 9.1 |
| large-system | S7 | config edit | R | 5 | 0 | 4324 | 4517 | 4517 | 7.5 |
| large-system | S7 | config edit | RN | 5 | 0 | 4240 | 4378 | 4378 | 7.2 |
| large-system | S8 | added | 0 | 1 | 0 | 298 | 298 | 298 | 7.7 |
| large-system | S8 | added | R | 1 | 0 | 266 | 266 | 266 | 7.8 |
| large-system | S8 | added | RN | 1 | 0 | 230 | 230 | 230 | 5.9 |
| large-system | S8 | renamed | 0 | 1 | 0 | 28.0 | 28.0 | 28.0 | 7.7 |
| large-system | S8 | renamed | R | 1 | 0 | 28.0 | 28.0 | 28.0 | 7.8 |
| large-system | S8 | renamed | RN | 1 | 0 | 26.0 | 26.0 | 26.0 | 5.9 |
| large-system | S8 | retitled | 0 | 1 | 0 | 27.0 | 27.0 | 27.0 | 7.7 |
| large-system | S8 | retitled | R | 1 | 0 | 92.0 | 92.0 | 92.0 | 7.8 |
| large-system | S8 | retitled | RN | 1 | 0 | 26.0 | 26.0 | 26.0 | 5.9 |
| large-system | S9-S4 | selection to visible | 0 | 1 | 0 | 140 | 140 | 140 | 7.4 |
| large-system | S9-S4 | selection to visible | R | 1 | 0 | 340 | 340 | 340 | 7.6 |
| large-system | S9-S4 | selection to visible | RN | 1 | 0 | 338 | 338 | 338 | 7.2 |
| large-system | S9-S6 | component, first page | 0 | 5 | 0 | 181 | 202 | 202 | 7.4 |
| large-system | S9-S6 | component, first page | R | 5 | 0 | 232 | 245 | 245 | 7.6 |
| large-system | S9-S6 | component, first page | RN | 5 | 0 | 230 | 385 | 385 | 7.2 |
| large-system | own | own build | R | 10 | 0 | 244 | 258 | 258 | 8.0 |
| large-system | own | own build | RN | 10 | 0 | 248 | 266 | 266 | 7.8 |
| giant | S1 | first page | 0 | 10 | 0 | 10886 | 11344 | 11344 | 12.5 |
| giant | S1 | first page | R | 10 | 0 | 1671 | 1829 | 1829 | 8.5 |
| giant | S1 | first page | RN | 10 | 0 | 1700 | 1746 | 1746 | 8.0 |
| giant | S1 | page list | 0 | 10 | 0 | 425 | 453 | 453 | 12.5 |
| giant | S1 | page list | R | 10 | 0 | 420 | 464 | 464 | 8.5 |
| giant | S1 | page list | RN | 10 | 0 | 432 | 458 | 458 | 8.0 |
| giant | S2 | first page | 0 | 10 | 0 | 16425 | 17878 | 17878 | 10.4 |
| giant | S2 | first page | R | 10 | 0 | 1278 | 1369 | 1369 | 8.0 |
| giant | S2 | first page | RN | 10 | 0 | 1306 | 1375 | 1375 | 8.0 |
| giant | S2 | page list | 0 | 10 | 0 | 415 | 445 | 445 | 10.4 |
| giant | S2 | page list | R | 10 | 0 | 409 | 416 | 416 | 8.0 |
| giant | S2 | page list | RN | 10 | 0 | 414 | 432 | 432 | 8.0 |
| giant | S2i | first page | 0 | 10 | 0 | 16438 | 18782 | 18782 | 10.4 |
| giant | S2i | first page | R | 9 | 1 | 1227 | 1348 | 1348 | 8.2 |
| giant | S2i | first page | RN | 10 | 0 | 1283 | 1353 | 1353 | 9.0 |
| giant | S2i | page list | 0 | 10 | 0 | 428 | 492 | 492 | 10.4 |
| giant | S2i | page list | R | 9 | 0 | 410 | 441 | 441 | 8.2 |
| giant | S2i | page list | RN | 10 | 0 | 417 | 442 | 442 | 9.0 |
| giant | S3 | first page | 0 | 10 | 0 | 16644 | 18086 | 18086 | 9.0 |
| giant | S3 | first page | R | 10 | 0 | 1297 | 1411 | 1411 | 7.7 |
| giant | S3 | first page | RN | 10 | 0 | 1341 | 1426 | 1426 | 9.7 |
| giant | S3 | page list | 0 | 10 | 0 | 402 | 437 | 437 | 9.0 |
| giant | S3 | page list | R | 10 | 0 | 440 | 509 | 509 | 7.7 |
| giant | S3 | page list | RN | 10 | 0 | 447 | 476 | 476 | 9.7 |
| giant | S4 | page not yet opened | 0 | 10 | 0 | 219 | 252 | 252 | 10.4 |
| giant | S4 | page not yet opened | R | 10 | 0 | 278 | 319 | 319 | 8.0 |
| giant | S4 | page not yet opened | RN | 10 | 0 | 292 | 318 | 318 | 8.0 |
| giant | S5 | selection to visible | 0 | 5 | 0 | 176 | 216 | 216 | 7.6 |
| giant | S5 | selection to visible | R | 5 | 0 | 255 | 321 | 321 | 4.3 |
| giant | S5 | selection to visible | RN | 5 | 0 | 278 | 336 | 336 | 6.9 |
| giant | S5r | selection to visible | 0 | 5 | 0 | 111 | 129 | 129 | 7.1 |
| giant | S5r | selection to visible | R | 5 | 0 | 145 | 149 | 149 | 4.3 |
| giant | S5r | selection to visible | RN | 5 | 0 | 234 | 258 | 258 | 6.6 |
| giant | S6 | component, first page | 0 | 5 | 0 | 798 | 877 | 877 | 6.8 |
| giant | S6 | component, first page | R | 5 | 0 | 144 | 149 | 149 | 4.3 |
| giant | S6 | component, first page | RN | 5 | 0 | 202 | 278 | 278 | 6.2 |
| giant | S6 | component, page opened later | 0 | 5 | 0 | 798 | 815 | 815 | 6.6 |
| giant | S6 | component, page opened later | R | 5 | 0 | 144 | 148 | 148 | 5.5 |
| giant | S6 | component, page opened later | RN | 5 | 0 | 214 | 218 | 218 | 6.0 |
| giant | S6 | shared module, first page | 0 | 5 | 0 | 820 | 825 | 825 | 6.7 |
| giant | S6 | shared module, first page | R | 5 | 0 | 145 | 150 | 150 | 5.5 |
| giant | S6 | shared module, first page | RN | 5 | 0 | 212 | 223 | 223 | 5.7 |
| giant | S7 | config edit | 0 | 5 | 0 | 12952 | 16699 | 16699 | 8.4 |
| giant | S7 | config edit | R | 5 | 0 | 4200 | 4592 | 4592 | 5.7 |
| giant | S7 | config edit | RN | 5 | 0 | 4252 | 4764 | 4764 | 5.4 |
| giant | S8 | added | 0 | 1 | 0 | 524 | 524 | 524 | 7.6 |
| giant | S8 | added | R | 1 | 0 | 432 | 432 | 432 | 4.7 |
| giant | S8 | added | RN | 1 | 0 | 476 | 476 | 476 | 4.0 |
| giant | S8 | renamed | 0 | 1 | 0 | 157 | 157 | 157 | 7.6 |
| giant | S8 | renamed | R | 1 | 0 | 138 | 138 | 138 | 4.7 |
| giant | S8 | renamed | RN | 1 | 0 | 140 | 140 | 140 | 4.0 |
| giant | S8 | retitled | 0 | 1 | 0 | 172 | 172 | 172 | 7.6 |
| giant | S8 | retitled | R | 1 | 0 | 137 | 137 | 137 | 4.7 |
| giant | S8 | retitled | RN | 1 | 0 | 140 | 140 | 140 | 4.0 |
| giant | S9-S4 | selection to visible | 0 | 1 | 0 | 132 | 132 | 132 | 6.7 |
| giant | S9-S4 | selection to visible | R | 1 | 0 | 524 | 524 | 524 | 5.8 |
| giant | S9-S4 | selection to visible | RN | 1 | 0 | 486 | 486 | 486 | 5.3 |
| giant | S9-S6 | component, first page | 0 | 5 | 0 | 1092 | 1173 | 1173 | 6.7 |
| giant | S9-S6 | component, first page | R | 5 | 0 | 380 | 457 | 457 | 5.8 |
| giant | S9-S6 | component, first page | RN | 5 | 0 | 427 | 692 | 692 | 5.3 |
| giant | own | own build | R | 10 | 0 | 243 | 255 | 255 | 7.7 |
| giant | own | own build | RN | 10 | 0 | 247 | 259 | 259 | 7.9 |
| colossal | S1 | first page | 0 | 7 | 3 | 115102 | 127281 | 127281 | 24.5 |
| colossal | S1 | first page | R | 10 | 0 | 7381 | 8592 | 8592 | 12.1 |
| colossal | S1 | first page | RN | 10 | 0 | 7387 | 7712 | 7712 | 13.3 |
| colossal | S1 | page list | 0 | 10 | 0 | 6580 | 9216 | 9216 | 24.5 |
| colossal | S1 | page list | R | 10 | 0 | 6231 | 7412 | 7412 | 12.1 |
| colossal | S1 | page list | RN | 10 | 0 | 6101 | 6516 | 6516 | 13.3 |
| colossal | S2 | first page | 0 | 6 | 4 | 107454 | 117341 | 117341 | 356.4 |
| colossal | S2 | first page | R | 10 | 0 | 4042 | 4239 | 4239 | 8.5 |
| colossal | S2 | first page | RN | 10 | 0 | 4051 | 46920 | 46920 | 7.7 |
| colossal | S2 | page list | 0 | 10 | 0 | 3022 | 8787 | 8787 | 356.4 |
| colossal | S2 | page list | R | 10 | 0 | 2858 | 3001 | 3001 | 8.5 |
| colossal | S2 | page list | RN | 10 | 0 | 2862 | 2927 | 2927 | 7.7 |
| colossal | S2i | first page | 0 | 4 | 6 | 98471 | 121235 | 121235 | 219.5 |
| colossal | S2i | first page | R | 10 | 0 | 4396 | 4570 | 4570 | 8.1 |
| colossal | S2i | first page | RN | 10 | 0 | 4417 | 4585 | 4585 | 12.2 |
| colossal | S2i | page list | 0 | 10 | 0 | 3463 | 9155 | 9155 | 219.5 |
| colossal | S2i | page list | R | 10 | 0 | 3243 | 3361 | 3361 | 8.1 |
| colossal | S2i | page list | RN | 10 | 0 | 3258 | 3353 | 3353 | 12.2 |
| colossal | S3 | first page | 0 | 7 | 3 | 113442 | 149073 | 149073 | 31.7 |
| colossal | S3 | first page | R | 10 | 0 | 4035 | 4140 | 4140 | 7.7 |
| colossal | S3 | first page | RN | 10 | 0 | 4053 | 4449 | 4449 | 8.9 |
| colossal | S3 | page list | 0 | 10 | 0 | 2988 | 3245 | 3245 | 31.7 |
| colossal | S3 | page list | R | 10 | 0 | 2876 | 2910 | 2910 | 7.7 |
| colossal | S3 | page list | RN | 10 | 0 | 2846 | 2907 | 2907 | 8.9 |
| colossal | S4 | page not yet opened | 0 | 9 | 1 | 955 | 63915 | 63915 | 50.8 |
| colossal | S4 | page not yet opened | R | 10 | 0 | 647 | 719 | 719 | 8.5 |
| colossal | S4 | page not yet opened | RN | 10 | 0 | 629 | 727 | 727 | 7.4 |
| colossal | S5 | selection to visible | 0 | 5 | 0 | 917 | 1040 | 1040 | 7.9 |
| colossal | S5 | selection to visible | R | 5 | 0 | 634 | 648 | 648 | 7.1 |
| colossal | S5 | selection to visible | RN | 5 | 0 | 638 | 671 | 671 | 7.5 |
| colossal | S5r | selection to visible | 0 | 5 | 0 | 542 | 1150 | 1150 | 8.8 |
| colossal | S5r | selection to visible | R | 5 | 0 | 536 | 600 | 600 | 6.6 |
| colossal | S5r | selection to visible | RN | 5 | 0 | 524 | 584 | 584 | 7.4 |
| colossal | S6 | component, first page | 0 | 0 | 1 | — | — | — | 5.2 |
| colossal | S6 | component, first page | R | 5 | 0 | 166 | 172 | 172 | 5.9 |
| colossal | S6 | component, first page | RN | 5 | 0 | 303 | 366 | 366 | 7.2 |
| colossal | S6 | component, page opened later | R | 5 | 0 | 159 | 166 | 166 | 5.8 |
| colossal | S6 | component, page opened later | RN | 5 | 0 | 359 | 389 | 389 | 7.8 |
| colossal | S6 | shared module, first page | R | 5 | 0 | 168 | 171 | 171 | 5.8 |
| colossal | S6 | shared module, first page | RN | 5 | 0 | 274 | 384 | 384 | 7.6 |
| colossal | S7 | config edit | R | 5 | 0 | 4305 | 4412 | 4412 | 7.4 |
| colossal | S7 | config edit | RN | 5 | 0 | 4228 | 5012 | 5012 | 9.0 |
| colossal | S8 | added | R | 1 | 0 | 2764 | 2764 | 2764 | 6.4 |
| colossal | S8 | added | RN | 1 | 0 | 3012 | 3012 | 3012 | 7.1 |
| colossal | S8 | renamed | R | 1 | 0 | 1324 | 1324 | 1324 | 6.4 |
| colossal | S8 | renamed | RN | 1 | 0 | 1347 | 1347 | 1347 | 7.1 |
| colossal | S8 | retitled | R | 1 | 0 | 1318 | 1318 | 1318 | 6.4 |
| colossal | S8 | retitled | RN | 1 | 0 | 1341 | 1341 | 1341 | 7.9 |
| colossal | S9-S4 | selection to visible | R | 1 | 0 | 680 | 680 | 680 | 6.3 |
| colossal | S9-S4 | selection to visible | RN | 1 | 0 | 725 | 725 | 725 | 7.2 |
| colossal | S9-S6 | component, first page | R | 5 | 0 | 394 | 398 | 398 | 7.0 |
| colossal | S9-S6 | component, first page | RN | 5 | 0 | 753 | 891 | 891 | 7.2 |
| colossal | own | own build | R | 10 | 0 | 242 | 728 | 728 | 7.7 |
| colossal | own | own build | RN | 10 | 0 | 244 | 307 | 307 | 7.9 |

## Checks

- S2i-retitled-listed 0 colossal: 13 pass, 0 fail
- S2i-retitled-listed 0 giant: 13 pass, 0 fail
- S2i-retitled-listed 0 large-system: 13 pass, 0 fail
- S2i-retitled-listed 0 small: 13 pass, 0 fail
- S2i-retitled-listed R colossal: 13 pass, 0 fail
- S2i-retitled-listed R giant: 12 pass, 0 fail
- S2i-retitled-listed R large-system: 13 pass, 0 fail
- S2i-retitled-listed R small: 13 pass, 0 fail
- S2i-retitled-listed RN colossal: 13 pass, 0 fail
- S2i-retitled-listed RN giant: 13 pass, 0 fail
- S2i-retitled-listed RN large-system: 13 pass, 0 fail
- S2i-retitled-listed RN small: 13 pass, 0 fail
- S8-added 0 giant: 1 pass, 0 fail
- S8-added 0 large-system: 1 pass, 0 fail
- S8-added R colossal: 1 pass, 0 fail
- S8-added R giant: 1 pass, 0 fail
- S8-added R large-system: 1 pass, 0 fail
- S8-added RN colossal: 1 pass, 0 fail
- S8-added RN giant: 1 pass, 0 fail
- S8-added RN large-system: 1 pass, 0 fail
- S8-renamed 0 giant: 1 pass, 0 fail
- S8-renamed 0 large-system: 1 pass, 0 fail
- S8-renamed R colossal: 1 pass, 0 fail
- S8-renamed R giant: 1 pass, 0 fail
- S8-renamed R large-system: 1 pass, 0 fail
- S8-renamed RN colossal: 1 pass, 0 fail
- S8-renamed RN giant: 1 pass, 0 fail
- S8-renamed RN large-system: 1 pass, 0 fail
- S8-retitled 0 giant: 1 pass, 0 fail
- S8-retitled 0 large-system: 1 pass, 0 fail
- S8-retitled R colossal: 1 pass, 0 fail
- S8-retitled R giant: 1 pass, 0 fail
- S8-retitled R large-system: 1 pass, 0 fail
- S8-retitled RN colossal: 1 pass, 0 fail
- S8-retitled RN giant: 1 pass, 0 fail
- S8-retitled RN large-system: 1 pass, 0 fail
- config-edit 0 giant: 5 pass, 0 fail
- config-edit 0 large-system: 5 pass, 0 fail
- config-edit R colossal: 5 pass, 0 fail
- config-edit R giant: 5 pass, 0 fail
- config-edit R large-system: 5 pass, 0 fail
- config-edit RN colossal: 5 pass, 0 fail
- config-edit RN giant: 5 pass, 0 fail
- config-edit RN large-system: 5 pass, 0 fail
- console 0 giant: 1 pass, 0 fail
- console 0 large-system: 1 pass, 0 fail
- console R colossal: 1 pass, 0 fail
- console R giant: 1 pass, 0 fail
- console R large-system: 1 pass, 0 fail
- console RN colossal: 1 pass, 0 fail
- console RN giant: 1 pass, 0 fail
- console RN large-system: 1 pass, 0 fail
- entry-parity R colossal: 1 pass, 0 fail
- entry-parity R formats: 1 pass, 0 fail
- entry-parity R giant: 1 pass, 0 fail
- entry-parity R large-system: 1 pass, 0 fail
- entry-parity R small: 1 pass, 0 fail
- error-contained 0 giant: 1 pass, 0 fail
- error-contained 0 large-system: 1 pass, 0 fail
- error-contained R colossal: 1 pass, 0 fail
- error-contained R giant: 1 pass, 0 fail
- error-contained R large-system: 1 pass, 0 fail
- error-contained RN colossal: 1 pass, 0 fail
- error-contained RN giant: 1 pass, 0 fail
- error-contained RN large-system: 1 pass, 0 fail
- error-shown 0 giant: 1 pass, 0 fail
- error-shown 0 large-system: 1 pass, 0 fail
- error-shown R colossal: 1 pass, 0 fail
- error-shown R giant: 1 pass, 0 fail
- error-shown R large-system: 1 pass, 0 fail
- error-shown RN colossal: 1 pass, 0 fail
- error-shown RN giant: 1 pass, 0 fail
- error-shown RN large-system: 1 pass, 0 fail
- hot-client-parity R colossal: 1 pass, 0 fail
- hot-client-parity R formats: 1 pass, 0 fail
- hot-client-parity R giant: 1 pass, 0 fail
- hot-client-parity R large-system: 1 pass, 0 fail
- hot-client-parity R small: 1 pass, 0 fail
- hot-update 0 colossal: 0 pass, 1 fail
- hot-update 0 giant: 20 pass, 0 fail
- hot-update 0 large-system: 20 pass, 0 fail
- hot-update R colossal: 20 pass, 0 fail
- hot-update R giant: 20 pass, 0 fail
- hot-update R large-system: 20 pass, 0 fail
- hot-update RN colossal: 20 pass, 0 fail
- hot-update RN giant: 20 pass, 0 fail
- hot-update RN large-system: 20 pass, 0 fail
- isolation 0 colossal: 10 pass, 0 fail
- isolation 0 giant: 11 pass, 0 fail
- isolation 0 large-system: 11 pass, 0 fail
- isolation R colossal: 11 pass, 0 fail
- isolation R giant: 11 pass, 0 fail
- isolation R large-system: 11 pass, 0 fail
- isolation RN colossal: 11 pass, 0 fail
- isolation RN giant: 11 pass, 0 fail
- isolation RN large-system: 11 pass, 0 fail
- no-moved-modules R colossal: 1 pass, 0 fail
- no-moved-modules R giant: 1 pass, 0 fail
- no-moved-modules R large-system: 1 pass, 0 fail
- no-moved-modules RN colossal: 1 pass, 0 fail
- no-moved-modules RN giant: 1 pass, 0 fail
- no-moved-modules RN large-system: 1 pass, 0 fail
- no-project-writes all colossal: 1 pass, 0 fail
- no-project-writes all formats: 1 pass, 0 fail
- no-project-writes all giant: 1 pass, 0 fail
- no-project-writes all large-system: 1 pass, 0 fail
- no-project-writes all small: 1 pass, 0 fail
- render-parity R formats: 0 pass, 1 fail
- render-parity R large-system: 1 pass, 0 fail
- render-parity RN formats: 0 pass, 1 fail
- render-parity RN large-system: 0 pass, 1 fail
- session 0 colossal: 0 pass, 1 fail

## Errors

- giant S2i first page R: Chromium timed out during Target.createTarget
- colossal S1 first page 0: Error: Preview request timed out
    at http://127.0.0.1:3579/_workbench/preview; Error: Preview request timed out
    at http://127.0.0.1:3580/_workbench/preview
- colossal S2 first page 0: Error: Preview request timed out
    at http://127.0.0.1:3579/_workbench/preview; Error: Preview request timed out
    at http://127.0.0.1:3580/_workbench/preview
- colossal S2i first page 0: Error: Preview request timed out
    at http://127.0.0.1:3579/_workbench/preview; Error: Preview request timed out
    at http://127.0.0.1:3580/_workbench/preview; Error: Preview request timed out
    at http://127.0.0.1:3581/_workbench/preview
- colossal S3 first page 0: Error: Preview request timed out
    at http://127.0.0.1:3581/_workbench/preview; Error: Preview request timed out
    at http://127.0.0.1:3579/_workbench/preview
- colossal S4 page not yet opened 0: not visible within 600000 ms

Samples started above the load threshold after waiting: 23. Browser launch retries: 0.
Load at start 37.7, at end 8.73.

