# Performance observations

Planned n=20; inspect actual n per cell. Incomplete sets and correctness failures cannot establish a performance verdict; nearest-rank tails remain exploratory.

Nearest rank, rounded for display; JSON/CSV retains full precision. Values are kept separate by fixture, cache condition and run set.

## first.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 204.6 | 328.9 | 800.5 |
| 0 | Small | cold-startup | 20 | 208.3 | 719.2 | 854.5 |
| 2 | Small | cold-startup | 19 | 224.9 | 384 | 384 |
| D | Small | cold-startup | 20 | 209.4 | 327.7 | 1108.8 |
| D | Small | fresh-profile-after-examples-settled | 20 | 205.5 | 321.7 | 388.9 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 210.2 | 346.5 | 438.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 197.8 | 423 | 797.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 203.9 | 254.4 | 431.4 |
| 0 | Overview | cold-startup | 20 | 219.2 | 718.7 | 951.1 |
| 2 | Overview | cold-startup | 20 | 226.3 | 456.4 | 1353.6 |
| 1 | Overview | cold-startup | 20 | 212.6 | 590.8 | 1709 |
| D | Overview | cold-startup | 20 | 216.4 | 1232.7 | 1736.6 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 223.4 | 958.6 | 1525.8 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 220.9 | 653.7 | 1407.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 207 | 303.1 | 1250 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 210.9 | 256.6 | 744.1 |
| 2 | Code-heavy | cold-startup | 20 | 268.6 | 423.8 | 524.3 |
| 1 | Code-heavy | cold-startup | 20 | 255.7 | 583.4 | 941.8 |
| 0 | Code-heavy | cold-startup | 20 | 261.6 | 372.8 | 487.8 |
| D | Code-heavy | cold-startup | 20 | 264.4 | 398.7 | 1105.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 244.8 | 299.8 | 396.8 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 261.5 | 742.6 | 887.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 263.2 | 435.3 | 503.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 263.7 | 323.1 | 328.2 |

## first.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 87 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 129 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 106 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 70 | 71 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 156 | 239 | 244 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 128 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 181 | 213 | 930 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 126 | 142 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## first.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 59.1 | 185.4 | 657 |
| 0 | Small | cold-startup | 20 | 62.2 | 569.6 | 707.7 |
| 2 | Small | cold-startup | 19 | 76.8 | 234.4 | 234.4 |
| D | Small | cold-startup | 20 | 57.9 | 161.5 | 963.1 |
| D | Small | fresh-profile-after-examples-settled | 20 | 54 | 179.6 | 241.2 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 58.6 | 192 | 290.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 50.4 | 276.5 | 647.8 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 54.8 | 108.5 | 289.5 |
| 0 | Overview | cold-startup | 20 | 62.9 | 523.1 | 790.2 |
| 2 | Overview | cold-startup | 20 | 69 | 290.6 | 1211.5 |
| 1 | Overview | cold-startup | 20 | 58.1 | 443.6 | 1558 |
| D | Overview | cold-startup | 20 | 64.1 | 1088.8 | 1593.3 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 74.7 | 820 | 1380.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 66.9 | 469.7 | 1267.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 49.2 | 150 | 1082.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 57.9 | 110.9 | 597.3 |
| 2 | Code-heavy | cold-startup | 20 | 105.8 | 279.7 | 387.5 |
| 1 | Code-heavy | cold-startup | 20 | 93.6 | 416.3 | 798.2 |
| 0 | Code-heavy | cold-startup | 20 | 104.3 | 234.2 | 326.5 |
| D | Code-heavy | cold-startup | 20 | 107.3 | 253.9 | 959.6 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 90.8 | 148.4 | 186.3 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 93.2 | 604 | 745.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 110.5 | 287.6 | 299.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 98.7 | 177.2 | 177.6 |

## first.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 2.8 | 4.1 | 4.5 |
| 0 | Small | cold-startup | 20 | 2.7 | 3.1 | 3.7 |
| 2 | Small | cold-startup | 19 | 2.6 | 7.9 | 7.9 |
| D | Small | cold-startup | 20 | 2.8 | 3.9 | 6 |
| D | Small | fresh-profile-after-examples-settled | 20 | 2.3 | 3.5 | 3.7 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 2.4 | 3.1 | 3.8 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 2.5 | 4.5 | 10.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 2.4 | 3.7 | 4 |
| 0 | Overview | cold-startup | 20 | 3.1 | 4.1 | 5.4 |
| 2 | Overview | cold-startup | 20 | 2.8 | 4.1 | 5.1 |
| 1 | Overview | cold-startup | 20 | 2.9 | 3.9 | 8.7 |
| D | Overview | cold-startup | 20 | 2.8 | 3.8 | 4.4 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 2.9 | 4.1 | 4.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 2.8 | 3.3 | 3.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 2.6 | 3.9 | 3.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 2.5 | 4 | 4 |
| 2 | Code-heavy | cold-startup | 20 | 5.1 | 6.7 | 7 |
| 1 | Code-heavy | cold-startup | 20 | 4.8 | 6.3 | 8.7 |
| 0 | Code-heavy | cold-startup | 20 | 5.3 | 7.4 | 7.8 |
| D | Code-heavy | cold-startup | 20 | 5.1 | 8.6 | 13.5 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.9 | 6.7 | 7.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 5.1 | 6.5 | 10.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 5 | 6.3 | 6.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.4 | 6.3 | 8.8 |

## first.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 216165 | 216167 | 216167 |
| 0 | Small | cold-startup | 20 | 215772 | 215774 | 215774 |
| 2 | Small | cold-startup | 19 | 216167 | 216167 | 216167 |
| D | Small | cold-startup | 20 | 212493 | 212495 | 212495 |
| D | Small | fresh-profile-after-examples-settled | 20 | 212493 | 212495 | 212495 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 216165 | 216167 | 216167 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 216165 | 216167 | 216167 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 215772 | 215774 | 215774 |
| 0 | Overview | cold-startup | 20 | 252475 | 252477 | 252477 |
| 2 | Overview | cold-startup | 20 | 1137623 | 1137625 | 1137625 |
| 1 | Overview | cold-startup | 20 | 336917 | 336919 | 336919 |
| D | Overview | cold-startup | 20 | 247313 | 247315 | 247315 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 247313 | 247315 | 247315 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1137623 | 1137625 | 1137625 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 252475 | 252477 | 252477 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 336917 | 336919 | 336919 |
| 2 | Code-heavy | cold-startup | 20 | 4690142 | 4690144 | 4690144 |
| 1 | Code-heavy | cold-startup | 20 | 344868 | 344870 | 344870 |
| 0 | Code-heavy | cold-startup | 20 | 760221 | 760223 | 760223 |
| D | Code-heavy | cold-startup | 20 | 255264 | 255266 | 255266 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 255264 | 255266 | 255266 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4690142 | 4690144 | 4690144 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 760221 | 760223 | 760223 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 344868 | 344870 | 344870 |

## first.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 211065 | 211067 | 211067 |
| 0 | Small | cold-startup | 20 | 210672 | 210674 | 210674 |
| 2 | Small | cold-startup | 19 | 211067 | 211067 | 211067 |
| D | Small | cold-startup | 20 | 207393 | 207395 | 207395 |
| D | Small | fresh-profile-after-examples-settled | 20 | 207393 | 207395 | 207395 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 211065 | 211067 | 211067 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 211065 | 211067 | 211067 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 210672 | 210674 | 210674 |
| 0 | Overview | cold-startup | 20 | 247675 | 247677 | 247677 |
| 2 | Overview | cold-startup | 20 | 1130123 | 1130125 | 1130125 |
| 1 | Overview | cold-startup | 20 | 328517 | 328519 | 328519 |
| D | Overview | cold-startup | 20 | 242813 | 242815 | 242815 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 242813 | 242815 | 242815 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1130123 | 1130125 | 1130125 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 247675 | 247677 | 247677 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 328517 | 328519 | 328519 |
| 2 | Code-heavy | cold-startup | 20 | 4662842 | 4662844 | 4662844 |
| 1 | Code-heavy | cold-startup | 20 | 335868 | 335870 | 335870 |
| 0 | Code-heavy | cold-startup | 20 | 754821 | 754823 | 754823 |
| D | Code-heavy | cold-startup | 20 | 250164 | 250166 | 250166 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 250164 | 250166 | 250166 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4662842 | 4662844 | 4662844 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754821 | 754823 | 754823 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335868 | 335870 | 335870 |

## first.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Overview | cold-startup | 20 | 0.2 | 0.2 | 0.3 |
| 1 | Overview | cold-startup | 20 | 0.2 | 0.3 | 0.3 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.2 | 0.2 | 0.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.2 | 0.2 | 0.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Code-heavy | cold-startup | 20 | 19 | 19.7 | 27 |
| 1 | Code-heavy | cold-startup | 20 | 15.199 | 15.5 | 15.6 |
| 0 | Code-heavy | cold-startup | 20 | 13 | 13.7 | 14 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 19 | 21.1 | 21.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 13.1 | 13.4 | 13.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 15.3 | 15.901 | 18.3 |

## first.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Code-heavy | cold-startup | 20 | 7.1 | 7.9 | 8.9 |
| 1 | Code-heavy | cold-startup | 20 | 4.4 | 4.7 | 5 |
| 0 | Code-heavy | cold-startup | 20 | 4.8 | 5.1 | 5.4 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 7 | 7.6 | 10.3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.7 | 5.2 | 5.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.3 | 4.9 | 5.9 |

