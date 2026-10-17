# Instant first page lab, stage 2, stage2-set-1

Samples per start cell: 10 (warmups 3); per session step: 5. Times in ms from the step's own start (first page and page list from the canvas's navigation), p50 / p95 / max by nearest rank. Missing counts samples with no time: timeouts and errors.

| Fixture | Sequence | Cell | Variant | n | missing | p50 | p95 | max | max load |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| small | S1 | first page | 0 | 10 | 0 | 1262 | 1333 | 1333 | 10.7 |
| small | S1 | first page | R | 10 | 0 | 667 | 819 | 819 | 8.0 |
| small | S1 | first page | RN | 10 | 0 | 650 | 680 | 680 | 8.8 |
| small | S1 | page list | 0 | 10 | 0 | 252 | 268 | 268 | 10.7 |
| small | S1 | page list | R | 10 | 0 | 235 | 262 | 262 | 8.0 |
| small | S1 | page list | RN | 10 | 0 | 247 | 287 | 287 | 8.8 |
| small | S2 | first page | 0 | 10 | 0 | 1220 | 1340 | 1340 | 10.5 |
| small | S2 | first page | R | 10 | 0 | 390 | 452 | 452 | 8.0 |
| small | S2 | first page | RN | 10 | 0 | 370 | 658 | 658 | 10.9 |
| small | S2 | page list | 0 | 10 | 0 | 211 | 284 | 284 | 10.5 |
| small | S2 | page list | R | 10 | 0 | 220 | 279 | 279 | 8.0 |
| small | S2 | page list | RN | 10 | 0 | 209 | 444 | 444 | 10.9 |
| small | S2i | first page | 0 | 10 | 0 | 1223 | 1285 | 1285 | 8.5 |
| small | S2i | first page | R | 10 | 0 | 366 | 447 | 447 | 8.0 |
| small | S2i | first page | RN | 10 | 0 | 371 | 432 | 432 | 8.1 |
| small | S2i | page list | 0 | 10 | 0 | 230 | 262 | 262 | 8.5 |
| small | S2i | page list | R | 10 | 0 | 213 | 278 | 278 | 8.0 |
| small | S2i | page list | RN | 10 | 0 | 214 | 233 | 233 | 8.1 |
| small | S3 | first page | 0 | 10 | 0 | 1273 | 1329 | 1329 | 8.2 |
| small | S3 | first page | R | 10 | 0 | 397 | 439 | 439 | 7.9 |
| small | S3 | first page | RN | 10 | 0 | 393 | 419 | 419 | 8.0 |
| small | S3 | page list | 0 | 10 | 0 | 227 | 238 | 238 | 8.2 |
| small | S3 | page list | R | 10 | 0 | 227 | 269 | 269 | 7.9 |
| small | S3 | page list | RN | 10 | 0 | 223 | 235 | 235 | 8.0 |
| small | S4 | page not yet opened | 0 | 10 | 0 | 63.8 | 69.6 | 69.6 | 10.5 |
| small | S4 | page not yet opened | R | 10 | 0 | 71.9 | 135 | 135 | 8.0 |
| small | S4 | page not yet opened | RN | 10 | 0 | 65.1 | 82.8 | 82.8 | 10.9 |
| small | own | own build | R | 10 | 0 | 64.8 | 96.5 | 96.5 | 7.9 |
| small | own | own build | RN | 10 | 0 | 63.3 | 65.7 | 65.7 | 7.9 |
| large-system | S1 | first page | 0 | 10 | 0 | 2736 | 2913 | 2913 | 9.2 |
| large-system | S1 | first page | R | 10 | 0 | 1699 | 1953 | 1953 | 11.4 |
| large-system | S1 | first page | RN | 10 | 0 | 1716 | 2005 | 2005 | 15.9 |
| large-system | S1 | page list | 0 | 10 | 0 | 250 | 347 | 347 | 9.2 |
| large-system | S1 | page list | R | 10 | 0 | 247 | 339 | 339 | 11.4 |
| large-system | S1 | page list | RN | 10 | 0 | 244 | 333 | 333 | 15.9 |
| large-system | S2 | first page | 0 | 10 | 0 | 3388 | 3655 | 3655 | 9.1 |
| large-system | S2 | first page | R | 10 | 0 | 1216 | 1423 | 1423 | 8.0 |
| large-system | S2 | first page | RN | 10 | 0 | 1227 | 1285 | 1285 | 8.2 |
| large-system | S2 | page list | 0 | 10 | 0 | 240 | 261 | 261 | 9.1 |
| large-system | S2 | page list | R | 10 | 0 | 228 | 259 | 259 | 8.0 |
| large-system | S2 | page list | RN | 10 | 0 | 239 | 261 | 261 | 8.2 |
| large-system | S2i | first page | 0 | 10 | 0 | 3219 | 3487 | 3487 | 9.4 |
| large-system | S2i | first page | R | 10 | 0 | 1166 | 1313 | 1313 | 9.1 |
| large-system | S2i | first page | RN | 10 | 0 | 1176 | 1237 | 1237 | 12.1 |
| large-system | S2i | page list | 0 | 10 | 0 | 233 | 303 | 303 | 9.4 |
| large-system | S2i | page list | R | 10 | 0 | 236 | 297 | 297 | 9.1 |
| large-system | S2i | page list | RN | 10 | 0 | 250 | 267 | 267 | 12.1 |
| large-system | S3 | first page | 0 | 10 | 0 | 3300 | 3526 | 3526 | 7.7 |
| large-system | S3 | first page | R | 10 | 0 | 1263 | 1900 | 1900 | 10.2 |
| large-system | S3 | first page | RN | 10 | 0 | 1237 | 1347 | 1347 | 8.0 |
| large-system | S3 | page list | 0 | 10 | 0 | 217 | 232 | 232 | 7.7 |
| large-system | S3 | page list | R | 10 | 0 | 260 | 314 | 314 | 10.2 |
| large-system | S3 | page list | RN | 10 | 0 | 251 | 270 | 270 | 8.0 |
| large-system | S4 | page not yet opened | 0 | 10 | 0 | 159 | 325 | 325 | 9.1 |
| large-system | S4 | page not yet opened | R | 10 | 0 | 272 | 361 | 361 | 8.3 |
| large-system | S4 | page not yet opened | RN | 10 | 0 | 258 | 313 | 313 | 8.2 |
| large-system | S5 | selection to visible | 0 | 5 | 0 | 140 | 157 | 157 | 7.0 |
| large-system | S5 | selection to visible | R | 5 | 0 | 231 | 235 | 235 | 7.5 |
| large-system | S5 | selection to visible | RN | 5 | 0 | 327 | 398 | 398 | 8.0 |
| large-system | S5r | selection to visible | 0 | 5 | 0 | 118 | 131 | 131 | 6.4 |
| large-system | S5r | selection to visible | R | 5 | 0 | 126 | 148 | 148 | 7.2 |
| large-system | S5r | selection to visible | RN | 5 | 0 | 188 | 235 | 235 | 7.5 |
| large-system | S6 | component, first page | 0 | 5 | 0 | 181 | 199 | 199 | 6.3 |
| large-system | S6 | component, first page | R | 5 | 0 | 131 | 133 | 133 | 7.2 |
| large-system | S6 | component, first page | RN | 5 | 0 | 230 | 233 | 233 | 7.2 |
| large-system | S6 | component, page opened later | 0 | 5 | 0 | 153 | 165 | 165 | 6.3 |
| large-system | S6 | component, page opened later | R | 5 | 0 | 143 | 155 | 155 | 7.2 |
| large-system | S6 | component, page opened later | RN | 5 | 0 | 233 | 233 | 233 | 6.9 |
| large-system | S6 | shared module, first page | 0 | 5 | 0 | 190 | 231 | 231 | 6.2 |
| large-system | S6 | shared module, first page | R | 5 | 0 | 148 | 150 | 150 | 7.1 |
| large-system | S6 | shared module, first page | RN | 5 | 0 | 248 | 299 | 299 | 6.6 |
| large-system | S7 | config edit | 0 | 5 | 0 | 2081 | 2106 | 2106 | 8.3 |
| large-system | S7 | config edit | R | 5 | 0 | 4295 | 4534 | 4534 | 7.6 |
| large-system | S7 | config edit | RN | 5 | 0 | 4187 | 4631 | 4631 | 6.3 |
| large-system | S8 | added | 0 | 1 | 0 | 358 | 358 | 358 | 7.8 |
| large-system | S8 | added | R | 1 | 0 | 239 | 239 | 239 | 7.4 |
| large-system | S8 | added | RN | 1 | 0 | 259 | 259 | 259 | 5.4 |
| large-system | S8 | renamed | 0 | 1 | 0 | 27.0 | 27.0 | 27.0 | 7.8 |
| large-system | S8 | renamed | R | 1 | 0 | 27.0 | 27.0 | 27.0 | 7.4 |
| large-system | S8 | renamed | RN | 1 | 0 | 27.0 | 27.0 | 27.0 | 5.4 |
| large-system | S8 | retitled | 0 | 1 | 0 | 28.0 | 28.0 | 28.0 | 7.8 |
| large-system | S8 | retitled | R | 1 | 0 | 26.0 | 26.0 | 26.0 | 7.4 |
| large-system | S8 | retitled | RN | 1 | 0 | 27.0 | 27.0 | 27.0 | 5.4 |
| large-system | S9-S4 | selection to visible | 0 | 1 | 0 | 146 | 146 | 146 | 7.7 |
| large-system | S9-S4 | selection to visible | R | 1 | 0 | 347 | 347 | 347 | 7.7 |
| large-system | S9-S4 | selection to visible | RN | 1 | 0 | 334 | 334 | 334 | 6.0 |
| large-system | S9-S6 | component, first page | 0 | 5 | 0 | 197 | 248 | 248 | 7.7 |
| large-system | S9-S6 | component, first page | R | 5 | 0 | 251 | 256 | 256 | 7.2 |
| large-system | S9-S6 | component, first page | RN | 5 | 0 | 248 | 428 | 428 | 6.3 |
| large-system | own | own build | R | 10 | 0 | 289 | 342 | 342 | 7.8 |
| large-system | own | own build | RN | 10 | 0 | 286 | 332 | 332 | 8.0 |
| giant | S1 | first page | 0 | 10 | 0 | 10714 | 11956 | 11956 | 11.5 |
| giant | S1 | first page | R | 10 | 0 | 1691 | 1833 | 1833 | 9.2 |
| giant | S1 | first page | RN | 10 | 0 | 1698 | 1868 | 1868 | 9.3 |
| giant | S1 | page list | 0 | 10 | 0 | 421 | 481 | 481 | 11.5 |
| giant | S1 | page list | R | 10 | 0 | 431 | 497 | 497 | 9.2 |
| giant | S1 | page list | RN | 10 | 0 | 428 | 452 | 452 | 9.3 |
| giant | S2 | first page | 0 | 10 | 0 | 16114 | 17757 | 17757 | 10.2 |
| giant | S2 | first page | R | 10 | 0 | 1316 | 1354 | 1354 | 7.9 |
| giant | S2 | first page | RN | 10 | 0 | 1304 | 1381 | 1381 | 7.8 |
| giant | S2 | page list | 0 | 10 | 0 | 408 | 427 | 427 | 10.2 |
| giant | S2 | page list | R | 10 | 0 | 416 | 428 | 428 | 7.9 |
| giant | S2 | page list | RN | 10 | 0 | 412 | 427 | 427 | 7.8 |
| giant | S2i | first page | 0 | 10 | 0 | 16225 | 17584 | 17584 | 9.5 |
| giant | S2i | first page | R | 10 | 0 | 1268 | 1352 | 1352 | 8.7 |
| giant | S2i | first page | RN | 10 | 0 | 1272 | 1356 | 1356 | 7.7 |
| giant | S2i | page list | 0 | 10 | 0 | 408 | 430 | 430 | 9.5 |
| giant | S2i | page list | R | 10 | 0 | 411 | 433 | 433 | 8.7 |
| giant | S2i | page list | RN | 10 | 0 | 415 | 424 | 424 | 7.7 |
| giant | S3 | first page | 0 | 10 | 0 | 16068 | 17894 | 17894 | 10.0 |
| giant | S3 | first page | R | 10 | 0 | 1322 | 1435 | 1435 | 8.4 |
| giant | S3 | first page | RN | 10 | 0 | 1347 | 1380 | 1380 | 8.3 |
| giant | S3 | page list | 0 | 10 | 0 | 393 | 406 | 406 | 10.0 |
| giant | S3 | page list | R | 10 | 0 | 445 | 556 | 556 | 8.4 |
| giant | S3 | page list | RN | 10 | 0 | 449 | 469 | 469 | 8.3 |
| giant | S4 | page not yet opened | 0 | 10 | 0 | 199 | 285 | 285 | 10.2 |
| giant | S4 | page not yet opened | R | 10 | 0 | 274 | 312 | 312 | 9.4 |
| giant | S4 | page not yet opened | RN | 10 | 0 | 283 | 313 | 313 | 7.7 |
| giant | S5 | selection to visible | 0 | 5 | 0 | 177 | 206 | 206 | 7.8 |
| giant | S5 | selection to visible | R | 5 | 0 | 252 | 330 | 330 | 6.7 |
| giant | S5 | selection to visible | RN | 5 | 0 | 281 | 348 | 348 | 6.7 |
| giant | S5r | selection to visible | 0 | 5 | 0 | 125 | 136 | 136 | 7.4 |
| giant | S5r | selection to visible | R | 5 | 0 | 129 | 134 | 134 | 6.0 |
| giant | S5r | selection to visible | RN | 5 | 0 | 243 | 250 | 250 | 6.5 |
| giant | S6 | component, first page | 0 | 5 | 0 | 813 | 857 | 857 | 7.3 |
| giant | S6 | component, first page | R | 5 | 0 | 145 | 147 | 147 | 5.8 |
| giant | S6 | component, first page | RN | 5 | 0 | 212 | 302 | 302 | 6.5 |
| giant | S6 | component, page opened later | 0 | 5 | 0 | 795 | 808 | 808 | 6.8 |
| giant | S6 | component, page opened later | R | 5 | 0 | 144 | 146 | 146 | 6.0 |
| giant | S6 | component, page opened later | RN | 5 | 0 | 211 | 231 | 231 | 6.3 |
| giant | S6 | shared module, first page | 0 | 5 | 0 | 811 | 843 | 843 | 6.3 |
| giant | S6 | shared module, first page | R | 5 | 0 | 147 | 155 | 155 | 6.0 |
| giant | S6 | shared module, first page | RN | 5 | 0 | 218 | 220 | 220 | 6.3 |
| giant | S7 | config edit | 0 | 4 | 1 | 12343 | 12563 | 12563 | 7.3 |
| giant | S7 | config edit | R | 5 | 0 | 4164 | 4332 | 4332 | 5.6 |
| giant | S7 | config edit | RN | 5 | 0 | 4211 | 4771 | 4771 | 6.8 |
| giant | S8 | added | 0 | 1 | 0 | 552 | 552 | 552 | 2.2 |
| giant | S8 | added | R | 1 | 0 | 427 | 427 | 427 | 4.2 |
| giant | S8 | added | RN | 1 | 0 | 515 | 515 | 515 | 6.9 |
| giant | S8 | renamed | 0 | 1 | 0 | 147 | 147 | 147 | 2.2 |
| giant | S8 | renamed | R | 1 | 0 | 186 | 186 | 186 | 4.2 |
| giant | S8 | renamed | RN | 1 | 0 | 144 | 144 | 144 | 6.9 |
| giant | S8 | retitled | 0 | 1 | 0 | 146 | 146 | 146 | 2.2 |
| giant | S8 | retitled | R | 1 | 0 | 142 | 142 | 142 | 4.2 |
| giant | S8 | retitled | RN | 1 | 0 | 140 | 140 | 140 | 6.9 |
| giant | S9-S4 | selection to visible | 0 | 1 | 0 | 181 | 181 | 181 | 2.8 |
| giant | S9-S4 | selection to visible | R | 1 | 0 | 475 | 475 | 475 | 5.0 |
| giant | S9-S4 | selection to visible | RN | 1 | 0 | 520 | 520 | 520 | 6.0 |
| giant | S9-S6 | component, first page | 0 | 5 | 0 | 1096 | 1193 | 1193 | 3.5 |
| giant | S9-S6 | component, first page | R | 5 | 0 | 433 | 444 | 444 | 5.0 |
| giant | S9-S6 | component, first page | RN | 5 | 0 | 415 | 799 | 799 | 6.5 |
| giant | own | own build | R | 10 | 0 | 245 | 262 | 262 | 7.9 |
| giant | own | own build | RN | 10 | 0 | 245 | 254 | 254 | 7.8 |
| colossal | S1 | first page | 0 | 10 | 0 | 93607 | 100984 | 100984 | 14.8 |
| colossal | S1 | first page | R | 10 | 0 | 7338 | 7368 | 7368 | 11.1 |
| colossal | S1 | first page | RN | 10 | 0 | 7542 | 8044 | 8044 | 11.2 |
| colossal | S1 | page list | 0 | 10 | 0 | 6015 | 6047 | 6047 | 14.8 |
| colossal | S1 | page list | R | 10 | 0 | 6177 | 6234 | 6234 | 11.1 |
| colossal | S1 | page list | RN | 10 | 0 | 6274 | 6619 | 6619 | 11.2 |
| colossal | S2 | first page | 0 | 10 | 0 | 93860 | 97585 | 97585 | 13.2 |
| colossal | S2 | first page | R | 10 | 0 | 4150 | 4294 | 4294 | 14.1 |
| colossal | S2 | first page | RN | 10 | 0 | 4109 | 4161 | 4161 | 8.3 |
| colossal | S2 | page list | 0 | 10 | 0 | 2831 | 2921 | 2921 | 13.2 |
| colossal | S2 | page list | R | 10 | 0 | 2917 | 2998 | 2998 | 14.1 |
| colossal | S2 | page list | RN | 10 | 0 | 2954 | 3042 | 3042 | 8.3 |
| colossal | S2i | first page | 0 | 10 | 0 | 91425 | 94747 | 94747 | 12.5 |
| colossal | S2i | first page | R | 10 | 0 | 4501 | 4571 | 4571 | 8.9 |
| colossal | S2i | first page | RN | 10 | 0 | 4374 | 4486 | 4486 | 9.7 |
| colossal | S2i | page list | 0 | 10 | 0 | 3233 | 3302 | 3302 | 12.5 |
| colossal | S2i | page list | R | 10 | 0 | 3314 | 3330 | 3330 | 8.9 |
| colossal | S2i | page list | RN | 10 | 0 | 3354 | 3467 | 3467 | 9.7 |
| colossal | S3 | first page | 0 | 10 | 0 | 92134 | 95370 | 95370 | 11.9 |
| colossal | S3 | first page | R | 10 | 0 | 4105 | 4197 | 4197 | 8.6 |
| colossal | S3 | first page | RN | 10 | 0 | 4079 | 4166 | 4166 | 10.0 |
| colossal | S3 | page list | 0 | 10 | 0 | 2802 | 2832 | 2832 | 11.9 |
| colossal | S3 | page list | R | 10 | 0 | 2910 | 2951 | 2951 | 8.6 |
| colossal | S3 | page list | RN | 10 | 0 | 2949 | 3038 | 3038 | 10.0 |
| colossal | S4 | page not yet opened | 0 | 10 | 0 | 867 | 1010 | 1010 | 13.3 |
| colossal | S4 | page not yet opened | R | 10 | 0 | 640 | 748 | 748 | 14.1 |
| colossal | S4 | page not yet opened | RN | 10 | 0 | 670 | 29908 | 29908 | 8.3 |
| colossal | S5 | selection to visible | 0 | 5 | 0 | 587 | 2274 | 2274 | 7.9 |
| colossal | S5 | selection to visible | R | 5 | 0 | 624 | 656 | 656 | 7.8 |
| colossal | S5 | selection to visible | RN | 5 | 0 | 638 | 679 | 679 | 7.5 |
| colossal | S5r | selection to visible | 0 | 5 | 0 | 548 | 799 | 799 | 7.5 |
| colossal | S5r | selection to visible | R | 5 | 0 | 536 | 606 | 606 | 6.9 |
| colossal | S5r | selection to visible | RN | 5 | 0 | 534 | 598 | 598 | 6.7 |
| colossal | S6 | component, first page | 0 | 5 | 0 | 24000 | 25562 | 25562 | 6.1 |
| colossal | S6 | component, first page | R | 5 | 0 | 163 | 164 | 164 | 6.6 |
| colossal | S6 | component, first page | RN | 5 | 0 | 352 | 401 | 401 | 6.6 |
| colossal | S6 | component, page opened later | 0 | 5 | 0 | 23460 | 24189 | 24189 | 6.6 |
| colossal | S6 | component, page opened later | R | 5 | 0 | 155 | 167 | 167 | 6.4 |
| colossal | S6 | component, page opened later | RN | 5 | 0 | 367 | 372 | 372 | 6.8 |
| colossal | S6 | shared module, first page | 0 | 5 | 0 | 24095 | 25353 | 25353 | 7.7 |
| colossal | S6 | shared module, first page | R | 5 | 0 | 167 | 171 | 171 | 6.2 |
| colossal | S6 | shared module, first page | RN | 5 | 0 | 352 | 383 | 383 | 6.8 |
| colossal | S7 | config edit | 0 | 5 | 0 | 95767 | 104683 | 104683 | 8.7 |
| colossal | S7 | config edit | R | 5 | 0 | 4221 | 4626 | 4626 | 6.0 |
| colossal | S7 | config edit | RN | 5 | 0 | 4452 | 4833 | 4833 | 6.5 |
| colossal | S8 | added | 0 | 1 | 0 | 3419 | 3419 | 3419 | 5.0 |
| colossal | S8 | added | R | 1 | 0 | 2798 | 2798 | 2798 | 4.8 |
| colossal | S8 | added | RN | 1 | 0 | 3218 | 3218 | 3218 | 5.9 |
| colossal | S8 | renamed | 0 | 1 | 0 | 1334 | 1334 | 1334 | 5.0 |
| colossal | S8 | renamed | R | 1 | 0 | 1365 | 1365 | 1365 | 4.8 |
| colossal | S8 | renamed | RN | 1 | 0 | 1504 | 1504 | 1504 | 5.9 |
| colossal | S8 | retitled | 0 | 1 | 0 | 1322 | 1322 | 1322 | 5.0 |
| colossal | S8 | retitled | R | 1 | 0 | 1299 | 1299 | 1299 | 4.5 |
| colossal | S8 | retitled | RN | 1 | 0 | 1330 | 1330 | 1330 | 7.3 |
| colossal | S9-S4 | selection to visible | 0 | 1 | 0 | 645 | 645 | 645 | 4.6 |
| colossal | S9-S4 | selection to visible | R | 1 | 0 | 692 | 692 | 692 | 5.8 |
| colossal | S9-S4 | selection to visible | RN | 1 | 0 | 1083 | 1083 | 1083 | 7.7 |
| colossal | S9-S6 | component, first page | 0 | 5 | 0 | 24333 | 26548 | 26548 | 7.5 |
| colossal | S9-S6 | component, first page | R | 5 | 0 | 386 | 405 | 405 | 5.8 |
| colossal | S9-S6 | component, first page | RN | 5 | 0 | 1485 | 1532 | 1532 | 8.0 |
| colossal | own | own build | R | 10 | 0 | 242 | 255 | 255 | 7.6 |
| colossal | own | own build | RN | 10 | 0 | 253 | 262 | 262 | 7.7 |

