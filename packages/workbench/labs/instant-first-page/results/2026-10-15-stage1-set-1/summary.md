# Instant first page lab, stage 1, stage1-set-1

Samples per cell: 10, warmups: 3. Times in ms, p50 / p95 / max by nearest rank; wall is from spawning the process.

| Group | Fixture | Cell | Mode | Held | Host | n | p50 | p95 | max | wall p50 | max load | missing |
| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| builds | small | B1 |  |  | node | 10 | 82.0 | 87.4 | 87.4 | 247 | 8.0 | 0 |
| builds | small | B1 |  |  | rust | 10 | 62.5 | 70.9 | 70.9 | 84.1 | 8.0 | 0 |
| builds | small | B2 |  |  | node | 10 | 47.1 | 93.7 | 93.7 | 216 | 9.1 | 0 |
| builds | small | B2 |  |  | rust | 10 | 19.5 | 43.2 | 43.2 | 36.8 | 8.0 | 0 |
| builds | large-system | B1 |  |  | node | 10 | 312 | 392 | 392 | 517 | 8.1 | 0 |
| builds | large-system | B1 |  |  | rust | 10 | 244 | 253 | 253 | 262 | 7.8 | 0 |
| builds | large-system | B2 |  |  | node | 10 | 432 | 447 | 447 | 601 | 8.6 | 0 |
| builds | large-system | B2 |  |  | rust | 10 | 195 | 221 | 221 | 213 | 7.8 | 0 |
| builds | large-system | B3 |  | 1 | node | 10 | 118 | 157 | 157 | — | 7.7 | 0 |
| builds | large-system | B3 |  | 1 | rust | 10 | 74.5 | 85.4 | 85.4 | — | 7.8 | 0 |
| builds | large-system | B3 |  | 20 | node | 10 | 231 | 267 | 267 | — | 7.8 | 0 |
| builds | large-system | B3 |  | 20 | rust | 10 | 120 | 132 | 132 | — | 7.0 | 0 |
| builds | large-system | B4-component |  | 1 | node | 10 | 121 | 130 | 130 | — | 7.7 | 0 |
| builds | large-system | B4-component |  | 1 | rust | 10 | 68.2 | 73.6 | 73.6 | — | 7.8 | 0 |
| builds | large-system | B4-component |  | 20 | node | 10 | 222 | 229 | 229 | — | 7.5 | 0 |
| builds | large-system | B4-component |  | 20 | rust | 10 | 114 | 118 | 118 | — | 7.0 | 0 |
| builds | large-system | B4-shared |  | 1 | node | 10 | 127 | 138 | 138 | — | 7.8 | 0 |
| builds | large-system | B4-shared |  | 1 | rust | 10 | 68.8 | 71.1 | 71.1 | — | 7.5 | 0 |
| builds | large-system | B4-shared |  | 20 | node | 10 | 255 | 366 | 366 | — | 7.0 | 0 |
| builds | large-system | B4-shared |  | 20 | rust | 10 | 122 | 166 | 166 | — | 6.6 | 0 |
| builds | giant | B1 |  |  | node | 10 | 319 | 426 | 426 | 521 | 9.2 | 0 |
| builds | giant | B1 |  |  | rust | 10 | 243 | 262 | 262 | 262 | 8.0 | 0 |
| builds | giant | B2 |  |  | node | 10 | 432 | 440 | 440 | 601 | 8.7 | 0 |
| builds | giant | B2 |  |  | rust | 10 | 192 | 205 | 205 | 210 | 8.6 | 0 |
| builds | giant | B3 |  | 1 | node | 10 | 141 | 203 | 203 | — | 7.4 | 0 |
| builds | giant | B3 |  | 1 | rust | 10 | 80.8 | 93.4 | 93.4 | — | 6.9 | 0 |
| builds | giant | B3 |  | 20 | node | 10 | 323 | 364 | 364 | — | 7.4 | 0 |
| builds | giant | B3 |  | 20 | rust | 10 | 127 | 141 | 141 | — | 7.9 | 0 |
| builds | giant | B3 |  | 100 | node | 10 | 909 | 955 | 955 | — | 7.4 | 0 |
| builds | giant | B3 |  | 100 | rust | 10 | 322 | 342 | 342 | — | 7.6 | 0 |
| builds | giant | B4-component |  | 1 | node | 10 | 176 | 199 | 199 | — | 7.4 | 0 |
| builds | giant | B4-component |  | 1 | rust | 10 | 77.6 | 79.4 | 79.4 | — | 7.9 | 0 |
| builds | giant | B4-component |  | 20 | node | 10 | 329 | 351 | 351 | — | 7.1 | 0 |
| builds | giant | B4-component |  | 20 | rust | 10 | 127 | 131 | 131 | — | 7.6 | 0 |
| builds | giant | B4-component |  | 100 | node | 10 | 1074 | 1144 | 1144 | — | 7.7 | 0 |
| builds | giant | B4-component |  | 100 | rust | 10 | 317 | 341 | 341 | — | 7.2 | 0 |
| builds | giant | B4-shared |  | 1 | node | 10 | 180 | 209 | 209 | — | 7.2 | 0 |
| builds | giant | B4-shared |  | 1 | rust | 10 | 76.8 | 85.4 | 85.4 | — | 7.9 | 0 |
| builds | giant | B4-shared |  | 20 | node | 10 | 333 | 340 | 340 | — | 6.6 | 0 |
| builds | giant | B4-shared |  | 20 | rust | 10 | 127 | 133 | 133 | — | 7.6 | 0 |
| builds | giant | B4-shared |  | 100 | node | 10 | 897 | 991 | 991 | — | 8.0 | 0 |
| builds | giant | B4-shared |  | 100 | rust | 10 | 304 | 321 | 321 | — | 7.6 | 0 |
| builds | colossal | B1 |  |  | node | 10 | 315 | 325 | 325 | 516 | 8.5 | 0 |
| builds | colossal | B1 |  |  | rust | 10 | 241 | 250 | 250 | 259 | 9.0 | 0 |
| builds | colossal | B2 |  |  | node | 10 | 432 | 449 | 449 | 602 | 8.9 | 0 |
| builds | colossal | B2 |  |  | rust | 10 | 188 | 197 | 197 | 207 | 8.8 | 0 |
| builds | colossal | B3 |  | 1 | node | 10 | 96.2 | 137 | 137 | — | 7.6 | 0 |
| builds | colossal | B3 |  | 1 | rust | 10 | 63.1 | 76.3 | 76.3 | — | 7.7 | 0 |
| builds | colossal | B3 |  | 20 | node | 10 | 218 | 252 | 252 | — | 7.2 | 0 |
| builds | colossal | B3 |  | 20 | rust | 10 | 100 | 115 | 115 | — | 7.2 | 0 |
| builds | colossal | B3 |  | 100 | node | 10 | 717 | 763 | 763 | — | 6.0 | 0 |
| builds | colossal | B3 |  | 100 | rust | 10 | 234 | 242 | 242 | — | 6.1 | 0 |
| builds | colossal | B4-component |  | 1 | node | 10 | 116 | 120 | 120 | — | 7.6 | 0 |
| builds | colossal | B4-component |  | 1 | rust | 10 | 63.9 | 68.3 | 68.3 | — | 7.7 | 0 |
| builds | colossal | B4-component |  | 20 | node | 10 | 222 | 235 | 235 | — | 6.9 | 0 |
| builds | colossal | B4-component |  | 20 | rust | 10 | 101 | 104 | 104 | — | 7.2 | 0 |
| builds | colossal | B4-component |  | 100 | node | 10 | 678 | 753 | 753 | — | 5.6 | 0 |
| builds | colossal | B4-component |  | 100 | rust | 10 | 238 | 251 | 251 | — | 6.2 | 0 |
| builds | colossal | B4-shared |  | 1 | node | 10 | 120 | 168 | 168 | — | 7.2 | 0 |
| builds | colossal | B4-shared |  | 1 | rust | 10 | 62.7 | 67.8 | 67.8 | — | 7.7 | 0 |
| builds | colossal | B4-shared |  | 20 | node | 10 | 221 | 255 | 255 | — | 6.7 | 0 |
| builds | colossal | B4-shared |  | 20 | rust | 10 | 100 | 102 | 102 | — | 7.2 | 0 |
| builds | colossal | B4-shared |  | 100 | node | 10 | 682 | 710 | 710 | — | 5.0 | 0 |
| builds | colossal | B4-shared |  | 100 | rust | 10 | 238 | 248 | 248 | — | 5.8 | 0 |
| index | small | I1 |  |  | rust | 10 | 2.5 | 2.7 | 2.7 | 14.6 | 8.0 | 0 |
| index | small | I1 |  |  | shipping | 10 | 161 | 180 | 180 | 194 | 8.0 | 0 |
| index | small | I2 | dirs |  | rust | 10 | 1.6 | 1.9 | 1.9 | 14.0 | 8.0 | 0 |
| index | small | I2 | walk |  | rust | 10 | 2.5 | 2.9 | 2.9 | 15.3 | 8.0 | 0 |
| index | small | I3 | dirs |  | rust | 10 | 2.3 | 2.5 | 2.5 | 14.4 | 8.0 | 0 |
| index | large-system | I1 |  |  | rust | 10 | 6.1 | 6.8 | 6.8 | 18.9 | 8.0 | 0 |
| index | large-system | I1 |  |  | shipping | 10 | 229 | 234 | 234 | 261 | 8.0 | 0 |
| index | large-system | I2 | dirs |  | rust | 10 | 2.6 | 2.8 | 2.8 | 15.1 | 8.0 | 0 |
| index | large-system | I2 | walk |  | rust | 10 | 5.1 | 5.7 | 5.7 | 17.2 | 8.0 | 0 |
| index | large-system | I3 | dirs |  | rust | 10 | 3.9 | 4.2 | 4.2 | 16.2 | 8.0 | 0 |
| index | giant | I1 |  |  | rust | 10 | 38.4 | 39.0 | 39.0 | 50.5 | 7.8 | 0 |
| index | giant | I1 |  |  | shipping | 10 | 913 | 959 | 959 | 964 | 7.8 | 0 |
| index | giant | I2 | dirs |  | rust | 10 | 8.5 | 9.0 | 9.0 | 20.9 | 7.1 | 0 |
| index | giant | I2 | walk |  | rust | 10 | 29.9 | 31.8 | 31.8 | 42.7 | 7.8 | 0 |
| index | giant | I3 | dirs |  | rust | 10 | 13.9 | 14.4 | 14.4 | 26.5 | 7.8 | 0 |
| index | colossal | I1 |  |  | rust | 10 | 1067 | 1110 | 1110 | 1086 | 10.1 | 0 |
| index | colossal | I1 |  |  | shipping | 10 | 12859 | 20656 | 20656 | 14425 | 8.6 | 0 |
| index | colossal | I2 | dirs |  | rust | 10 | 108 | 123 | 123 | 126 | 7.1 | 0 |
| index | colossal | I2 | walk |  | rust | 10 | 364 | 379 | 379 | 386 | 6.8 | 0 |
| index | colossal | I3 | dirs |  | rust | 10 | 214 | 263 | 263 | 239 | 6.5 | 0 |