## first.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 87 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 129 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 106 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 70 | 71 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 100 | 239 | 244 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 128 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 181 | 213 | 930 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 74 | 76 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## first.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 21.3 | 27.4 | 28.2 |
| 1 | Overview | cold-startup | 20 | 26.9 | 33.9 | 52.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 20.9 | 23.1 | 24.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 26.8 | 31.7 | 34.9 |
| 2 | Code-heavy | cold-startup | 20 | 20.5 | 23.8 | 32.9 |
| 1 | Code-heavy | cold-startup | 20 | 23.8 | 37.1 | 44.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 19.8 | 25 | 25.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 23.9 | 32.9 | 35.1 |

## first.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 9.2 | 11.8 | 12.7 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 9 | 11.7 | 13.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 87.5 | 228.1 | 278.4 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 87.8 | 166 | 357.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## first.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 45.3 | 63.4 | 63.6 |
| 1 | Overview | cold-startup | 20 | 7.3 | 8.2 | 8.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 46 | 79.6 | 91.8 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7.3 | 8.2 | 8.3 |
| 2 | Code-heavy | cold-startup | 20 | 860 | 1495.6 | 3042.5 |
| 1 | Code-heavy | cold-startup | 20 | 93 | 99.7 | 105.8 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 842.4 | 1017.9 | 3879.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 90.6 | 98 | 162.5 |

## first.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 86.3 | 112.9 | 115 |
| 1 | Overview | cold-startup | 20 | 44.1 | 62.9 | 76.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 85 | 129 | 138.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 43.5 | 54.8 | 55.9 |
| 2 | Code-heavy | cold-startup | 20 | 972.6 | 1823.6 | 3315.7 |
| 1 | Code-heavy | cold-startup | 20 | 126.3 | 149.7 | 151.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 964.3 | 1197.3 | 4293.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 123.9 | 144.1 | 219 |

## first.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 7409166 | 7440280 | 7452886 |
| 0 | Small | cold-startup | 20 | 7411069 | 7444399 | 8004489 |
| 2 | Small | cold-startup | 19 | 7406030 | 7447166 | 7447166 |
| D | Small | cold-startup | 20 | 7404165 | 7447514 | 7459299 |
| D | Small | fresh-profile-after-examples-settled | 20 | 7403160 | 7418135 | 7422414 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 7409266 | 7448784 | 7454562 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7397278 | 7434938 | 7451548 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7409501 | 7435449 | 7447283 |
| 0 | Overview | cold-startup | 20 | 7737294 | 7768116 | 7787032 |
| 2 | Overview | cold-startup | 20 | 7773468 | 7802289 | 7809869 |
| 1 | Overview | cold-startup | 20 | 7767234 | 7797951 | 7805481 |
| D | Overview | cold-startup | 20 | 7721726 | 7753112 | 7764110 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 7718883 | 7736068 | 7743900 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 7771705 | 7794042 | 8043770 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 7762690 | 7789829 | 7799039 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7774707 | 7815473 | 8098172 |
| 2 | Code-heavy | cold-startup | 20 | 14621751 | 25321511 | 26117426 |
| 1 | Code-heavy | cold-startup | 20 | 8732926 | 8859967 | 9227903 |
| 0 | Code-heavy | cold-startup | 20 | 9260587 | 9329995 | 9606824 |
| D | Code-heavy | cold-startup | 20 | 7882834 | 7930593 | 7939217 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 7877830 | 7905419 | 7945796 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 20797531 | 22646479 | 23935223 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 9250799 | 9325529 | 9752007 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8735925 | 9278537 | 9438233 |

## second.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 132.8 | 179.5 | 195.9 |
| 0 | Small | cold-startup | 20 | 146.8 | 258.9 | 347.5 |
| 2 | Small | cold-startup | 19 | 130.3 | 263.1 | 263.1 |
| D | Small | cold-startup | 20 | 146.5 | 163.9 | 263.9 |
| D | Small | fresh-profile-after-examples-settled | 20 | 146.7 | 229.5 | 329.6 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 130.2 | 162.1 | 196.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 131.3 | 279 | 293.4 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 146.2 | 217.7 | 228.5 |
| 0 | Overview | cold-startup | 20 | 136.5 | 219.6 | 269.6 |
| 2 | Overview | cold-startup | 20 | 134.7 | 185 | 213.9 |
| 1 | Overview | cold-startup | 20 | 152.3 | 198.3 | 313 |
| D | Overview | cold-startup | 20 | 154.8 | 184.9 | 205.3 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 139.5 | 156.7 | 603.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 134.2 | 151.3 | 170.8 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 136.4 | 199.7 | 261.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 152.1 | 169.9 | 183.4 |
| 2 | Code-heavy | cold-startup | 20 | 542.6 | 743.7 | 1775.8 |
| 1 | Code-heavy | cold-startup | 20 | 407.9 | 470.4 | 530.4 |
| 0 | Code-heavy | cold-startup | 20 | 230.6 | 429.8 | 430.4 |
| D | Code-heavy | cold-startup | 20 | 138.8 | 170 | 292.9 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 139.9 | 155.9 | 185.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 542.5 | 689.9 | 689.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 243.4 | 441.5 | 1252.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 399.9 | 645.6 | 1157.8 |

## second.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 132.8 | 196.8 | 196.9 |
| 0 | Small | cold-startup | 20 | 146.8 | 258.9 | 347.5 |
| 2 | Small | cold-startup | 19 | 131.2 | 263.8 | 263.8 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 131.2 | 164 | 197.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 132.3 | 295.8 | 369.8 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 146.2 | 217.7 | 228.5 |
| 2 | Code-heavy | cold-startup | 20 | 1860.5 | 3496.3 | 4611.7 |
| 1 | Code-heavy | cold-startup | 20 | 885.5 | 1126.9 | 1424.3 |
| 0 | Code-heavy | cold-startup | 20 | 946.6 | 1214 | 1923.8 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1859.8 | 2067.1 | 2067.1 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 969.6 | 1258.8 | 2203.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 888.6 | 1196 | 2075.5 |

## second.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 132 | 196.8 | 196.9 |
| 0 | Small | cold-startup | 20 | 100.5 | 227.9 | 316.4 |
| 2 | Small | cold-startup | 19 | 131.2 | 263.8 | 263.8 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 131.2 | 164 | 197.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 132.3 | 295.8 | 369.8 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 99.5 | 183.1 | 186.9 |
| 2 | Code-heavy | cold-startup | 20 | 1860.5 | 3496.3 | 4611.7 |
| 1 | Code-heavy | cold-startup | 20 | 885.5 | 1126.9 | 1424.3 |
| 0 | Code-heavy | cold-startup | 20 | 946.6 | 1214 | 1923.8 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1859.8 | 2067.1 | 2067.1 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 969.6 | 1258.8 | 2203.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 888.6 | 1196 | 2075.5 |

## second.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 70 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 72 | 93 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 84 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 74 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 54 |
| 2 | Code-heavy | cold-startup | 20 | 618 | 1081 | 1549 |
| 1 | Code-heavy | cold-startup | 20 | 277 | 296 | 312 |
| 0 | Code-heavy | cold-startup | 20 | 114 | 178 | 268 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 610 | 707 | 707 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 121 | 201 | 256 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 271 | 290 | 744 |

## second.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 17.8 | 52.7 |
| 0 | Small | cold-startup | 20 | 0 | 113.9 | 201.3 |
| 2 | Small | cold-startup | 19 | 0 | 120.7 | 120.7 |
| D | Small | cold-startup | 20 | 0 | 17 | 119.8 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 85.5 | 187.2 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 17 | 48.8 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 134.2 | 146.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 73.7 | 83.1 |
| 0 | Overview | cold-startup | 20 | 0 | 68.8 | 126.1 |
| 2 | Overview | cold-startup | 20 | 0 | 42.3 | 68.7 |
| 1 | Overview | cold-startup | 20 | 5.7 | 49.4 | 162 |
| D | Overview | cold-startup | 20 | 5.7 | 13.1 | 17 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 12.3 | 455.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 6 | 6.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 53.8 | 123.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 17 | 40.2 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 140.7 | 212.2 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 275.7 | 381.1 |
| 0 | Code-heavy | cold-startup | 20 | 55.6 | 185.4 | 282 |
| D | Code-heavy | cold-startup | 20 | 0 | 17.3 | 146.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 11.7 | 15.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 98.7 | 98.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 82.2 | 168.3 | 269.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 114.1 | 125.8 |

## second.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 1.9 | 3 | 3.3 |
| 0 | Small | cold-startup | 20 | 2 | 2.5 | 2.9 |
| 2 | Small | cold-startup | 19 | 1.6 | 28.4 | 28.4 |
| D | Small | cold-startup | 20 | 1.9 | 3.4 | 4.5 |
| D | Small | fresh-profile-after-examples-settled | 20 | 2 | 2.4 | 2.7 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 1.9 | 3 | 3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1.9 | 3 | 4.6 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 2 | 2.4 | 3.3 |
| 0 | Overview | cold-startup | 20 | 2.3 | 2.8 | 2.8 |
| 2 | Overview | cold-startup | 20 | 1.9 | 2.8 | 3.9 |
| 1 | Overview | cold-startup | 20 | 1.9 | 7.3 | 9.9 |
| D | Overview | cold-startup | 20 | 1.8 | 3.1 | 3.7 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 2 | 2.7 | 2.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 2 | 2.4 | 2.5 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 2.2 | 3.7 | 3.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 2 | 2.6 | 3.4 |
| 2 | Code-heavy | cold-startup | 20 | 3.6 | 5.6 | 5.8 |
| 1 | Code-heavy | cold-startup | 20 | 3.3 | 6 | 6.9 |
| 0 | Code-heavy | cold-startup | 20 | 4.1 | 6 | 9.8 |
| D | Code-heavy | cold-startup | 20 | 4.1 | 5.9 | 8.4 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.8 | 5.9 | 6.8 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 3.5 | 5.6 | 5.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.2 | 6.1 | 6.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.4 | 5.2 | 5.6 |