## Checks

- S2i-retitled-listed 0 colossal: 13 pass, 0 fail
- S2i-retitled-listed 0 giant: 13 pass, 0 fail
- S2i-retitled-listed 0 large-system: 13 pass, 0 fail
- S2i-retitled-listed 0 small: 13 pass, 0 fail
- S2i-retitled-listed R colossal: 13 pass, 0 fail
- S2i-retitled-listed R giant: 13 pass, 0 fail
- S2i-retitled-listed R large-system: 13 pass, 0 fail
- S2i-retitled-listed R small: 13 pass, 0 fail
- S2i-retitled-listed RN colossal: 13 pass, 0 fail
- S2i-retitled-listed RN giant: 13 pass, 0 fail
- S2i-retitled-listed RN large-system: 13 pass, 0 fail
- S2i-retitled-listed RN small: 13 pass, 0 fail
- S8-added 0 colossal: 1 pass, 0 fail
- S8-added 0 giant: 1 pass, 0 fail
- S8-added 0 large-system: 1 pass, 0 fail
- S8-added R colossal: 1 pass, 0 fail
- S8-added R giant: 1 pass, 0 fail
- S8-added R large-system: 1 pass, 0 fail
- S8-added RN colossal: 1 pass, 0 fail
- S8-added RN giant: 1 pass, 0 fail
- S8-added RN large-system: 1 pass, 0 fail
- S8-renamed 0 colossal: 1 pass, 0 fail
- S8-renamed 0 giant: 1 pass, 0 fail
- S8-renamed 0 large-system: 1 pass, 0 fail
- S8-renamed R colossal: 1 pass, 0 fail
- S8-renamed R giant: 1 pass, 0 fail
- S8-renamed R large-system: 1 pass, 0 fail
- S8-renamed RN colossal: 1 pass, 0 fail
- S8-renamed RN giant: 1 pass, 0 fail
- S8-renamed RN large-system: 1 pass, 0 fail
- S8-retitled 0 colossal: 1 pass, 0 fail
- S8-retitled 0 giant: 1 pass, 0 fail
- S8-retitled 0 large-system: 1 pass, 0 fail
- S8-retitled R colossal: 1 pass, 0 fail
- S8-retitled R giant: 1 pass, 0 fail
- S8-retitled R large-system: 1 pass, 0 fail
- S8-retitled RN colossal: 1 pass, 0 fail
- S8-retitled RN giant: 1 pass, 0 fail
- S8-retitled RN large-system: 1 pass, 0 fail
- config-edit 0 colossal: 5 pass, 0 fail
- config-edit 0 giant: 4 pass, 1 fail
- config-edit 0 large-system: 5 pass, 0 fail
- config-edit R colossal: 5 pass, 0 fail
- config-edit R giant: 5 pass, 0 fail
- config-edit R large-system: 5 pass, 0 fail
- config-edit RN colossal: 5 pass, 0 fail
- config-edit RN giant: 5 pass, 0 fail
- config-edit RN large-system: 5 pass, 0 fail
- console 0 colossal: 1 pass, 0 fail
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
- error-contained 0 colossal: 1 pass, 0 fail
- error-contained 0 giant: 1 pass, 0 fail
- error-contained 0 large-system: 1 pass, 0 fail
- error-contained R colossal: 1 pass, 0 fail
- error-contained R giant: 1 pass, 0 fail
- error-contained R large-system: 1 pass, 0 fail
- error-contained RN colossal: 1 pass, 0 fail
- error-contained RN giant: 1 pass, 0 fail
- error-contained RN large-system: 1 pass, 0 fail
- error-shown 0 colossal: 1 pass, 0 fail
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
- hot-update 0 colossal: 20 pass, 0 fail
- hot-update 0 giant: 20 pass, 0 fail
- hot-update 0 large-system: 20 pass, 0 fail
- hot-update R colossal: 20 pass, 0 fail
- hot-update R giant: 20 pass, 0 fail
- hot-update R large-system: 20 pass, 0 fail
- hot-update RN colossal: 20 pass, 0 fail
- hot-update RN giant: 20 pass, 0 fail
- hot-update RN large-system: 20 pass, 0 fail
- isolation 0 colossal: 11 pass, 0 fail
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
- render-parity RN large-system: 1 pass, 0 fail

Samples started above the load threshold after waiting: 0. Browser launch retries: 0.
Load at start 7.74, at end 2.05.