## Checks

- B3-no-moved-modules node colossal: 39 pass, 0 fail
- B3-no-moved-modules node giant: 39 pass, 0 fail
- B3-no-moved-modules node large-system: 26 pass, 0 fail
- B3-no-moved-modules rust colossal: 39 pass, 0 fail
- B3-no-moved-modules rust giant: 39 pass, 0 fail
- B3-no-moved-modules rust large-system: 26 pass, 0 fail
- C1 all: 1 pass, 0 fail
- C2 node large-system: 1 pass, 0 fail
- C2 rust large-system: 1 pass, 0 fail
- C2-format node formats astro/card: 0 pass, 1 fail
- C2-format node formats canvas/badge: 1 pass, 0 fail
- C2-format node formats html/landing: 0 pass, 1 fail
- C2-format node formats native/counter: 0 pass, 1 fail
- C2-format node formats pages/page-a: 0 pass, 1 fail
- C2-format node formats pages/page-b: 0 pass, 1 fail
- C2-format node formats vue/counter: 0 pass, 1 fail
- C2-format rust formats astro/card: 0 pass, 1 fail
- C2-format rust formats canvas/badge: 1 pass, 0 fail
- C2-format rust formats html/landing: 0 pass, 1 fail
- C2-format rust formats native/counter: 0 pass, 1 fail
- C2-format rust formats pages/page-a: 0 pass, 1 fail
- C2-format rust formats pages/page-b: 0 pass, 1 fail
- C2-format rust formats vue/counter: 0 pass, 1 fail
- C3 node large-system: 1 pass, 0 fail
- C3 rust large-system: 1 pass, 0 fail
- C4 node large-system: 1 pass, 0 fail
- C4 rust large-system: 1 pass, 0 fail
- C5 node large-system: 1 pass, 0 fail
- C5 rust large-system: 1 pass, 0 fail
- C6 node large-system: 1 pass, 0 fail
- C6 rust large-system: 1 pass, 0 fail
- C7 node large-system: 1 pass, 0 fail
- C7 rust large-system: 1 pass, 0 fail
- C8 rust large-system: 1 pass, 0 fail
- index-matches colossal: 52 pass, 0 fail
- index-matches giant: 52 pass, 0 fail
- index-matches large-system: 52 pass, 0 fail
- index-matches small: 52 pass, 0 fail
- index-retitled colossal: 13 pass, 0 fail
- index-retitled giant: 13 pass, 0 fail
- index-retitled large-system: 13 pass, 0 fail
- index-retitled small: 13 pass, 0 fail

Samples started above the load threshold after waiting: 0.
Load at start 9.98, at end 5.4.