## second.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 174944 | 174946 | 174946 |
| 0 | Small | cold-startup | 20 | 89223 | 89225 | 89225 |
| 2 | Small | cold-startup | 19 | 664663 | 664663 | 664663 |
| D | Small | cold-startup | 20 | 85340 | 85342 | 85342 |
| D | Small | fresh-profile-after-examples-settled | 20 | 85340 | 85342 | 85342 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 664661 | 664663 | 664663 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 174944 | 174946 | 174946 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 89223 | 89225 | 89225 |
| 0 | Overview | cold-startup | 20 | 163655 | 163662 | 163662 |
| 2 | Overview | cold-startup | 20 | 164048 | 164055 | 200639 |
| 1 | Overview | cold-startup | 20 | 164048 | 164055 | 200639 |
| D | Overview | cold-startup | 20 | 160376 | 160383 | 196967 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 160376 | 160383 | 196967 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 164048 | 164055 | 200639 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 163655 | 163662 | 163662 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 164048 | 164055 | 164055 |
| 2 | Code-heavy | cold-startup | 20 | 4562989 | 4562991 | 4562991 |
| 1 | Code-heavy | cold-startup | 20 | 217715 | 217717 | 217717 |
| 0 | Code-heavy | cold-startup | 20 | 633068 | 633070 | 633070 |
| D | Code-heavy | cold-startup | 20 | 128111 | 128113 | 128113 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 128111 | 128113 | 128113 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 4562991 | 4562991 | 4562991 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 633068 | 633070 | 633070 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 217715 | 217717 | 217717 |

## second.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 293103 | 293105 | 293105 |
| 0 | Small | cold-startup | 20 | 210982 | 210984 | 210984 |
| 2 | Small | cold-startup | 19 | 784322 | 784322 | 784322 |
| D | Small | cold-startup | 20 | 207399 | 207401 | 207401 |
| D | Small | fresh-profile-after-examples-settled | 20 | 207399 | 207401 | 207401 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 784320 | 784322 | 784322 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 293103 | 293105 | 293105 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 210982 | 210984 | 210984 |
| 0 | Overview | cold-startup | 20 | 232929 | 232936 | 232936 |
| 2 | Overview | cold-startup | 20 | 233322 | 233329 | 233329 |
| 1 | Overview | cold-startup | 20 | 233322 | 233329 | 233329 |
| D | Overview | cold-startup | 20 | 229650 | 229657 | 229657 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 229650 | 229657 | 229657 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 233322 | 233329 | 233329 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 232929 | 232936 | 232936 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 233322 | 233329 | 233329 |
| 2 | Code-heavy | cold-startup | 20 | 4662848 | 4662850 | 4662850 |
| 1 | Code-heavy | cold-startup | 20 | 335874 | 335876 | 335876 |
| 0 | Code-heavy | cold-startup | 20 | 754827 | 754829 | 754829 |
| D | Code-heavy | cold-startup | 20 | 250170 | 250172 | 250172 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 250170 | 250172 | 250172 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 4662850 | 4662850 | 4662850 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754827 | 754829 | 754829 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335874 | 335876 | 335876 |

## second.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 2 | Small | cold-startup | 19 | 0.1 | 0.2 | 0.2 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 18.8 | 21.199 | 23 |
| 1 | Code-heavy | cold-startup | 20 | 12.599 | 15.3 | 15.801 |
| 0 | Code-heavy | cold-startup | 20 | 11.6 | 13.5 | 13.5 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 18.601 | 21.4 | 21.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 11.6 | 12.7 | 14 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 12.6 | 14.799 | 14.8 |

## second.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 0 | Small | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 2 | Small | cold-startup | 19 | 0 | 0.1 | 0.1 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 6.8 | 7.401 | 8.399 |
| 1 | Code-heavy | cold-startup | 20 | 4 | 4.5 | 4.5 |
| 0 | Code-heavy | cold-startup | 20 | 4.7 | 5.099 | 5.201 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 6.7 | 9.101 | 9.101 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.501 | 4.8 | 5.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4 | 4.6 | 4.8 |

## second.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 70 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 72 | 93 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 84 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 74 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 54 |
| 2 | Code-heavy | cold-startup | 20 | 316 | 450 | 1233 |
| 1 | Code-heavy | cold-startup | 20 | 213 | 225 | 229 |
| 0 | Code-heavy | cold-startup | 20 | 114 | 178 | 205 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 317 | 360 | 360 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 121 | 156 | 195 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 211 | 266 | 744 |

## second.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 22.7 | 28.4 | 32.4 |
| 2 | Small | cold-startup | 19 | 20 | 27.4 | 27.4 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 19.4 | 24.1 | 44.9 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 23.7 | 30.6 | 79.7 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 19.3 | 25.1 | 33.4 |
| 1 | Code-heavy | cold-startup | 20 | 23.4 | 54.4 | 355.6 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 19.7 | 27.2 | 27.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 23.1 | 52.3 | 93 |

## second.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 2.7 | 4.6 | 4.6 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 2.6 | 3.2 | 3.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 85.4 | 230 | 236.7 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 83.9 | 123.2 | 123.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## second.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 2.7 | 3.1 | 3.4 |
| 2 | Small | cold-startup | 19 | 5.5 | 7 | 7 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 5.4 | 6.4 | 7 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 2.7 | 3.3 | 3.5 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 810 | 2009.3 | 2158.6 |
| 1 | Code-heavy | cold-startup | 20 | 92.3 | 147.4 | 165.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 793.2 | 943.1 | 943.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 91.9 | 196 | 439.3 |

## second.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 32.9 | 52.5 | 71.3 |
| 2 | Small | cold-startup | 19 | 35.5 | 47 | 47 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 35.2 | 57.9 | 60.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 34.3 | 90.4 | 153.1 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 1031.8 | 2266.5 | 2527.6 |
| 1 | Code-heavy | cold-startup | 20 | 193.8 | 277.2 | 465.7 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 989.8 | 1203.4 | 1203.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 196.6 | 367.6 | 651.2 |

## second.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 7513306 | 7544469 | 7547497 |
| 0 | Small | cold-startup | 20 | 7471670 | 7533901 | 7861493 |
| 2 | Small | cold-startup | 19 | 7508050 | 7551598 | 7551598 |
| D | Small | cold-startup | 20 | 7416430 | 7468450 | 7862408 |
| D | Small | fresh-profile-after-examples-settled | 20 | 7435800 | 7458783 | 7465598 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 7518489 | 7547881 | 7549281 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7510620 | 7580218 | 7970305 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7473135 | 7499851 | 7510206 |
| 0 | Overview | cold-startup | 20 | 7789648 | 7876712 | 7916520 |
| 2 | Overview | cold-startup | 20 | 7914574 | 7975404 | 8013989 |
| 1 | Overview | cold-startup | 20 | 7890357 | 7949816 | 7971094 |
| D | Overview | cold-startup | 20 | 7745859 | 7790670 | 7816302 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 7742790 | 7817174 | 8573542 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 7919995 | 7964870 | 8431098 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 7821795 | 7930888 | 8083506 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7878314 | 7950062 | 7985893 |
| 2 | Code-heavy | cold-startup | 20 | 23969705 | 44027932 | 44204636 |
| 1 | Code-heavy | cold-startup | 20 | 19032348 | 24346819 | 24493769 |
| 0 | Code-heavy | cold-startup | 20 | 21325712 | 25306825 | 26691612 |
| D | Code-heavy | cold-startup | 20 | 8448349 | 8463053 | 8581654 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 8443797 | 8470483 | 8496959 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 21501454 | 43910086 | 43910086 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 22258519 | 29128021 | 30909086 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 14482814 | 24340201 | 24523095 |

## retained.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 48.7 | 56.3 | 60.5 |
| 0 | Small | cold-startup | 20 | 49 | 55.7 | 115 |
| 2 | Small | cold-startup | 19 | 48.5 | 56.7 | 56.7 |
| D | Small | cold-startup | 20 | 48.6 | 49.6 | 49.7 |
| D | Small | fresh-profile-after-examples-settled | 20 | 49 | 52.2 | 52.6 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 48.6 | 64.4 | 64.8 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 48.6 | 55.3 | 58.9 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 48.9 | 50.2 | 59.3 |
| 0 | Overview | cold-startup | 20 | 48.9 | 57.5 | 61.2 |
| 2 | Overview | cold-startup | 20 | 48.9 | 54 | 59.2 |
| 1 | Overview | cold-startup | 20 | 48.8 | 51.7 | 54 |
| D | Overview | cold-startup | 20 | 48.8 | 51 | 98.9 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 48.7 | 53.3 | 56.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 48.5 | 53.1 | 55.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 48.8 | 50.4 | 52 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 48.7 | 50.3 | 72.9 |
| 2 | Code-heavy | cold-startup | 20 | 45.3 | 380.8 | 392.1 |
| 1 | Code-heavy | cold-startup | 20 | 48.2 | 319.9 | 365.8 |
| 0 | Code-heavy | cold-startup | 20 | 46 | 239.8 | 243.3 |
| D | Code-heavy | cold-startup | 20 | 46.5 | 53.7 | 56.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 46.2 | 47.7 | 54.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 45.2 | 393.2 | 393.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45.7 | 229.4 | 251.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 43.9 | 287 | 290.5 |

## retained.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 56 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 352 | 355 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 269 | 340 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 200 | 206 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 274 | 274 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 204 | 206 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 260 | 264 |

## retained.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 56 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 259 | 273 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 207 | 273 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 150 | 151 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 274 | 274 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 150 | 154 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 192 | 196 |

## retained.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 7800157 | 8600132 | 8656027 |
| 0 | Small | cold-startup | 20 | 7774990 | 8682872 | 8696525 |
| 2 | Small | cold-startup | 19 | 7801521 | 8565258 | 8565258 |
| D | Small | cold-startup | 20 | 8453180 | 8636023 | 8641005 |
| D | Small | fresh-profile-after-examples-settled | 20 | 7706336 | 8446115 | 8484311 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 7780737 | 8573959 | 8590611 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7808657 | 8541843 | 8572539 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7759057 | 8680623 | 8686788 |
| 0 | Overview | cold-startup | 20 | 8046257 | 8223111 | 8246085 |
| 2 | Overview | cold-startup | 20 | 8224612 | 8286454 | 9205286 |
| 1 | Overview | cold-startup | 20 | 8120401 | 8282324 | 9079646 |
| D | Overview | cold-startup | 20 | 8053844 | 8191571 | 9035511 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 8010934 | 8588460 | 8731448 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 8092902 | 8672879 | 9858527 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 8110540 | 8256234 | 8682588 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 8160243 | 8382363 | 8877475 |
| 2 | Code-heavy | cold-startup | 20 | 37113154 | 62964533 | 64854088 |
| 1 | Code-heavy | cold-startup | 20 | 24281666 | 36732361 | 41234457 |
| 0 | Code-heavy | cold-startup | 20 | 31857276 | 40876331 | 50422059 |
| D | Code-heavy | cold-startup | 20 | 8902120 | 9051317 | 9413324 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 8898013 | 8925566 | 8931899 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 35514830 | 61234208 | 61234208 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 26585976 | 42513752 | 48944322 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 15748190 | 36313386 | 41511481 |

## evicted.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 132.3 | 158.7 | 222.3 |
| 0 | Small | cold-startup | 20 | 132.1 | 304.6 | 314.9 |
| 2 | Small | cold-startup | 19 | 132.1 | 623.7 | 623.7 |
| D | Small | cold-startup | 20 | 132 | 148.6 | 232.7 |
| D | Small | fresh-profile-after-examples-settled | 20 | 132.2 | 232.4 | 303.3 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 131.9 | 178.5 | 179.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 132 | 279.9 | 439.7 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 131.9 | 397.6 | 399.9 |
| 0 | Overview | cold-startup | 20 | 131.9 | 235.3 | 248.1 |
| 2 | Overview | cold-startup | 20 | 131.4 | 225 | 230.8 |
| 1 | Overview | cold-startup | 20 | 131.9 | 279.3 | 498.2 |
| D | Overview | cold-startup | 20 | 131.8 | 215.3 | 300.8 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 131.5 | 191.5 | 229.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 131.4 | 165.9 | 206.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 132 | 148.6 | 164.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 131.7 | 181.5 | 182 |
| 2 | Code-heavy | cold-startup | 20 | 148.6 | 214.7 | 248 |
| 1 | Code-heavy | cold-startup | 20 | 148.1 | 265.1 | 367 |
| 0 | Code-heavy | cold-startup | 20 | 148.4 | 164.8 | 166 |
| D | Code-heavy | cold-startup | 20 | 131.9 | 165.4 | 280.2 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 148.4 | 165.4 | 170.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 148.1 | 198.7 | 198.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 133.1 | 315.6 | 757.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 148.4 | 166.1 | 332.2 |

## evicted.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 68 | 204 | 218 |
| 1 | Code-heavy | cold-startup | 20 | 57 | 219 | 404 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 110 | 127 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 66 | 280 | 280 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 117 | 131 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 103 | 198 | 213 |

## evicted.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 20.7 | 75.9 |
| 0 | Small | cold-startup | 20 | 0 | 163.7 | 170.8 |
| 2 | Small | cold-startup | 19 | 0 | 484.3 | 484.3 |
| D | Small | cold-startup | 20 | 0 | 6.2 | 90.4 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 91.2 | 137.4 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 33 | 43.9 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 136.7 | 295.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 253.8 | 255.7 |
| 0 | Overview | cold-startup | 20 | 0 | 79.7 | 102.9 |
| 2 | Overview | cold-startup | 20 | 0 | 78.7 | 93.4 |
| 1 | Overview | cold-startup | 20 | 0 | 123.3 | 350.1 |
| D | Overview | cold-startup | 20 | 0 | 69.8 | 149.7 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 46.9 | 82 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 16.5 | 56.7 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 17.8 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 37.4 | 38.3 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 77.7 | 113.2 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 118 | 216.8 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 18 | 21.9 |
| D | Code-heavy | cold-startup | 20 | 0 | 16.8 | 149.5 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 16.7 | 22.7 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 49.9 | 49.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 169.5 | 615.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 23.5 | 192.3 |

## evicted.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 1.4 | 2 | 2.2 |
| 0 | Small | cold-startup | 20 | 1.7 | 2.2 | 2.4 |
| 2 | Small | cold-startup | 19 | 1.5 | 2.5 | 2.5 |
| D | Small | cold-startup | 20 | 1.5 | 2 | 2.4 |
| D | Small | fresh-profile-after-examples-settled | 20 | 1.6 | 2.2 | 2.4 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 1.5 | 2.3 | 2.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1.6 | 2 | 2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1.6 | 2.7 | 3.2 |
| 0 | Overview | cold-startup | 20 | 1.8 | 2.7 | 5.2 |
| 2 | Overview | cold-startup | 20 | 1.7 | 3.6 | 4.7 |
| 1 | Overview | cold-startup | 20 | 1.9 | 2.7 | 3.5 |
| D | Overview | cold-startup | 20 | 1.6 | 2.3 | 2.7 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1.6 | 2.6 | 2.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1.8 | 2.4 | 2.6 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1.7 | 2.4 | 2.8 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1.7 | 2.2 | 2.8 |
| 2 | Code-heavy | cold-startup | 20 | 3.7 | 5.9 | 6.2 |
| 1 | Code-heavy | cold-startup | 20 | 3.3 | 5 | 5.2 |
| 0 | Code-heavy | cold-startup | 20 | 3.2 | 5 | 7 |
| D | Code-heavy | cold-startup | 20 | 3.3 | 4.7 | 5.8 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.2 | 4.3 | 4.7 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 3.7 | 6.4 | 6.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.5 | 4.7 | 5.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.4 | 4.7 | 5.4 |

## evicted.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 85708 | 85708 | 85708 |
| 0 | Small | cold-startup | 20 | 85315 | 85315 | 85315 |
| 2 | Small | cold-startup | 19 | 85708 | 85708 | 85708 |
| D | Small | cold-startup | 20 | 82036 | 82036 | 82036 |
| D | Small | fresh-profile-after-examples-settled | 20 | 82036 | 82036 | 82036 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 85708 | 85708 | 85708 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 85708 | 85708 | 85708 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 85315 | 85315 | 85315 |
| 0 | Overview | cold-startup | 20 | 155807 | 155807 | 155807 |
| 2 | Overview | cold-startup | 20 | 1040955 | 1040955 | 1040955 |
| 1 | Overview | cold-startup | 20 | 240249 | 240249 | 240249 |
| D | Overview | cold-startup | 20 | 150645 | 150645 | 150645 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 150645 | 150645 | 150645 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1040955 | 1040955 | 1040955 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 155807 | 155807 | 155807 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 240249 | 240249 | 240249 |
| 2 | Code-heavy | cold-startup | 20 | 4516914 | 4516914 | 4516914 |
| 1 | Code-heavy | cold-startup | 20 | 171640 | 171640 | 171640 |
| 0 | Code-heavy | cold-startup | 20 | 586993 | 586993 | 586993 |
| D | Code-heavy | cold-startup | 20 | 82036 | 82036 | 82036 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 82036 | 82036 | 82036 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 4516914 | 4516914 | 4516914 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 586993 | 586993 | 586993 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 171640 | 171640 | 171640 |

## evicted.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 211065 | 211067 | 211067 |
| 0 | Small | cold-startup | 20 | 210672 | 210674 | 210674 |
| 2 | Small | cold-startup | 19 | 211067 | 211067 | 211067 |
| D | Small | cold-startup | 20 | 207393 | 207395 | 207395 |
| D | Small | fresh-profile-after-examples-settled | 20 | 207393 | 207395 | 207395 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 211065 | 211067 | 211067 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 211065 | 211067 | 211067 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 210672 | 210674 | 210674 |
| 0 | Overview | cold-startup | 20 | 247675 | 247677 | 247677 |
| 2 | Overview | cold-startup | 20 | 1130123 | 1130125 | 1130125 |
| 1 | Overview | cold-startup | 20 | 328517 | 328519 | 328519 |
| D | Overview | cold-startup | 20 | 242813 | 242815 | 242815 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 242813 | 242815 | 242815 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1130123 | 1130125 | 1130125 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 247675 | 247677 | 247677 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 328517 | 328519 | 328519 |
| 2 | Code-heavy | cold-startup | 20 | 4662842 | 4662844 | 4662844 |
| 1 | Code-heavy | cold-startup | 20 | 335868 | 335870 | 335870 |
| 0 | Code-heavy | cold-startup | 20 | 754821 | 754823 | 754823 |
| D | Code-heavy | cold-startup | 20 | 250164 | 250166 | 250166 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 250164 | 250166 | 250166 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 4662844 | 4662844 | 4662844 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754821 | 754823 | 754823 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335868 | 335870 | 335870 |

## evicted.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.4 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.3 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Code-heavy | cold-startup | 20 | 15.2 | 16.301 | 26.6 |
| 1 | Code-heavy | cold-startup | 20 | 12.3 | 16.8 | 17.3 |
| 0 | Code-heavy | cold-startup | 20 | 10.3 | 11.9 | 17 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 15.1 | 16.8 | 16.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 10.2 | 14 | 33.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 12.1 | 15.3 | 16 |

## evicted.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0.1 | 0.2 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0 | 0.2 | 0.2 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.2 |
| 2 | Code-heavy | cold-startup | 20 | 6.5 | 7 | 8.199 |
| 1 | Code-heavy | cold-startup | 20 | 4.3 | 4.8 | 5.1 |
| 0 | Code-heavy | cold-startup | 20 | 4.499 | 4.9 | 5 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 6.799 | 7.101 | 7.101 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.3 | 5.1 | 5.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.1 | 4.5 | 4.699 |

## evicted.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 68 | 204 | 218 |
| 1 | Code-heavy | cold-startup | 20 | 57 | 219 | 352 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 110 | 127 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 66 | 210 | 210 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 117 | 131 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 103 | 149 | 160 |

## evicted.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 17.7 | 20 | 20 |
| 1 | Overview | cold-startup | 20 | 23.1 | 106.3 | 202.4 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 18.5 | 22.2 | 31.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 23.6 | 31.4 | 96 |
| 2 | Code-heavy | cold-startup | 20 | 17.8 | 22.1 | 35.6 |
| 1 | Code-heavy | cold-startup | 20 | 21.7 | 193.1 | 237 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 18.3 | 25.3 | 25.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 22.8 | 49.5 | 232.2 |

## evicted.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 7.3 | 8.1 | 8.4 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 7.5 | 9.3 | 9.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 76.8 | 162.6 | 212.1 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 74.7 | 143 | 143 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## evicted.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 41.1 | 56.1 | 64.7 |
| 1 | Overview | cold-startup | 20 | 6.8 | 7.6 | 8.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 42.4 | 47.7 | 55.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7 | 8.2 | 9 |
| 2 | Code-heavy | cold-startup | 20 | 755.7 | 1042 | 1099.1 |
| 1 | Code-heavy | cold-startup | 20 | 82.9 | 90.1 | 118.6 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 768 | 1795.7 | 1795.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 83.6 | 97.4 | 171.7 |

## evicted.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 73.4 | 102.5 | 121.6 |
| 1 | Overview | cold-startup | 20 | 36.5 | 121.1 | 232.4 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 76.5 | 94.4 | 167.8 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 38.1 | 111.5 | 141.1 |
| 2 | Code-heavy | cold-startup | 20 | 860.9 | 1219.8 | 1222 |
| 1 | Code-heavy | cold-startup | 20 | 116.1 | 283.1 | 330 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 868.8 | 1967 | 1967 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 114.7 | 146.1 | 425.8 |

## evicted.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 11231033 | 25064320 | 25247580 |
| 0 | Small | cold-startup | 20 | 11192379 | 25125454 | 25191597 |
| 2 | Small | cold-startup | 19 | 24759738 | 25286722 | 25286722 |
| D | Small | cold-startup | 20 | 24574038 | 24981470 | 25082647 |
| D | Small | fresh-profile-after-examples-settled | 20 | 11755860 | 25002111 | 25024063 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 24765900 | 25184442 | 25222390 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 24905419 | 25239997 | 25292939 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 10879594 | 24849824 | 24904209 |
| 0 | Overview | cold-startup | 20 | 14989138 | 25590091 | 25678301 |
| 2 | Overview | cold-startup | 20 | 14900577 | 26274816 | 26405037 |
| 1 | Overview | cold-startup | 20 | 25257258 | 26290734 | 26321244 |
| D | Overview | cold-startup | 20 | 14307483 | 25744200 | 25773638 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 14635187 | 25310499 | 25434212 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 25498455 | 26053642 | 26481158 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 25256462 | 25845588 | 25958875 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 25498164 | 25951940 | 25956217 |
| 2 | Code-heavy | cold-startup | 20 | 33842471 | 74976677 | 78645591 |
| 1 | Code-heavy | cold-startup | 20 | 26091333 | 50972153 | 55945186 |
| 0 | Code-heavy | cold-startup | 20 | 29700445 | 48691556 | 50340870 |
| D | Code-heavy | cold-startup | 20 | 12414854 | 13513190 | 17601744 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 12645399 | 13456391 | 13506776 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 42344656 | 70576899 | 70576899 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 22576441 | 40883219 | 50757268 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 26398213 | 51430649 | 62081235 |

## sourceFirst.readableMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 14.4 | 19.2 | 25.5 |
| 0 | Small | cold-startup | 20 | 14.5 | 15.3 | 15.5 |
| 2 | Small | cold-startup | 19 | 14.4 | 15.2 | 15.2 |
| D | Small | cold-startup | 20 | 14.2 | 15.4 | 15.9 |
| D | Small | fresh-profile-after-examples-settled | 20 | 14.3 | 15.1 | 15.2 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 14.3 | 16.6 | 29.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 14.3 | 15 | 15.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 14.1 | 14.7 | 14.8 |
| 0 | Overview | cold-startup | 20 | 14.4 | 16.3 | 18.6 |
| 2 | Overview | cold-startup | 20 | 12.5 | 21.5 | 23.5 |
| 1 | Overview | cold-startup | 20 | 12.4 | 22 | 22.3 |
| D | Overview | cold-startup | 20 | 14.2 | 15.1 | 15.3 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 14.5 | 15.2 | 15.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 11.9 | 21.9 | 27.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 14.1 | 15.3 | 20.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 12.1 | 16 | 19.2 |
| 2 | Code-heavy | cold-startup | 20 | 22.6 | 30.6 | 354.5 |
| 1 | Code-heavy | cold-startup | 20 | 19.4 | 24.9 | 46.5 |
| 0 | Code-heavy | cold-startup | 20 | 18 | 23.4 | 23.7 |
| D | Code-heavy | cold-startup | 20 | 10.1 | 23.7 | 24.9 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 10 | 22.4 | 22.7 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21.6 | 30.4 | 30.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 20.6 | 36.8 | 73.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 18.5 | 23.2 | 24.5 |

## sourceFirst.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.6 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Small | cold-startup | 19 | 0.2 | 0.3 | 0.3 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0.2 | 0.2 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.3 | 0.3 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.3 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.4 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Code-heavy | cold-startup | 20 | 3 | 3.3 | 3.6 |
| 1 | Code-heavy | cold-startup | 20 | 2.1 | 2.4 | 2.6 |
| 0 | Code-heavy | cold-startup | 20 | 2.1 | 2.3 | 2.5 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2.9 | 3.3 | 3.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2.1 | 2.2 | 4.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2.1 | 2.3 | 2.4 |

## sourceFirst.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| 2 | Small | cold-startup | 19 | 0.1 | 0.2 | 0.2 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 6.7 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 2 | Overview | cold-startup | 20 | 0 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 2 | Code-heavy | cold-startup | 20 | 1 | 1.1 | 1.7 |
| 1 | Code-heavy | cold-startup | 20 | 0.6 | 0.7 | 0.7 |
| 0 | Code-heavy | cold-startup | 20 | 0.6 | 0.7 | 0.7 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1 | 1.1 | 1.1 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.6 | 0.7 | 1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.6 | 0.7 | 1.3 |

## sourceFirst.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 250 | 254 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 162 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 250 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 160 | 260 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceFirst.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 87208 | 87208 | 87208 |
| 0 | Small | cold-startup | 20 | 1540 | 1540 | 1540 |
| 2 | Small | cold-startup | 19 | 766980 | 766980 | 766980 |
| D | Small | cold-startup | 20 | 638 | 638 | 638 |
| D | Small | fresh-profile-after-examples-settled | 20 | 638 | 638 | 638 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 766980 | 766980 | 766980 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 87208 | 87208 | 87208 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1540 | 1540 | 1540 |
| 0 | Overview | cold-startup | 20 | 4697 | 4697 | 4697 |
| 2 | Overview | cold-startup | 20 | 198727 | 198727 | 212426 |
| 1 | Overview | cold-startup | 20 | 4448 | 4448 | 4448 |
| D | Overview | cold-startup | 20 | 2224 | 2224 | 2224 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 2224 | 2224 | 2224 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 198727 | 198727 | 212426 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 4697 | 4697 | 4697 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 4448 | 4448 | 4448 |
| 2 | Code-heavy | cold-startup | 20 | 200228 | 200228 | 200228 |
| 1 | Code-heavy | cold-startup | 20 | 200228 | 200228 | 200228 |
| 0 | Code-heavy | cold-startup | 20 | 236384 | 236384 | 236384 |
| D | Code-heavy | cold-startup | 20 | 100114 | 100114 | 100114 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 100114 | 100114 | 100114 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 200228 | 200228 | 200228 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 236384 | 236384 | 236384 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 200228 | 200228 | 200228 |

## sourceFirst.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 82708 | 82708 | 82708 |
| 0 | Small | cold-startup | 20 | 940 | 940 | 940 |
| 2 | Small | cold-startup | 19 | 763980 | 763980 | 763980 |
| D | Small | cold-startup | 20 | 338 | 338 | 338 |
| D | Small | fresh-profile-after-examples-settled | 20 | 338 | 338 | 338 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 763980 | 763980 | 763980 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 82708 | 82708 | 82708 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 940 | 940 | 940 |
| 0 | Overview | cold-startup | 20 | 4097 | 4097 | 4097 |
| 2 | Overview | cold-startup | 20 | 197827 | 197827 | 210326 |
| 1 | Overview | cold-startup | 20 | 3848 | 3848 | 3848 |
| D | Overview | cold-startup | 20 | 1924 | 1924 | 1924 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1924 | 1924 | 1924 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 197827 | 197827 | 210326 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 4097 | 4097 | 4097 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 3848 | 3848 | 3848 |
| 2 | Code-heavy | cold-startup | 20 | 199628 | 199628 | 199628 |
| 1 | Code-heavy | cold-startup | 20 | 199628 | 199628 | 199628 |
| 0 | Code-heavy | cold-startup | 20 | 235784 | 235784 | 235784 |
| D | Code-heavy | cold-startup | 20 | 99814 | 99814 | 99814 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 99814 | 99814 | 99814 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 199628 | 199628 | 199628 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 235784 | 235784 | 235784 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 199628 | 199628 | 199628 |

## sourceFirst.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 79.8 | 100.7 | 110.6 |
| 0 | Small | cold-startup | 20 | 60.6 | 74 | 129.8 |
| 2 | Small | cold-startup | 19 | 163.3 | 219.6 | 219.6 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 162.5 | 198 | 290.3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 75.7 | 143.1 | 264 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 59.8 | 90.1 | 160.8 |
| 0 | Overview | cold-startup | 20 | 58.2 | 73.5 | 75.7 |
| 2 | Overview | cold-startup | 20 | 111.3 | 319.2 | 327.7 |
| 1 | Overview | cold-startup | 20 | 39.9 | 51.7 | 52.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 108.9 | 369.3 | 371.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 57.9 | 75.5 | 79.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 40.2 | 48.1 | 52.8 |
| 2 | Code-heavy | cold-startup | 20 | 112.7 | 422.6 | 456.5 |
| 1 | Code-heavy | cold-startup | 20 | 55.6 | 60.9 | 63.2 |
| 0 | Code-heavy | cold-startup | 20 | 129.7 | 274.2 | 352.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 107.1 | 130.7 | 364.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 124.7 | 391.8 | 554.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 56 | 58.6 | 64.1 |

## sourceFirst.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 24.1 | 29.7 | 31 |
| 2 | Small | cold-startup | 19 | 19.7 | 29.7 | 29.7 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 19.6 | 23.8 | 25.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 22.8 | 40.8 | 73 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceFirst.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 5.1 | 7.1 | 7.1 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 5.1 | 6 | 6.3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 3.4 | 5.6 | 10.3 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 3.1 | 3.9 | 4.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceFirst.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 8.1 | 9.1 | 9.5 |
| 2 | Small | cold-startup | 19 | 92.9 | 136.4 | 136.4 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 91.6 | 102.4 | 197.3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 8 | 8.5 | 8.6 |
| 2 | Overview | cold-startup | 20 | 72.6 | 280.6 | 284.6 |
| 1 | Overview | cold-startup | 20 | 6.7 | 7.5 | 7.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 70.6 | 323.6 | 327.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 6.7 | 7.3 | 9.6 |
| 2 | Code-heavy | cold-startup | 20 | 49.9 | 65.6 | 87.9 |
| 1 | Code-heavy | cold-startup | 20 | 7.8 | 8.9 | 8.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 48.8 | 64.3 | 65.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 7.9 | 8.7 | 8.8 |

## sourceFirst.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 41.2 | 55.1 | 55.7 |
| 2 | Small | cold-startup | 19 | 125.1 | 181.1 | 181.1 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 124 | 151.9 | 250.8 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 38.1 | 98.3 | 209.8 |
| 2 | Overview | cold-startup | 20 | 76.6 | 284.3 | 290.7 |
| 1 | Overview | cold-startup | 20 | 7 | 7.7 | 7.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 74.4 | 327.4 | 332.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7 | 7.6 | 10 |
| 2 | Code-heavy | cold-startup | 20 | 52.4 | 326.9 | 330.7 |
| 1 | Code-heavy | cold-startup | 20 | 8.8 | 10 | 10.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 51.1 | 67.7 | 329.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8.9 | 9.7 | 9.7 |

## sourceReopen.readableMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 15.4 | 16.4 | 17.6 |
| 0 | Small | cold-startup | 20 | 15.6 | 16 | 16.1 |
| 2 | Small | cold-startup | 19 | 15.3 | 16.4 | 16.4 |
| D | Small | cold-startup | 20 | 15.5 | 16.2 | 16.2 |
| D | Small | fresh-profile-after-examples-settled | 20 | 15.7 | 16.4 | 16.6 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 15.5 | 16.2 | 16.3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 15.4 | 16.5 | 16.8 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 15.5 | 16.5 | 17.4 |
| 0 | Overview | cold-startup | 20 | 15.4 | 16.9 | 17.6 |
| 2 | Overview | cold-startup | 20 | 15.3 | 15.8 | 16.3 |
| 1 | Overview | cold-startup | 20 | 15.3 | 16 | 16.6 |
| D | Overview | cold-startup | 20 | 14.9 | 16.1 | 16.3 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 14.9 | 15.4 | 15.5 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 15.2 | 15.8 | 16.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 15.4 | 16.1 | 16.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 15.2 | 15.6 | 15.9 |
| 2 | Code-heavy | cold-startup | 20 | 12.8 | 13.5 | 13.8 |
| 1 | Code-heavy | cold-startup | 20 | 14 | 14.3 | 14.3 |
| 0 | Code-heavy | cold-startup | 20 | 13.8 | 236.5 | 249.4 |
| D | Code-heavy | cold-startup | 20 | 14.1 | 15 | 15.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 14.3 | 15.1 | 15.4 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 13 | 14.1 | 14.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 14.3 | 14.7 | 15.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 13.7 | 14.8 | 15 |

## sourceReopen.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 175 | 180 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Small | cold-startup | 19 | 0.1 | 0.1 | 0.1 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.3 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Code-heavy | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Code-heavy | cold-startup | 20 | 0.1 | 0.3 | 0.6 |
| 0 | Code-heavy | cold-startup | 20 | 0.1 | 0.2 | 0.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 2.1 |

## sourceReopen.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## edit.fence.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 44.933 | 51.077 | 80.578 |
| 0 | Small | cold-startup | 20 | 44.869 | 52.349 | 75.088 |
| 2 | Small | cold-startup | 19 | 45.459 | 131.675 | 131.675 |
| D | Small | cold-startup | 20 | 45.039 | 49.218 | 59.906 |
| D | Small | fresh-profile-after-examples-settled | 20 | 45.111 | 47.01 | 101.619 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 44.982 | 61.849 | 66.643 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 44.646 | 63.028 | 104.954 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 45.17 | 76.534 | 143.596 |
| 0 | Overview | cold-startup | 20 | 45.705 | 118.784 | 226.865 |
| 2 | Overview | cold-startup | 20 | 44.94 | 56.194 | 57.456 |
| 1 | Overview | cold-startup | 20 | 45.56 | 60.829 | 100.656 |
| D | Overview | cold-startup | 20 | 44.799 | 49.944 | 58.309 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 44.9 | 52.676 | 54.081 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 44.604 | 46.899 | 47.233 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 44.718 | 52.984 | 53.357 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 44.514 | 48.87 | 50.609 |
| 2 | Code-heavy | cold-startup | 20 | 792.309 | 1173.704 | 2840.687 |
| 1 | Code-heavy | cold-startup | 20 | 68.714 | 623.925 | 773.166 |
| 0 | Code-heavy | cold-startup | 20 | 68.291 | 476.05 | 522.036 |
| D | Code-heavy | cold-startup | 20 | 47.247 | 79.738 | 81.555 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 46.108 | 65.008 | 76.385 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 130.941 | 2180.189 | 2180.189 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 68.487 | 433.929 | 4283.457 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 70.265 | 700.699 | 707.264 |

## edit.fence.highlightSettledMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 116.985 | 132.29 | 168.353 |
| 0 | Small | cold-startup | 20 | 83.448 | 84.851 | 209.113 |
| 2 | Small | cold-startup | 19 | 148.03 | 229.141 | 229.141 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 145.387 | 163.175 | 190.907 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 116.837 | 138.192 | 172.71 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 83.286 | 116.113 | 145.012 |
| 0 | Overview | cold-startup | 20 | 100.047 | 150.713 | 229.578 |
| 2 | Overview | cold-startup | 20 | 133.433 | 134.443 | 134.911 |
| 1 | Overview | cold-startup | 20 | 100.146 | 112.2 | 132.96 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 133.493 | 135.094 | 149.316 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 100.097 | 102.102 | 102.805 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 99.964 | 101.438 | 101.705 |
| 2 | Code-heavy | cold-startup | 20 | 1100.482 | 1533.341 | 3561.874 |
| 1 | Code-heavy | cold-startup | 20 | 670.655 | 877.461 | 961.929 |
| 0 | Code-heavy | cold-startup | 20 | 909.487 | 1173.954 | 1191.524 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 778.585 | 2430.397 | 2430.397 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 840.466 | 1169.744 | 8452.665 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 679.224 | 829.144 | 858.026 |

## edit.example.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 172.373 | 364.715 | 913.045 |
| 0 | Small | cold-startup | 20 | 168.269 | 592.652 | 1169.951 |
| 2 | Small | cold-startup | 19 | 175.546 | 803.932 | 803.932 |
| D | Small | cold-startup | 20 | 164.936 | 243.758 | 862.921 |
| D | Small | fresh-profile-after-examples-settled | 20 | 164.417 | 958.631 | 996.515 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 173.753 | 431.654 | 836.228 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 164.141 | 977.96 | 1064.265 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 165.316 | 797.556 | 841.03 |
| 0 | Overview | cold-startup | 20 | 116.095 | 592.979 | 734.48 |
| 2 | Overview | cold-startup | 20 | 124.663 | 570.357 | 739.654 |
| 1 | Overview | cold-startup | 20 | 114.615 | 779.162 | 966.042 |
| D | Overview | cold-startup | 20 | 106.362 | 592.267 | 626.153 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 107.366 | 680.571 | 716.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 108.077 | 150.077 | 630.388 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 107.918 | 271.134 | 382.746 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 118.155 | 154.527 | 548.198 |
| 2 | Code-heavy | cold-startup | 20 | 255.561 | 1161.209 | 1246.661 |
| 1 | Code-heavy | cold-startup | 20 | 247.724 | 755.227 | 992.926 |
| 0 | Code-heavy | cold-startup | 20 | 205.993 | 292.27 | 628.547 |
| D | Code-heavy | cold-startup | 20 | 191.853 | 660.985 | 792.882 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 190.717 | 243.261 | 650.459 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 232.024 | 1400.779 | 1400.779 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 230.655 | 625.264 | 6886.943 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 236.669 | 309.132 | 2577.499 |

## edit.shared.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 100.548 | 396.305 | 489.11 |
| 0 | Small | cold-startup | 20 | 101.9 | 147.251 | 488.604 |
| 2 | Small | cold-startup | 19 | 110.885 | 601.731 | 601.731 |
| D | Small | cold-startup | 20 | 100.048 | 148.972 | 326.663 |
| D | Small | fresh-profile-after-examples-settled | 20 | 98.518 | 629.598 | 660.239 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 100.278 | 267.189 | 461.841 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 97.162 | 663.003 | 744.662 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 102.512 | 515.924 | 546.276 |
| 0 | Overview | cold-startup | 20 | 120.33 | 685.536 | 746.804 |
| 2 | Overview | cold-startup | 20 | 123.38 | 572.405 | 633.291 |
| 1 | Overview | cold-startup | 20 | 131.894 | 847.38 | 877.963 |
| D | Overview | cold-startup | 20 | 114.217 | 608.718 | 626.786 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 107.212 | 504.312 | 554.718 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 125.888 | 152.077 | 472.091 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 108.805 | 255.294 | 302.512 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 127.758 | 228.298 | 679.641 |
| 2 | Code-heavy | cold-startup | 20 | 159.678 | 331.663 | 373.46 |
| 1 | Code-heavy | cold-startup | 20 | 142.206 | 420.161 | 495.694 |
| 0 | Code-heavy | cold-startup | 20 | 146.038 | 375.75 | 381.234 |
| D | Code-heavy | cold-startup | 20 | 127.454 | 215.53 | 370.372 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 121.178 | 200.389 | 341.528 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 157.518 | 301.788 | 301.788 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 147.925 | 271.716 | 6227.847 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 139.566 | 274.555 | 370.665 |

## memory.peakServerRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 451559424 | 464490496 | 472461312 |
| 0 | Small | cold-startup | 20 | 460640256 | 474460160 | 488210432 |
| 2 | Small | cold-startup | 19 | 454225920 | 472903680 | 472903680 |
| D | Small | cold-startup | 20 | 451358720 | 465805312 | 467349504 |
| D | Small | fresh-profile-after-examples-settled | 20 | 453689344 | 463884288 | 472031232 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 457220096 | 471285760 | 474836992 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 455798784 | 477114368 | 481583104 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 458792960 | 485777408 | 487436288 |
| 0 | Overview | cold-startup | 20 | 484470784 | 508358656 | 510427136 |
| 2 | Overview | cold-startup | 20 | 486404096 | 507691008 | 523112448 |
| 1 | Overview | cold-startup | 20 | 480006144 | 501501952 | 516251648 |
| D | Overview | cold-startup | 20 | 478285824 | 501219328 | 516108288 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 474394624 | 490582016 | 505131008 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 479715328 | 501923840 | 529006592 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 482807808 | 498012160 | 505577472 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 476487680 | 488923136 | 503078912 |
| 2 | Code-heavy | cold-startup | 20 | 1107267584 | 1238380544 | 1302315008 |
| 1 | Code-heavy | cold-startup | 20 | 888950784 | 956317696 | 961404928 |
| 0 | Code-heavy | cold-startup | 20 | 903090176 | 969240576 | 990363648 |
| D | Code-heavy | cold-startup | 20 | 486076416 | 510844928 | 514101248 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 491327488 | 502452224 | 509145088 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1091870720 | 1180749824 | 1180749824 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 924991488 | 1002430464 | 1072365568 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 904323072 | 947740672 | 962977792 |

## memory.peakBrowserRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 984805376 | 1025093632 | 1035452416 |
| 0 | Small | cold-startup | 20 | 985010176 | 1026228224 | 1037090816 |
| 2 | Small | cold-startup | 19 | 1008664576 | 1070632960 | 1070632960 |
| D | Small | cold-startup | 20 | 996880384 | 1024802816 | 1048502272 |
| D | Small | fresh-profile-after-examples-settled | 20 | 987656192 | 1022943232 | 1023692800 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 1011466240 | 1034334208 | 1047945216 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 996061184 | 1033166848 | 1070784512 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 986345472 | 1015582720 | 1034964992 |
| 0 | Overview | cold-startup | 20 | 1002799104 | 1029767168 | 1036537856 |
| 2 | Overview | cold-startup | 20 | 1038254080 | 1084231680 | 1091268608 |
| 1 | Overview | cold-startup | 20 | 1008431104 | 1037213696 | 1073664000 |
| D | Overview | cold-startup | 20 | 1004433408 | 1036443648 | 1057972224 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1011847168 | 1060311040 | 1064263680 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1038184448 | 1082310656 | 1082744832 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1021227008 | 1043124224 | 1055035392 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1016987648 | 1043582976 | 1061044224 |
| 2 | Code-heavy | cold-startup | 20 | 1463037952 | 1514463232 | 1531269120 |
| 1 | Code-heavy | cold-startup | 20 | 1237016576 | 1280540672 | 1298776064 |
| 0 | Code-heavy | cold-startup | 20 | 1132826624 | 1255899136 | 1269100544 |
| D | Code-heavy | cold-startup | 20 | 1033461760 | 1062752256 | 1074159616 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 1017663488 | 1045327872 | 1066520576 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1457389568 | 1523941376 | 1523941376 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1142087680 | 1254895616 | 1261420544 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1186582528 | 1249697792 | 1276071936 |

## memory.settledServerRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 451559424 | 458014720 | 472461312 |
| 0 | Small | cold-startup | 20 | 460058624 | 467697664 | 488210432 |
| 2 | Small | cold-startup | 19 | 453275648 | 472903680 | 472903680 |
| D | Small | cold-startup | 20 | 451358720 | 459075584 | 467349504 |
| D | Small | fresh-profile-after-examples-settled | 20 | 451956736 | 463884288 | 472031232 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 452243456 | 466071552 | 474836992 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 452751360 | 477114368 | 481583104 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 458600448 | 485777408 | 487436288 |
| 0 | Overview | cold-startup | 20 | 481886208 | 502906880 | 508358656 |
| 2 | Overview | cold-startup | 20 | 483655680 | 507691008 | 523112448 |
| 1 | Overview | cold-startup | 20 | 479191040 | 499986432 | 516251648 |
| D | Overview | cold-startup | 20 | 475942912 | 501219328 | 515260416 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 473067520 | 490582016 | 505131008 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 479408128 | 501923840 | 529006592 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 482242560 | 498012160 | 505577472 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 476487680 | 487399424 | 503078912 |
| 2 | Code-heavy | cold-startup | 20 | 1080463360 | 1232752640 | 1300930560 |
| 1 | Code-heavy | cold-startup | 20 | 888950784 | 956317696 | 961404928 |
| 0 | Code-heavy | cold-startup | 20 | 898793472 | 969240576 | 986902528 |
| D | Code-heavy | cold-startup | 20 | 486076416 | 505184256 | 510844928 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 490090496 | 502452224 | 509145088 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1064759296 | 1172869120 | 1172869120 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 924991488 | 994795520 | 1072365568 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 903311360 | 945664000 | 962977792 |

## memory.settledBrowserRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 932745216 | 947220480 | 949657600 |
| 0 | Small | cold-startup | 20 | 929783808 | 941613056 | 943808512 |
| 2 | Small | cold-startup | 19 | 963227648 | 967557120 | 967557120 |
| D | Small | cold-startup | 20 | 937635840 | 941170688 | 945643520 |
| D | Small | fresh-profile-after-examples-settled | 20 | 930418688 | 941740032 | 943742976 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 959594496 | 963690496 | 964063232 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 938242048 | 943636480 | 943751168 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 928428032 | 940093440 | 946425856 |
| 0 | Overview | cold-startup | 20 | 943706112 | 973070336 | 974454784 |
| 2 | Overview | cold-startup | 20 | 975876096 | 1009000448 | 1009770496 |
| 1 | Overview | cold-startup | 20 | 971460608 | 978788352 | 979488768 |
| D | Overview | cold-startup | 20 | 942850048 | 969465856 | 972464128 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 941051904 | 970240000 | 970616832 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1002242048 | 1010135040 | 1012015104 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 970133504 | 972013568 | 972242944 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 975491072 | 978579456 | 979038208 |
| 2 | Code-heavy | cold-startup | 20 | 1349926912 | 1405816832 | 1406980096 |
| 1 | Code-heavy | cold-startup | 20 | 1209212928 | 1251303424 | 1280794624 |
| 0 | Code-heavy | cold-startup | 20 | 1124184064 | 1228378112 | 1236115456 |
| D | Code-heavy | cold-startup | 20 | 944373760 | 984117248 | 984788992 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 978046976 | 980926464 | 982831104 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1343754240 | 1419771904 | 1419771904 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1105866752 | 1219747840 | 1222299648 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1177636864 | 1228472320 | 1255120896 |

## memory.settledCombinedRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 1387229184 | 1400823808 | 1402683392 |
| 0 | Small | cold-startup | 20 | 1390899200 | 1404248064 | 1404772352 |
| 2 | Small | cold-startup | 19 | 1417179136 | 1426423808 | 1426423808 |
| D | Small | cold-startup | 20 | 1388965888 | 1403793408 | 1403957248 |
| D | Small | fresh-profile-after-examples-settled | 20 | 1385713664 | 1393979392 | 1405624320 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 1412169728 | 1420677120 | 1437986816 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1392476160 | 1400025088 | 1404502016 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1388085248 | 1405845504 | 1409798144 |
| 0 | Overview | cold-startup | 20 | 1441988608 | 1458307072 | 1458954240 |
| 2 | Overview | cold-startup | 20 | 1471029248 | 1497726976 | 1501671424 |
| 1 | Overview | cold-startup | 20 | 1446920192 | 1472151552 | 1478348800 |
| D | Overview | cold-startup | 20 | 1428799488 | 1454899200 | 1458667520 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1418698752 | 1446182912 | 1446436864 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1482039296 | 1503338496 | 1503973376 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1446547456 | 1467326464 | 1477152768 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1448230912 | 1461923840 | 1464860672 |
| 2 | Code-heavy | cold-startup | 20 | 2446946304 | 2609512448 | 2658091008 |
| 1 | Code-heavy | cold-startup | 20 | 2100051968 | 2154520576 | 2207621120 |
| 0 | Code-heavy | cold-startup | 20 | 2040201216 | 2146365440 | 2162462720 |
| D | Code-heavy | cold-startup | 20 | 1443840000 | 1477287936 | 1482227712 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 1463762944 | 1481023488 | 1487765504 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 2416340992 | 2551865344 | 2551865344 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2053464064 | 2176724992 | 2196307968 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2050461696 | 2154622976 | 2171322368 |

## cpu.ownedProcessesSeconds

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 17.04 | 18.53 | 18.72 |
| 0 | Small | cold-startup | 20 | 16.45 | 18.81 | 19.81 |
| 2 | Small | cold-startup | 19 | 16.95 | 20.2 | 20.2 |
| D | Small | cold-startup | 20 | 16.23 | 18.2 | 18.31 |
| D | Small | fresh-profile-after-examples-settled | 20 | 16.22 | 17.61 | 18.14 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 16.5 | 17.71 | 18.41 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 16.48 | 19.03 | 19.5 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 16.43 | 18.85 | 19.01 |
| 0 | Overview | cold-startup | 20 | 16.61 | 19.26 | 23.41 |
| 2 | Overview | cold-startup | 20 | 18.5 | 20 | 21.18 |
| 1 | Overview | cold-startup | 20 | 17.27 | 22.95 | 25.14 |
| D | Overview | cold-startup | 20 | 16.82 | 18.73 | 20.02 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 16.62 | 18.24 | 19.14 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 18.28 | 21.27 | 21.65 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 16.9 | 18.51 | 19.15 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 17.35 | 19.04 | 19.07 |
| 2 | Code-heavy | cold-startup | 20 | 41.01 | 49.97 | 50.79 |
| 1 | Code-heavy | cold-startup | 20 | 24.4 | 30.5 | 35.29 |
| 0 | Code-heavy | cold-startup | 20 | 24.95 | 31.44 | 32.31 |
| D | Code-heavy | cold-startup | 20 | 16.29 | 19.61 | 20.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 16.57 | 18.54 | 19.14 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 39.47 | 47.22 | 47.22 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 25.09 | 31.59 | 34.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 23.99 | 27.39 | 30.59 |

## memory.workerHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 19 | 0 | 0 | 0 |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 2075512 | 2094392 | 2100932 |
| 1 | Overview | cold-startup | 20 | 1100812 | 1101364 | 1101392 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 2061216 | 2091076 | 2100956 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1101000 | 1101364 | 1101376 |
| 2 | Code-heavy | cold-startup | 20 | 21365344 | 37830936 | 37941524 |
| 1 | Code-heavy | cold-startup | 20 | 8180764 | 8230380 | 8252276 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 21364044 | 21848084 | 21848084 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8192248 | 8241136 | 8258260 |

## cache.compilerBytes

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 383720 | 384367 | 384369 |
| 0 | Small | cold-startup | 20 | 383721 | 384369 | 384369 |
| 2 | Small | cold-startup | 19 | 384345 | 384369 | 384369 |
| D | Small | cold-startup | 20 | 383718 | 384349 | 384350 |
| D | Small | fresh-profile-after-examples-settled | 20 | 383735 | 384366 | 384367 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 383730 | 384351 | 384352 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 383724 | 384348 | 384350 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 383725 | 384350 | 384351 |
| 0 | Overview | cold-startup | 20 | 507987 | 508847 | 508848 |
| 2 | Overview | cold-startup | 20 | 507986 | 508853 | 508854 |
| 1 | Overview | cold-startup | 20 | 507986 | 508852 | 508852 |
| D | Overview | cold-startup | 20 | 507992 | 508867 | 508868 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 507988 | 508863 | 508864 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 507992 | 508868 | 508875 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 507991 | 508873 | 508874 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 507991 | 508873 | 508878 |
| 2 | Code-heavy | cold-startup | 20 | 466553 | 467125 | 467127 |
| 1 | Code-heavy | cold-startup | 20 | 466550 | 467123 | 467124 |
| 0 | Code-heavy | cold-startup | 20 | 466551 | 467126 | 467129 |
| D | Code-heavy | cold-startup | 20 | 466564 | 467118 | 467118 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 466566 | 467140 | 467141 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 467110 | 467128 | 467128 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 466574 | 467123 | 467127 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 466576 | 467124 | 467126 |

## cache.indexBytes

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 467 | 467 | 467 |
| 0 | Small | cold-startup | 20 | 467 | 467 | 467 |
| 2 | Small | cold-startup | 19 | 467 | 467 | 467 |
| D | Small | cold-startup | 20 | 467 | 467 | 467 |
| D | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 0 | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| 2 | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| 1 | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| D | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 2 | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| 1 | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| 0 | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| D | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 467 | 467 | 467 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |

## first.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 219.2 | 718.7 | 951.1 |
| 2 | Overview | cold-startup | 20 | 281.5 | 546 | 1411.8 |
| 1 | Overview | cold-startup | 20 | 229.5 | 615.3 | 1709 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 269.9 | 705.5 | 1500.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 207 | 303.1 | 1250 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 229.8 | 286.1 | 771.9 |
| 2 | Code-heavy | cold-startup | 20 | 1670.3 | 2592.1 | 4306.9 |
| 1 | Code-heavy | cold-startup | 20 | 649.8 | 967.5 | 1359.2 |
| 0 | Code-heavy | cold-startup | 20 | 874.1 | 1223.2 | 1272.3 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1703.6 | 2293.4 | 2293.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 852.1 | 1185.1 | 1837.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 637.5 | 741.4 | 838.4 |

## first.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 194.9 | 692 | 920.4 |
| 2 | Overview | cold-startup | 20 | 281.5 | 546 | 1411.8 |
| 1 | Overview | cold-startup | 20 | 229.5 | 615.3 | 1693.7 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 269.9 | 705.5 | 1500.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 188.2 | 288.9 | 1200.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 229.8 | 286.1 | 771.9 |
| 2 | Code-heavy | cold-startup | 20 | 1670.3 | 2592.1 | 4306.9 |
| 1 | Code-heavy | cold-startup | 20 | 649.8 | 967.5 | 1359.2 |
| 0 | Code-heavy | cold-startup | 20 | 874.1 | 1223.2 | 1272.3 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1703.6 | 2293.4 | 2293.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 852.1 | 1185.1 | 1837.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 637.5 | 741.4 | 838.4 |

## retained.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 48.9 | 57.5 | 61.2 |
| 2 | Overview | cold-startup | 20 | 48.9 | 54 | 59.2 |
| 1 | Overview | cold-startup | 20 | 48.8 | 51.7 | 54 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 48.5 | 53.1 | 55.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 48.8 | 50.4 | 52 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 48.7 | 50.3 | 72.9 |
| 2 | Code-heavy | cold-startup | 20 | 45.3 | 380.8 | 392.1 |
| 1 | Code-heavy | cold-startup | 20 | 48.2 | 319.9 | 365.8 |
| 0 | Code-heavy | cold-startup | 20 | 46 | 239.8 | 243.3 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 45.2 | 393.2 | 393.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45.7 | 229.4 | 251.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 43.9 | 287 | 290.5 |

## retained.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## evicted.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 131.9 | 235.3 | 248.1 |
| 2 | Overview | cold-startup | 20 | 155.2 | 281.8 | 302.8 |
| 1 | Overview | cold-startup | 20 | 132.4 | 431.5 | 574.5 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 160.5 | 198 | 328.8 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 132 | 148.6 | 164.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 132.4 | 235 | 267.7 |
| 2 | Code-heavy | cold-startup | 20 | 1366.4 | 1750.5 | 1798 |
| 1 | Code-heavy | cold-startup | 20 | 616 | 975.1 | 1068.6 |
| 0 | Code-heavy | cold-startup | 20 | 716.2 | 848.4 | 1085.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1364.8 | 2500.4 | 2500.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 19 | 665.9 | 1119 | 1119 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 648.8 | 734 | 1146.5 |

## evicted.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 102.4 | 198.8 | 218.1 |
| 2 | Overview | cold-startup | 20 | 155.2 | 281.8 | 302.8 |
| 1 | Overview | cold-startup | 20 | 132.4 | 431.5 | 574.5 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 160.5 | 198 | 328.8 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 102.3 | 119.2 | 135.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 132.4 | 235 | 267.7 |
| 2 | Code-heavy | cold-startup | 20 | 1366.4 | 1750.5 | 1798 |
| 1 | Code-heavy | cold-startup | 20 | 616 | 975.1 | 1068.6 |
| 0 | Code-heavy | cold-startup | 20 | 716.2 | 848.4 | 1085.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 19 | 1364.8 | 2500.4 | 2500.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 19 | 665.9 | 1119 | 1119 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 648.8 | 734 | 1146.5 |
