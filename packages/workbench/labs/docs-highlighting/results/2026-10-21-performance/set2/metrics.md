# Performance observations

Planned n=20; inspect actual n per cell. Incomplete sets and correctness failures cannot establish a performance verdict; nearest-rank tails remain exploratory.

Nearest rank, rounded for display; JSON/CSV retains full precision. Values are kept separate by fixture, cache condition and run set.

## first.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 209.2 | 264.7 | 342.3 |
| 1 | Small | cold-startup | 20 | 228.8 | 536.1 | 1447.6 |
| 0 | Small | cold-startup | 20 | 245.9 | 799.3 | 1181.2 |
| 2 | Small | cold-startup | 20 | 226.7 | 352.2 | 514.2 |
| D | Small | fresh-profile-after-examples-settled | 20 | 225.3 | 358 | 898.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 227.8 | 668.3 | 1148.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 227.1 | 1022.1 | 1663.2 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 220.4 | 474.3 | 527.2 |
| D | Overview | cold-startup | 20 | 237.8 | 449.2 | 694 |
| 0 | Overview | cold-startup | 20 | 223.1 | 446.8 | 1207.2 |
| 2 | Overview | cold-startup | 20 | 238.2 | 854.3 | 908.6 |
| 1 | Overview | cold-startup | 20 | 244.1 | 365.8 | 551.1 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 259.1 | 675.5 | 1030.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 238.1 | 1035 | 1035.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 228.8 | 572 | 944.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 239.7 | 628.8 | 1016.7 |
| 1 | Code-heavy | cold-startup | 20 | 281.3 | 513.8 | 560.6 |
| 2 | Code-heavy | cold-startup | 20 | 275.8 | 574.7 | 827.4 |
| 0 | Code-heavy | cold-startup | 20 | 281 | 431.8 | 661.4 |
| D | Code-heavy | cold-startup | 20 | 303.6 | 638.8 | 724.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 276 | 435.7 | 488.8 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 294.7 | 640.7 | 814.3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 281.5 | 721 | 848 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 288.5 | 530.5 | 661.6 |

## first.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 79 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 61 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 72 | 154 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 63 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 51 | 74 |
| 2 | Code-heavy | cold-startup | 20 | 169 | 244 | 300 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 129 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 87 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 94 | 190 | 212 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 77 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 74 | 155 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 151 |

## first.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 59.3 | 117.8 | 134.2 |
| 1 | Small | cold-startup | 20 | 83.9 | 385.3 | 1301.8 |
| 0 | Small | cold-startup | 20 | 93.4 | 649.3 | 1034.1 |
| 2 | Small | cold-startup | 20 | 77.8 | 148.1 | 360.8 |
| D | Small | fresh-profile-after-examples-settled | 20 | 82.8 | 203.8 | 753.6 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 81.2 | 519.1 | 1002.8 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 76.9 | 877.5 | 1509.4 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 71.7 | 326.3 | 377.3 |
| D | Overview | cold-startup | 20 | 89.9 | 290.9 | 503.4 |
| 0 | Overview | cold-startup | 20 | 72.7 | 300.6 | 1070 |
| 2 | Overview | cold-startup | 20 | 92.1 | 630.4 | 711.3 |
| 1 | Overview | cold-startup | 20 | 89.5 | 193.3 | 380 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 112.6 | 533.8 | 886.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 92.8 | 891.9 | 893.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 76.9 | 416.4 | 800.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 91.7 | 484.1 | 871 |
| 1 | Code-heavy | cold-startup | 20 | 130.9 | 370.3 | 417.5 |
| 2 | Code-heavy | cold-startup | 20 | 123.6 | 352.7 | 675.5 |
| 0 | Code-heavy | cold-startup | 20 | 138.2 | 244.8 | 516.4 |
| D | Code-heavy | cold-startup | 20 | 155.2 | 493 | 577.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 127.4 | 269.6 | 332.2 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 143 | 496.1 | 611 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 131.2 | 586 | 715.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 133.1 | 380.8 | 518.9 |

## first.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 2.9 | 3.9 | 4.4 |
| 1 | Small | cold-startup | 20 | 2.6 | 4.8 | 4.9 |
| 0 | Small | cold-startup | 20 | 2.8 | 3.3 | 3.9 |
| 2 | Small | cold-startup | 20 | 2.7 | 3.7 | 3.9 |
| D | Small | fresh-profile-after-examples-settled | 20 | 2.8 | 3.8 | 3.9 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 2.5 | 4.4 | 4.5 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 2.6 | 3.7 | 4.7 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 2.7 | 3.5 | 5 |
| D | Overview | cold-startup | 20 | 3.4 | 4.9 | 5 |
| 0 | Overview | cold-startup | 20 | 3 | 4.7 | 9.3 |
| 2 | Overview | cold-startup | 20 | 3 | 4.4 | 5.7 |
| 1 | Overview | cold-startup | 20 | 3 | 4.1 | 4.3 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 3 | 4.6 | 4.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 2.7 | 4.5 | 5.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 2.7 | 4 | 4.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 2.8 | 5.1 | 5.5 |
| 1 | Code-heavy | cold-startup | 20 | 6.1 | 8.1 | 10.2 |
| 2 | Code-heavy | cold-startup | 20 | 5.6 | 7.3 | 8.6 |
| 0 | Code-heavy | cold-startup | 20 | 5.6 | 7.1 | 10.1 |
| D | Code-heavy | cold-startup | 20 | 6.2 | 8.5 | 32.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 5.8 | 8.3 | 10.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 5.8 | 7.1 | 22.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 5.9 | 7.2 | 7.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 6 | 8.4 | 9.9 |

## first.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 212493 | 212495 | 212495 |
| 1 | Small | cold-startup | 20 | 216165 | 216167 | 216167 |
| 0 | Small | cold-startup | 20 | 215772 | 215774 | 215774 |
| 2 | Small | cold-startup | 20 | 216165 | 216167 | 216167 |
| D | Small | fresh-profile-after-examples-settled | 20 | 212493 | 212495 | 212495 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 216165 | 216167 | 216167 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 215772 | 215774 | 215774 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 216165 | 216167 | 216167 |
| D | Overview | cold-startup | 20 | 247313 | 247315 | 247315 |
| 0 | Overview | cold-startup | 20 | 252475 | 252477 | 252477 |
| 2 | Overview | cold-startup | 20 | 1137623 | 1137625 | 1137625 |
| 1 | Overview | cold-startup | 20 | 336917 | 336919 | 336919 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 247313 | 247315 | 247315 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1137623 | 1137625 | 1137625 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 252475 | 252477 | 252477 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 336917 | 336919 | 336919 |
| 1 | Code-heavy | cold-startup | 20 | 344868 | 344870 | 344870 |
| 2 | Code-heavy | cold-startup | 20 | 4690142 | 4690144 | 4690144 |
| 0 | Code-heavy | cold-startup | 20 | 760221 | 760223 | 760223 |
| D | Code-heavy | cold-startup | 20 | 255264 | 255266 | 255266 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4690142 | 4690144 | 4690144 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 255264 | 255266 | 255266 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 760221 | 760223 | 760223 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 344868 | 344870 | 344870 |

## first.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 207393 | 207395 | 207395 |
| 1 | Small | cold-startup | 20 | 211065 | 211067 | 211067 |
| 0 | Small | cold-startup | 20 | 210672 | 210674 | 210674 |
| 2 | Small | cold-startup | 20 | 211065 | 211067 | 211067 |
| D | Small | fresh-profile-after-examples-settled | 20 | 207393 | 207395 | 207395 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 211065 | 211067 | 211067 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 210672 | 210674 | 210674 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 211065 | 211067 | 211067 |
| D | Overview | cold-startup | 20 | 242813 | 242815 | 242815 |
| 0 | Overview | cold-startup | 20 | 247675 | 247677 | 247677 |
| 2 | Overview | cold-startup | 20 | 1130123 | 1130125 | 1130125 |
| 1 | Overview | cold-startup | 20 | 328517 | 328519 | 328519 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 242813 | 242815 | 242815 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1130123 | 1130125 | 1130125 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 247675 | 247677 | 247677 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 328517 | 328519 | 328519 |
| 1 | Code-heavy | cold-startup | 20 | 335868 | 335870 | 335870 |
| 2 | Code-heavy | cold-startup | 20 | 4662842 | 4662844 | 4662844 |
| 0 | Code-heavy | cold-startup | 20 | 754821 | 754823 | 754823 |
| D | Code-heavy | cold-startup | 20 | 250164 | 250166 | 250166 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4662842 | 4662844 | 4662844 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 250164 | 250166 | 250166 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754821 | 754823 | 754823 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335868 | 335870 | 335870 |

## first.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.2 | 0.2 | 0.2 |
| 2 | Overview | cold-startup | 20 | 0.2 | 0.5 | 0.6 |
| 1 | Overview | cold-startup | 20 | 0.2 | 0.3 | 0.5 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.3 | 0.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.3 | 0.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.2 | 0.3 | 0.4 |
| 1 | Code-heavy | cold-startup | 20 | 15.3 | 16.999 | 21.001 |
| 2 | Code-heavy | cold-startup | 20 | 19.3 | 32.9 | 130.7 |
| 0 | Code-heavy | cold-startup | 20 | 13.199 | 14.4 | 14.4 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 19.3 | 25.1 | 128.2 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 12.9 | 14.1 | 15.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 15.2 | 16 | 131.1 |

## first.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.6 | 0.6 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Code-heavy | cold-startup | 20 | 4.399 | 4.8 | 5 |
| 2 | Code-heavy | cold-startup | 20 | 7.199 | 9.6 | 20.3 |
| 0 | Code-heavy | cold-startup | 20 | 5 | 5.101 | 5.201 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 6.901 | 9 | 13 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.8 | 5.8 | 6.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.3 | 4.7 | 6.4 |

## first.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 79 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 61 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 72 | 154 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 63 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 51 | 74 |
| 2 | Code-heavy | cold-startup | 20 | 118 | 244 | 300 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 129 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 87 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 94 | 190 | 212 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 77 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 74 | 155 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 77 |

## first.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 7401196 | 7413367 | 7426900 |
| 1 | Small | cold-startup | 20 | 7411693 | 7493909 | 7501990 |
| 0 | Small | cold-startup | 20 | 7408487 | 7451849 | 7505335 |
| 2 | Small | cold-startup | 20 | 7401928 | 7442749 | 7466880 |
| D | Small | fresh-profile-after-examples-settled | 20 | 7395101 | 7419229 | 7421288 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7408718 | 7424601 | 7436424 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7408887 | 7454815 | 7458424 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 7409124 | 7429326 | 7500288 |
| D | Overview | cold-startup | 20 | 7701644 | 7767134 | 7776498 |
| 0 | Overview | cold-startup | 20 | 7745325 | 7781102 | 7781358 |
| 2 | Overview | cold-startup | 20 | 7758685 | 7788837 | 7803808 |
| 1 | Overview | cold-startup | 20 | 7778659 | 7804003 | 7812374 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 7712923 | 7767738 | 7782333 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 7765696 | 7814558 | 7821360 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 7751063 | 7781206 | 7803236 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7773679 | 7811562 | 7824430 |
| 1 | Code-heavy | cold-startup | 20 | 8730572 | 9254460 | 9314309 |
| 2 | Code-heavy | cold-startup | 20 | 14745189 | 22585646 | 25641335 |
| 0 | Code-heavy | cold-startup | 20 | 9255668 | 9325053 | 16908565 |
| D | Code-heavy | cold-startup | 20 | 7891850 | 7909762 | 7929136 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 14544052 | 21946149 | 22542172 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 7891114 | 7924539 | 7946928 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 9254682 | 9328911 | 17441895 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8804626 | 9327566 | 9336235 |

## second.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 130.8 | 163.3 | 163.3 |
| 1 | Small | cold-startup | 20 | 146.2 | 180.4 | 346.7 |
| 0 | Small | cold-startup | 20 | 146.9 | 263.1 | 297.7 |
| 2 | Small | cold-startup | 20 | 130.5 | 163.9 | 230.3 |
| D | Small | fresh-profile-after-examples-settled | 20 | 147.2 | 197.1 | 351.6 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 147.1 | 213.6 | 232.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 146.9 | 227.1 | 376.4 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 145.6 | 246.3 | 324.3 |
| D | Overview | cold-startup | 20 | 149.4 | 325.2 | 390.9 |
| 0 | Overview | cold-startup | 20 | 152.4 | 303.7 | 319.3 |
| 2 | Overview | cold-startup | 20 | 135.8 | 191.2 | 355.4 |
| 1 | Overview | cold-startup | 20 | 168.4 | 201 | 206.1 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 155.1 | 263.6 | 384.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 135.8 | 219.1 | 535.7 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 136.7 | 304.1 | 402.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 153 | 217.2 | 368.3 |
| 1 | Code-heavy | cold-startup | 20 | 414.7 | 1091.2 | 1394.2 |
| 2 | Code-heavy | cold-startup | 19 | 545.3 | 1855.1 | 1855.1 |
| 0 | Code-heavy | cold-startup | 20 | 248.5 | 846 | 940.6 |
| D | Code-heavy | cold-startup | 20 | 139.3 | 269.7 | 342.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 541.4 | 747.5 | 974.5 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 153.6 | 310.6 | 318.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 346.3 | 1003.6 | 1193.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 447.5 | 1555.9 | 1768.3 |

## second.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 87 | 140 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 54 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 92 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 283 | 711 | 854 |
| 2 | Code-heavy | cold-startup | 19 | 602 | 1590 | 1590 |
| 0 | Code-heavy | cold-startup | 20 | 139 | 423 | 678 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 138 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 614 | 767 | 1008 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 53 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 138 | 481 | 658 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 283 | 838 | 963 |

## second.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 21.9 | 22.6 |
| 1 | Small | cold-startup | 20 | 5.6 | 38.2 | 206.8 |
| 0 | Small | cold-startup | 20 | 0 | 118.5 | 151.7 |
| 2 | Small | cold-startup | 20 | 0 | 17 | 94.7 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 51.9 | 209.7 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 5.5 | 66.4 | 88.4 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 5.7 | 82.7 | 231.3 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 100.5 | 180.5 |
| D | Overview | cold-startup | 20 | 0 | 139.8 | 182.8 |
| 0 | Overview | cold-startup | 20 | 0 | 161.9 | 187.9 |
| 2 | Overview | cold-startup | 20 | 0 | 7.8 | 214.1 |
| 1 | Overview | cold-startup | 20 | 10.7 | 32.6 | 63.9 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 10.5 | 115.8 | 238.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 74.8 | 393.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 118.5 | 261.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 10.6 | 50.2 | 226.8 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 326.1 | 939.1 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 392.8 | 392.8 |
| 0 | Code-heavy | cold-startup | 20 | 65.8 | 217.5 | 223.4 |
| D | Code-heavy | cold-startup | 20 | 0 | 128.2 | 201.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 116.6 | 142.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 5.6 | 171 | 176.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 53.6 | 231.9 | 239.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 18.2 | 478.6 | 690.5 |

## second.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 1.8 | 2.6 | 2.7 |
| 1 | Small | cold-startup | 20 | 1.8 | 3.8 | 4.5 |
| 0 | Small | cold-startup | 20 | 2 | 3.7 | 5.7 |
| 2 | Small | cold-startup | 20 | 2 | 2.9 | 7.3 |
| D | Small | fresh-profile-after-examples-settled | 20 | 2 | 2.9 | 6.3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 2.2 | 2.8 | 9.4 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 2.2 | 3.2 | 3.9 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 2 | 2.7 | 3.3 |
| D | Overview | cold-startup | 20 | 2.2 | 4.4 | 31.4 |
| 0 | Overview | cold-startup | 20 | 2.4 | 4 | 4.1 |
| 2 | Overview | cold-startup | 20 | 2.1 | 3.5 | 4.1 |
| 1 | Overview | cold-startup | 20 | 2.1 | 3 | 4.6 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 2.3 | 4.1 | 4.5 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 2.2 | 2.7 | 2.8 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 2.3 | 4.1 | 4.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 2.5 | 3.7 | 7.6 |
| 1 | Code-heavy | cold-startup | 20 | 4.2 | 5.3 | 5.8 |
| 2 | Code-heavy | cold-startup | 19 | 4.2 | 6.4 | 6.4 |
| 0 | Code-heavy | cold-startup | 20 | 4.9 | 8.3 | 10.3 |
| D | Code-heavy | cold-startup | 20 | 4 | 7.9 | 9.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.8 | 6 | 6.3 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.9 | 6.1 | 6.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.2 | 6 | 7.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.2 | 5.9 | 6.5 |

## second.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 85340 | 85342 | 85342 |
| 1 | Small | cold-startup | 20 | 174944 | 174946 | 174946 |
| 0 | Small | cold-startup | 20 | 89223 | 89225 | 89225 |
| 2 | Small | cold-startup | 20 | 664661 | 664663 | 664663 |
| D | Small | fresh-profile-after-examples-settled | 20 | 85340 | 85342 | 85342 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 174944 | 174946 | 174946 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 89223 | 89225 | 89225 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 664661 | 664663 | 664663 |
| D | Overview | cold-startup | 20 | 160376 | 160383 | 160383 |
| 0 | Overview | cold-startup | 20 | 163655 | 163662 | 163662 |
| 2 | Overview | cold-startup | 20 | 164048 | 164055 | 164055 |
| 1 | Overview | cold-startup | 20 | 164048 | 164055 | 164055 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 160376 | 160383 | 160383 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 164048 | 164055 | 164055 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 163655 | 163662 | 163662 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 164048 | 164055 | 164055 |
| 1 | Code-heavy | cold-startup | 20 | 217715 | 217717 | 217717 |
| 2 | Code-heavy | cold-startup | 19 | 4562991 | 4562991 | 4562991 |
| 0 | Code-heavy | cold-startup | 20 | 633068 | 633070 | 633070 |
| D | Code-heavy | cold-startup | 20 | 128111 | 128113 | 128113 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4562989 | 4562991 | 4562991 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 128111 | 128113 | 128113 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 633068 | 633070 | 633070 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 217715 | 217717 | 217717 |

## second.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 207399 | 207401 | 207401 |
| 1 | Small | cold-startup | 20 | 293103 | 293105 | 293105 |
| 0 | Small | cold-startup | 20 | 210982 | 210984 | 210984 |
| 2 | Small | cold-startup | 20 | 784320 | 784322 | 784322 |
| D | Small | fresh-profile-after-examples-settled | 20 | 207399 | 207401 | 207401 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 293103 | 293105 | 293105 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 210982 | 210984 | 210984 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 784320 | 784322 | 784322 |
| D | Overview | cold-startup | 20 | 229650 | 229657 | 229657 |
| 0 | Overview | cold-startup | 20 | 232929 | 232936 | 232936 |
| 2 | Overview | cold-startup | 20 | 233322 | 233329 | 233329 |
| 1 | Overview | cold-startup | 20 | 233322 | 233329 | 233329 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 229650 | 229657 | 229657 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 233322 | 233329 | 233329 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 232929 | 232936 | 232936 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 233322 | 233329 | 233329 |
| 1 | Code-heavy | cold-startup | 20 | 335874 | 335876 | 335876 |
| 2 | Code-heavy | cold-startup | 19 | 4662850 | 4662850 | 4662850 |
| 0 | Code-heavy | cold-startup | 20 | 754827 | 754829 | 754829 |
| D | Code-heavy | cold-startup | 20 | 250170 | 250172 | 250172 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4662848 | 4662850 | 4662850 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 250170 | 250172 | 250172 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754827 | 754829 | 754829 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335874 | 335876 | 335876 |

## second.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 12.3 | 17.3 | 20 |
| 2 | Code-heavy | cold-startup | 19 | 19.1 | 35.101 | 35.101 |
| 0 | Code-heavy | cold-startup | 20 | 12.1 | 15 | 16.6 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 18.7 | 19.7 | 32.5 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 10.9 | 12.6 | 12.901 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 12.1 | 24 | 110.9 |

## second.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Small | cold-startup | 20 | 0 | 0.1 | 0.2 |
| 2 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.1 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 3.901 | 5.2 | 8.7 |
| 2 | Code-heavy | cold-startup | 19 | 6.901 | 8.199 | 8.199 |
| 0 | Code-heavy | cold-startup | 20 | 4.4 | 5 | 5.199 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 6.7 | 7.3 | 8.6 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.4 | 5 | 5.001 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4 | 4.5 | 7.5 |

## second.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 87 | 140 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 54 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 92 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 221 | 577 | 711 |
| 2 | Code-heavy | cold-startup | 19 | 324 | 1046 | 1046 |
| 0 | Code-heavy | cold-startup | 20 | 139 | 230 | 548 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 138 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 318 | 422 | 475 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 53 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 137 | 406 | 503 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 223 | 838 | 870 |

## second.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 7418501 | 7444167 | 7477896 |
| 1 | Small | cold-startup | 20 | 7517677 | 7559264 | 7570015 |
| 0 | Small | cold-startup | 20 | 7484820 | 7891507 | 7891847 |
| 2 | Small | cold-startup | 20 | 7512918 | 7542712 | 7558312 |
| D | Small | fresh-profile-after-examples-settled | 20 | 7414014 | 7463452 | 7829957 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7514062 | 7888206 | 8056170 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7476702 | 7893075 | 7954265 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 7517771 | 7559148 | 7562764 |
| D | Overview | cold-startup | 20 | 7731806 | 7852962 | 8038002 |
| 0 | Overview | cold-startup | 20 | 7782220 | 7941129 | 7944500 |
| 2 | Overview | cold-startup | 20 | 7884891 | 7971890 | 7976859 |
| 1 | Overview | cold-startup | 20 | 7831472 | 7941673 | 7946482 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 7762575 | 7825849 | 8046349 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 7898411 | 7975647 | 8018685 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 7790205 | 7952174 | 7960822 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7865381 | 7977961 | 7985085 |
| 1 | Code-heavy | cold-startup | 20 | 14590443 | 24603507 | 24638022 |
| 2 | Code-heavy | cold-startup | 19 | 24319873 | 45195176 | 45195176 |
| 0 | Code-heavy | cold-startup | 20 | 22155544 | 30708227 | 30837335 |
| D | Code-heavy | cold-startup | 20 | 8448762 | 8589639 | 8595784 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21514913 | 44743150 | 45201376 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 8453463 | 8587965 | 8588579 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21883569 | 30812474 | 31020998 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 14718244 | 24554903 | 24597857 |

## retained.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 48.4 | 49.1 | 49.6 |
| 1 | Small | cold-startup | 20 | 48.7 | 54 | 55.3 |
| 0 | Small | cold-startup | 20 | 48.8 | 49.5 | 54.8 |
| 2 | Small | cold-startup | 20 | 48.7 | 58.4 | 68.5 |
| D | Small | fresh-profile-after-examples-settled | 20 | 48.8 | 54.2 | 96.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 48.5 | 53.4 | 54.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 48.7 | 54.9 | 66.3 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 48.6 | 57.8 | 61 |
| D | Overview | cold-startup | 20 | 48.9 | 106 | 115.8 |
| 0 | Overview | cold-startup | 20 | 48.4 | 49.7 | 53.7 |
| 2 | Overview | cold-startup | 20 | 48.1 | 56.7 | 57.1 |
| 1 | Overview | cold-startup | 20 | 48.7 | 57.1 | 132.2 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 48.8 | 54.2 | 107.8 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 48.4 | 54.2 | 65.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 48.3 | 49.7 | 50.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 48.9 | 81.6 | 89.9 |
| 1 | Code-heavy | cold-startup | 20 | 46 | 395 | 459.5 |
| 2 | Code-heavy | cold-startup | 19 | 45.5 | 344.9 | 344.9 |
| 0 | Code-heavy | cold-startup | 20 | 45.8 | 277.6 | 446.4 |
| D | Code-heavy | cold-startup | 20 | 46.2 | 47.4 | 54.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45 | 392.6 | 1191 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 46.8 | 53.5 | 143.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45.9 | 49.6 | 245.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45.8 | 303.1 | 571.3 |

## retained.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 64 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 66 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 258 | 428 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 219 | 219 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 249 | 378 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 350 | 1078 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 72 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 219 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 260 | 389 |

## retained.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 64 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 66 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 251 | 255 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 219 | 219 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 174 | 308 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 261 | 728 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 72 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 161 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 195 | 389 |

## retained.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 7722183 | 8468411 | 8469369 |
| 1 | Small | cold-startup | 20 | 7804105 | 8577722 | 8923954 |
| 0 | Small | cold-startup | 20 | 7773610 | 8532891 | 8700326 |
| 2 | Small | cold-startup | 20 | 7792559 | 8577605 | 8626511 |
| D | Small | fresh-profile-after-examples-settled | 20 | 8317464 | 8585076 | 8610712 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7803849 | 8597570 | 8600929 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7755801 | 8546381 | 8548198 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 7839572 | 8590675 | 8671443 |
| D | Overview | cold-startup | 20 | 8029913 | 8357563 | 8727230 |
| 0 | Overview | cold-startup | 20 | 8020415 | 8348433 | 8855284 |
| 2 | Overview | cold-startup | 20 | 8243095 | 9015928 | 9671459 |
| 1 | Overview | cold-startup | 20 | 8233702 | 8281786 | 8283385 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 7980986 | 8170013 | 8269307 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 8241913 | 8371610 | 8382017 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 8123838 | 8369119 | 8851093 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 8216826 | 8375486 | 8926332 |
| 1 | Code-heavy | cold-startup | 20 | 21779971 | 36715689 | 36738161 |
| 2 | Code-heavy | cold-startup | 19 | 34652578 | 67469355 | 67469355 |
| 0 | Code-heavy | cold-startup | 20 | 29392488 | 38228316 | 39769085 |
| D | Code-heavy | cold-startup | 20 | 8895611 | 8999775 | 9040528 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 35569886 | 53989422 | 66768844 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 8841869 | 9046128 | 9351800 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 25206901 | 42073676 | 49043024 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21254716 | 36282894 | 39239765 |

## evicted.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 132.3 | 165.1 | 196.3 |
| 1 | Small | cold-startup | 20 | 132 | 249.1 | 331 |
| 0 | Small | cold-startup | 20 | 148 | 180.6 | 248.5 |
| 2 | Small | cold-startup | 20 | 132.9 | 354.9 | 379.7 |
| D | Small | fresh-profile-after-examples-settled | 20 | 132 | 199.3 | 280.8 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 132 | 319.6 | 365.3 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 132.6 | 280.1 | 367 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 131.9 | 197.3 | 294.7 |
| D | Overview | cold-startup | 20 | 132.4 | 264.2 | 389.8 |
| 0 | Overview | cold-startup | 20 | 148.1 | 331.7 | 365.1 |
| 2 | Overview | cold-startup | 20 | 131.8 | 263.4 | 488.6 |
| 1 | Overview | cold-startup | 20 | 131.9 | 198.5 | 215.2 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 131.8 | 179.2 | 398.7 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 148 | 337.8 | 485.9 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 148 | 214.2 | 365.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 149.4 | 254.3 | 363.2 |
| 1 | Code-heavy | cold-startup | 20 | 149.7 | 255.4 | 309.1 |
| 2 | Code-heavy | cold-startup | 19 | 148.5 | 181.7 | 181.7 |
| 0 | Code-heavy | cold-startup | 20 | 148.6 | 315.1 | 780.1 |
| D | Code-heavy | cold-startup | 20 | 148.4 | 357.1 | 405.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 132.3 | 161.7 | 165.9 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 148.2 | 347.9 | 381.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 148.8 | 261.3 | 349.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 148.3 | 298.3 | 372.9 |

## evicted.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 70 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 69 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 68 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 64 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 75 | 76 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 107 | 206 | 557 |
| 2 | Code-heavy | cold-startup | 19 | 67 | 201 | 201 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 141 | 274 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 76 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 67 | 80 | 214 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 60 | 120 | 121 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 95 | 158 | 167 |

## evicted.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 29.3 | 65.4 |
| 1 | Small | cold-startup | 20 | 0 | 112.6 | 187.5 |
| 0 | Small | cold-startup | 20 | 0 | 33.1 | 104.5 |
| 2 | Small | cold-startup | 20 | 0 | 171.9 | 213 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 61.3 | 144.7 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 110.9 | 222.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 145.9 | 223.3 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 52.8 | 159.2 |
| D | Overview | cold-startup | 20 | 0 | 127.8 | 236.9 |
| 0 | Overview | cold-startup | 20 | 5.6 | 154.2 | 218.2 |
| 2 | Overview | cold-startup | 20 | 0 | 72.7 | 329.1 |
| 1 | Overview | cold-startup | 20 | 0 | 33.6 | 78.7 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 25.1 | 254.7 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 6.4 | 195.2 | 335.9 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 54.5 | 220.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 6.3 | 107.8 | 170.6 |
| 1 | Code-heavy | cold-startup | 20 | 5.9 | 67 | 148.7 |
| 2 | Code-heavy | cold-startup | 19 | 5.6 | 33.5 | 33.5 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 121.7 | 646.6 |
| D | Code-heavy | cold-startup | 20 | 5.6 | 168.7 | 227.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 10.9 | 16.4 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 5.4 | 215.1 | 237.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 115.6 | 203.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 133.7 | 228.7 |

## evicted.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 1.7 | 2.9 | 11.9 |
| 1 | Small | cold-startup | 20 | 1.8 | 2.6 | 2.9 |
| 0 | Small | cold-startup | 20 | 1.9 | 2.5 | 2.7 |
| 2 | Small | cold-startup | 20 | 1.7 | 3.1 | 3.2 |
| D | Small | fresh-profile-after-examples-settled | 20 | 1.7 | 2.8 | 3.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1.7 | 2.7 | 4.5 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1.8 | 2.3 | 2.3 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 1.8 | 2.7 | 2.8 |
| D | Overview | cold-startup | 20 | 1.9 | 3.1 | 5.4 |
| 0 | Overview | cold-startup | 20 | 2 | 2.6 | 3.1 |
| 2 | Overview | cold-startup | 20 | 1.8 | 2.7 | 3.6 |
| 1 | Overview | cold-startup | 20 | 2 | 4.5 | 4.6 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 2.2 | 2.9 | 3.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 2.1 | 3.1 | 3.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 2.1 | 2.8 | 3.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 2.2 | 4.2 | 85.4 |
| 1 | Code-heavy | cold-startup | 20 | 4.2 | 5.8 | 6.2 |
| 2 | Code-heavy | cold-startup | 19 | 3.9 | 8.9 | 8.9 |
| 0 | Code-heavy | cold-startup | 20 | 3.6 | 6.2 | 6.4 |
| D | Code-heavy | cold-startup | 20 | 3.6 | 5 | 5.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.7 | 5.8 | 8.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.7 | 5.9 | 8.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.3 | 6.9 | 10.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.6 | 7.4 | 12.8 |

## evicted.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 82036 | 82036 | 82036 |
| 1 | Small | cold-startup | 20 | 85708 | 85708 | 85708 |
| 0 | Small | cold-startup | 20 | 85315 | 85315 | 85315 |
| 2 | Small | cold-startup | 20 | 85708 | 85708 | 85708 |
| D | Small | fresh-profile-after-examples-settled | 20 | 82036 | 82036 | 82036 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 85708 | 85708 | 85708 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 85315 | 85315 | 85315 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 85708 | 85708 | 85708 |
| D | Overview | cold-startup | 20 | 150645 | 150645 | 150645 |
| 0 | Overview | cold-startup | 20 | 155807 | 155807 | 155807 |
| 2 | Overview | cold-startup | 20 | 1040955 | 1040955 | 1040955 |
| 1 | Overview | cold-startup | 20 | 240249 | 240249 | 240249 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 150645 | 150645 | 150645 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1040955 | 1040955 | 1040955 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 155807 | 155807 | 155807 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 240249 | 240249 | 240249 |
| 1 | Code-heavy | cold-startup | 20 | 171640 | 171640 | 171640 |
| 2 | Code-heavy | cold-startup | 19 | 4516914 | 4516914 | 4516914 |
| 0 | Code-heavy | cold-startup | 20 | 586993 | 586993 | 586993 |
| D | Code-heavy | cold-startup | 20 | 82036 | 82036 | 82036 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4516914 | 4516914 | 4516914 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 82036 | 82036 | 82036 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 586993 | 586993 | 586993 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 171640 | 171640 | 171640 |

## evicted.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 207393 | 207395 | 207395 |
| 1 | Small | cold-startup | 20 | 211065 | 211067 | 211067 |
| 0 | Small | cold-startup | 20 | 210672 | 210674 | 210674 |
| 2 | Small | cold-startup | 20 | 211065 | 211067 | 211067 |
| D | Small | fresh-profile-after-examples-settled | 20 | 207393 | 207395 | 207395 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 211065 | 211067 | 211067 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 210672 | 210674 | 210674 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 211065 | 211067 | 211067 |
| D | Overview | cold-startup | 20 | 242813 | 242815 | 242815 |
| 0 | Overview | cold-startup | 20 | 247675 | 247677 | 247677 |
| 2 | Overview | cold-startup | 20 | 1130123 | 1130125 | 1130125 |
| 1 | Overview | cold-startup | 20 | 328517 | 328519 | 328519 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 242813 | 242815 | 242815 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1130123 | 1130125 | 1130125 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 247675 | 247677 | 247677 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 328517 | 328519 | 328519 |
| 1 | Code-heavy | cold-startup | 20 | 335868 | 335870 | 335870 |
| 2 | Code-heavy | cold-startup | 19 | 4662844 | 4662844 | 4662844 |
| 0 | Code-heavy | cold-startup | 20 | 754821 | 754823 | 754823 |
| D | Code-heavy | cold-startup | 20 | 250164 | 250166 | 250166 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4662842 | 4662844 | 4662844 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 250164 | 250166 | 250166 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754821 | 754823 | 754823 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335868 | 335870 | 335870 |

## evicted.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Overview | cold-startup | 20 | 0.2 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.4 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.3 | 0.6 |
| 1 | Code-heavy | cold-startup | 20 | 12.2 | 18.8 | 21.5 |
| 2 | Code-heavy | cold-startup | 19 | 15 | 68.401 | 68.401 |
| 0 | Code-heavy | cold-startup | 20 | 10.2 | 12.2 | 12.9 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 15.2 | 16.899 | 36.5 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 10.3 | 12 | 12.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 12.4 | 15.199 | 27.4 |

## evicted.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| 2 | Overview | cold-startup | 20 | 0 | 0.2 | 0.3 |
| 1 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.2 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Code-heavy | cold-startup | 20 | 4.399 | 5.801 | 10 |
| 2 | Code-heavy | cold-startup | 19 | 6.6 | 8.7 | 8.7 |
| 0 | Code-heavy | cold-startup | 20 | 4.5 | 4.9 | 5.6 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 6.7 | 7.1 | 7.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.6 | 5.801 | 7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.2 | 5.201 | 8.899 |

## evicted.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 70 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 69 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 68 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 64 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 75 | 76 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 107 | 206 | 493 |
| 2 | Code-heavy | cold-startup | 19 | 67 | 201 | 201 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 141 | 274 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 76 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 67 | 80 | 214 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 60 | 120 | 121 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 94 | 158 | 167 |

## evicted.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 24657703 | 25096643 | 25102657 |
| 1 | Small | cold-startup | 20 | 11612969 | 25244516 | 25270352 |
| 0 | Small | cold-startup | 20 | 24754824 | 25217033 | 25229713 |
| 2 | Small | cold-startup | 20 | 24784362 | 25560876 | 25873918 |
| D | Small | fresh-profile-after-examples-settled | 20 | 24615999 | 25129544 | 25143989 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 24841279 | 25447298 | 25859628 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 24711518 | 25226796 | 25276521 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 24829188 | 25158781 | 25253978 |
| D | Overview | cold-startup | 20 | 25236742 | 25607417 | 25627533 |
| 0 | Overview | cold-startup | 20 | 25523639 | 25776869 | 25902062 |
| 2 | Overview | cold-startup | 20 | 25596440 | 26094635 | 26241528 |
| 1 | Overview | cold-startup | 20 | 25546466 | 26172677 | 27001820 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 25134295 | 25480922 | 25777772 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 25482727 | 26172571 | 26746891 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 14459768 | 25493235 | 26034555 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 14770692 | 26085027 | 26194551 |
| 1 | Code-heavy | cold-startup | 20 | 31740725 | 71593636 | 85978730 |
| 2 | Code-heavy | cold-startup | 19 | 43706536 | 67536803 | 67536803 |
| 0 | Code-heavy | cold-startup | 20 | 27268027 | 53202396 | 83217282 |
| D | Code-heavy | cold-startup | 20 | 12631969 | 13705683 | 13908145 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 32216940 | 60351070 | 77857257 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 12414462 | 14548721 | 17556320 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 29084362 | 48007879 | 51933935 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 23932228 | 47927300 | 77033777 |

## sourceFirst.readableMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 14.2 | 14.9 | 31.5 |
| 1 | Small | cold-startup | 20 | 14.1 | 14.9 | 14.9 |
| 0 | Small | cold-startup | 20 | 14.2 | 14.9 | 15.1 |
| 2 | Small | cold-startup | 20 | 14.4 | 18.6 | 22.9 |
| D | Small | fresh-profile-after-examples-settled | 20 | 14.4 | 14.6 | 14.7 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 14.2 | 15.2 | 25.4 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 14.3 | 15.2 | 15.3 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 14 | 15.2 | 27.7 |
| D | Overview | cold-startup | 20 | 13.8 | 20.1 | 27.5 |
| 0 | Overview | cold-startup | 20 | 14.2 | 15 | 15 |
| 2 | Overview | cold-startup | 20 | 12.5 | 21.4 | 24.1 |
| 1 | Overview | cold-startup | 20 | 11.9 | 21.3 | 23.2 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 14.3 | 15.3 | 45.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 12.6 | 32.3 | 60.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 14.4 | 15.3 | 23.8 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 12.1 | 22.6 | 25.2 |
| 1 | Code-heavy | cold-startup | 20 | 20.7 | 31.2 | 36.3 |
| 2 | Code-heavy | cold-startup | 20 | 26.9 | 51.3 | 370 |
| 0 | Code-heavy | cold-startup | 20 | 18.9 | 32.3 | 43.8 |
| D | Code-heavy | cold-startup | 20 | 10.7 | 23.9 | 24.8 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 22.8 | 355.1 | 450.2 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 9.5 | 24.2 | 27.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 18.3 | 26.5 | 47.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21 | 48.6 | 169.1 |

## sourceFirst.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.3 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.2 | 0.3 | 0.7 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.4 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.8 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Code-heavy | cold-startup | 20 | 2.2 | 2.4 | 2.6 |
| 2 | Code-heavy | cold-startup | 20 | 2.9 | 3.8 | 5.2 |
| 0 | Code-heavy | cold-startup | 20 | 2.1 | 2.8 | 2.9 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3 | 4.2 | 5.6 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2.1 | 4.7 | 5.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2.1 | 2.3 | 2.4 |

## sourceFirst.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 1.2 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.1 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0.1 | 0.5 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0 | 0.1 | 0.2 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.2 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 1 | Code-heavy | cold-startup | 20 | 0.6 | 0.9 | 1.1 |
| 2 | Code-heavy | cold-startup | 20 | 1 | 1.8 | 2.4 |
| 0 | Code-heavy | cold-startup | 20 | 0.6 | 0.7 | 1.1 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1 | 1.8 | 1.9 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.6 | 1.9 | 53.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.6 | 0.7 | 0.8 |

## sourceFirst.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 184 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 283 | 857 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 162 | 181 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 566 | 895 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 166 | 264 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 144 |

## sourceFirst.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 638 | 638 | 638 |
| 1 | Small | cold-startup | 20 | 87208 | 87208 | 87208 |
| 0 | Small | cold-startup | 20 | 1540 | 1540 | 1540 |
| 2 | Small | cold-startup | 20 | 766980 | 766980 | 766980 |
| D | Small | fresh-profile-after-examples-settled | 20 | 638 | 638 | 638 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 87208 | 87208 | 87208 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1540 | 1540 | 1540 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 766980 | 766980 | 766980 |
| D | Overview | cold-startup | 20 | 2224 | 2224 | 2224 |
| 0 | Overview | cold-startup | 20 | 4697 | 4697 | 4697 |
| 2 | Overview | cold-startup | 20 | 198727 | 198727 | 198727 |
| 1 | Overview | cold-startup | 20 | 4448 | 4448 | 4448 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 2224 | 2224 | 2224 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 198727 | 198727 | 198727 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 4697 | 4697 | 4697 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 4448 | 4448 | 4448 |
| 1 | Code-heavy | cold-startup | 20 | 200228 | 200228 | 200228 |
| 2 | Code-heavy | cold-startup | 20 | 200228 | 200228 | 200228 |
| 0 | Code-heavy | cold-startup | 20 | 236384 | 236384 | 236384 |
| D | Code-heavy | cold-startup | 20 | 100114 | 100114 | 100114 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 200228 | 200228 | 200228 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 100114 | 100114 | 100114 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 236384 | 236384 | 236384 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 200228 | 200228 | 200228 |

## sourceFirst.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 338 | 338 | 338 |
| 1 | Small | cold-startup | 20 | 82708 | 82708 | 82708 |
| 0 | Small | cold-startup | 20 | 940 | 940 | 940 |
| 2 | Small | cold-startup | 20 | 763980 | 763980 | 763980 |
| D | Small | fresh-profile-after-examples-settled | 20 | 338 | 338 | 338 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 82708 | 82708 | 82708 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 940 | 940 | 940 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 763980 | 763980 | 763980 |
| D | Overview | cold-startup | 20 | 1924 | 1924 | 1924 |
| 0 | Overview | cold-startup | 20 | 4097 | 4097 | 4097 |
| 2 | Overview | cold-startup | 20 | 197827 | 197827 | 197827 |
| 1 | Overview | cold-startup | 20 | 3848 | 3848 | 3848 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1924 | 1924 | 1924 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 197827 | 197827 | 197827 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 4097 | 4097 | 4097 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 3848 | 3848 | 3848 |
| 1 | Code-heavy | cold-startup | 20 | 199628 | 199628 | 199628 |
| 2 | Code-heavy | cold-startup | 20 | 199628 | 199628 | 199628 |
| 0 | Code-heavy | cold-startup | 20 | 235784 | 235784 | 235784 |
| D | Code-heavy | cold-startup | 20 | 99814 | 99814 | 99814 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 199628 | 199628 | 199628 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 99814 | 99814 | 99814 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 235784 | 235784 | 235784 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 199628 | 199628 | 199628 |

## sourceReopen.readableMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 15.6 | 16.4 | 16.5 |
| 1 | Small | cold-startup | 20 | 15.2 | 16.1 | 16.1 |
| 0 | Small | cold-startup | 20 | 15.5 | 15.8 | 15.9 |
| 2 | Small | cold-startup | 20 | 15.4 | 16 | 16.3 |
| D | Small | fresh-profile-after-examples-settled | 20 | 15.5 | 16.1 | 16.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 15.3 | 16.1 | 16.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 15.5 | 16.5 | 16.7 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 15.3 | 16 | 16.2 |
| D | Overview | cold-startup | 20 | 14.7 | 15.1 | 15.2 |
| 0 | Overview | cold-startup | 20 | 15.2 | 16.2 | 16.3 |
| 2 | Overview | cold-startup | 20 | 15.1 | 15.6 | 15.7 |
| 1 | Overview | cold-startup | 20 | 15.1 | 15.6 | 46.3 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 14.9 | 15.3 | 16.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 15.2 | 15.9 | 16.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 15.4 | 16.3 | 16.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 15.1 | 15.8 | 16 |
| 1 | Code-heavy | cold-startup | 20 | 13.8 | 14.9 | 15.4 |
| 2 | Code-heavy | cold-startup | 20 | 12.2 | 14 | 20.4 |
| 0 | Code-heavy | cold-startup | 20 | 13.6 | 14.4 | 14.8 |
| D | Code-heavy | cold-startup | 20 | 14.2 | 15.3 | 15.4 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 12 | 13.2 | 13.4 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 14.3 | 15.2 | 15.3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 13.6 | 14.5 | 265.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 13.8 | 14.4 | 14.4 |

## sourceReopen.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 203 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## edit.fence.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 45.027 | 53.179 | 74.043 |
| 1 | Small | cold-startup | 20 | 45.396 | 61.965 | 129.389 |
| 0 | Small | cold-startup | 20 | 45.163 | 51.216 | 54.173 |
| 2 | Small | cold-startup | 20 | 45.197 | 74.291 | 75.323 |
| D | Small | fresh-profile-after-examples-settled | 20 | 45.262 | 51.475 | 65.998 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 45.305 | 78.136 | 98.533 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 45.414 | 74.578 | 301.453 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 45.22 | 54.454 | 62.365 |
| D | Overview | cold-startup | 20 | 46.085 | 68.927 | 75.003 |
| 0 | Overview | cold-startup | 20 | 45.799 | 67.813 | 93.868 |
| 2 | Overview | cold-startup | 20 | 47.134 | 63.151 | 85.847 |
| 1 | Overview | cold-startup | 20 | 44.819 | 51.22 | 56.962 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 45.435 | 64.151 | 82.434 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 46.448 | 73.401 | 296.32 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 46.15 | 49.445 | 114.267 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 49.053 | 70.17 | 76.057 |
| 1 | Code-heavy | cold-startup | 20 | 77.508 | 590.714 | 1565.029 |
| 2 | Code-heavy | cold-startup | 19 | 836.265 | 4587.402 | 4587.402 |
| 0 | Code-heavy | cold-startup | 20 | 68.802 | 371.852 | 549.991 |
| D | Code-heavy | cold-startup | 20 | 47.51 | 83.561 | 134.56 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 108.537 | 1069.206 | 1151.456 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 46.817 | 73.068 | 122.472 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 78.737 | 734.846 | 1435.674 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 68.006 | 489.919 | 901.256 |

## edit.example.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 178.521 | 417.236 | 618.015 |
| 1 | Small | cold-startup | 20 | 180.301 | 1024.181 | 1181.187 |
| 0 | Small | cold-startup | 20 | 173.816 | 545.31 | 622.854 |
| 2 | Small | cold-startup | 20 | 181.186 | 585.727 | 974.507 |
| D | Small | fresh-profile-after-examples-settled | 20 | 173.598 | 685.454 | 1202.774 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 187.223 | 727.699 | 819.997 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 184.096 | 843.893 | 1171.397 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 180.927 | 853.678 | 1052.086 |
| D | Overview | cold-startup | 20 | 115.211 | 326.938 | 401.527 |
| 0 | Overview | cold-startup | 20 | 125.66 | 491.687 | 571.79 |
| 2 | Overview | cold-startup | 20 | 125.319 | 245.591 | 465.498 |
| 1 | Overview | cold-startup | 20 | 111.862 | 358.077 | 365.611 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 117.894 | 675.165 | 842.977 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 118.524 | 612.959 | 768.952 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 126.721 | 366.981 | 401.22 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 131.494 | 684.902 | 737.875 |
| 1 | Code-heavy | cold-startup | 20 | 249.645 | 1390.026 | 1984.458 |
| 2 | Code-heavy | cold-startup | 19 | 247.608 | 1145.01 | 1145.01 |
| 0 | Code-heavy | cold-startup | 20 | 227.473 | 791.158 | 833.15 |
| D | Code-heavy | cold-startup | 20 | 192.128 | 666.867 | 958.851 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 230.689 | 429.559 | 1130.707 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 198.934 | 1122.484 | 1589.025 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 242.806 | 1047.66 | 1098.704 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 319.189 | 1408.519 | 1488.361 |

## edit.shared.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 109.666 | 217.428 | 465.899 |
| 1 | Small | cold-startup | 20 | 105.468 | 599.7 | 619.847 |
| 0 | Small | cold-startup | 20 | 110.213 | 473.069 | 700.343 |
| 2 | Small | cold-startup | 20 | 111.095 | 510.287 | 810.696 |
| D | Small | fresh-profile-after-examples-settled | 20 | 108.198 | 546.412 | 917.112 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 109.854 | 523.572 | 581.612 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 107.916 | 600.906 | 624.794 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 110.283 | 357.367 | 803.459 |
| D | Overview | cold-startup | 20 | 112.134 | 231.154 | 705.89 |
| 0 | Overview | cold-startup | 20 | 129.838 | 461.874 | 587.944 |
| 2 | Overview | cold-startup | 20 | 146.513 | 429.123 | 691.324 |
| 1 | Overview | cold-startup | 20 | 131.531 | 230.069 | 696.648 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 116.795 | 681.486 | 691.679 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 127.719 | 511.703 | 949.337 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 118.405 | 538.035 | 651.473 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 158.341 | 551.448 | 971.702 |
| 1 | Code-heavy | cold-startup | 20 | 172.999 | 1570.962 | 1999.435 |
| 2 | Code-heavy | cold-startup | 19 | 163.281 | 784.717 | 784.717 |
| 0 | Code-heavy | cold-startup | 20 | 152.74 | 627.477 | 717.694 |
| D | Code-heavy | cold-startup | 20 | 127.274 | 359.395 | 417.782 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 159.262 | 287.263 | 310.015 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 126.693 | 677.604 | 735.556 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 154.411 | 920.044 | 1266.802 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 163.432 | 610.477 | 695.239 |

## memory.peakServerRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 455868416 | 464080896 | 469180416 |
| 1 | Small | cold-startup | 20 | 456380416 | 466743296 | 471285760 |
| 0 | Small | cold-startup | 20 | 462651392 | 474378240 | 477573120 |
| 2 | Small | cold-startup | 20 | 460140544 | 471584768 | 476106752 |
| D | Small | fresh-profile-after-examples-settled | 20 | 456450048 | 473608192 | 477904896 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 455598080 | 467595264 | 474488832 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 463122432 | 476983296 | 479604736 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 458600448 | 469037056 | 479670272 |
| D | Overview | cold-startup | 20 | 480489472 | 487809024 | 491634688 |
| 0 | Overview | cold-startup | 20 | 487964672 | 504528896 | 507363328 |
| 2 | Overview | cold-startup | 20 | 488763392 | 508579840 | 509091840 |
| 1 | Overview | cold-startup | 20 | 478801920 | 500137984 | 510078976 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 479641600 | 485740544 | 499789824 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 487870464 | 508223488 | 509276160 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 483315712 | 506093568 | 511422464 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 482742272 | 492699648 | 504307712 |
| 1 | Code-heavy | cold-startup | 20 | 917987328 | 966197248 | 1050562560 |
| 2 | Code-heavy | cold-startup | 19 | 1123483648 | 1292513280 | 1292513280 |
| 0 | Code-heavy | cold-startup | 20 | 929517568 | 991162368 | 1000103936 |
| D | Code-heavy | cold-startup | 20 | 496812032 | 512737280 | 516481024 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1094471680 | 1199738880 | 1247911936 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 494432256 | 510160896 | 524365824 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 936898560 | 1003405312 | 1012572160 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 939184128 | 983461888 | 989708288 |

## memory.peakBrowserRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 993136640 | 1022234624 | 1023119360 |
| 1 | Small | cold-startup | 20 | 988676096 | 1035337728 | 1054146560 |
| 0 | Small | cold-startup | 20 | 996196352 | 1026768896 | 1050386432 |
| 2 | Small | cold-startup | 20 | 1001783296 | 1069498368 | 1078824960 |
| D | Small | fresh-profile-after-examples-settled | 20 | 989822976 | 1050894336 | 1055674368 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1000738816 | 1043509248 | 1049587712 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 997408768 | 1057054720 | 1081131008 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 1014087680 | 1062150144 | 1095450624 |
| D | Overview | cold-startup | 20 | 1015345152 | 1050615808 | 1052499968 |
| 0 | Overview | cold-startup | 20 | 1015898112 | 1052467200 | 1083678720 |
| 2 | Overview | cold-startup | 20 | 1045721088 | 1076461568 | 1089531904 |
| 1 | Overview | cold-startup | 20 | 1019658240 | 1061539840 | 1082060800 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1003540480 | 1056567296 | 1064345600 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1038544896 | 1063247872 | 1087479808 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1005092864 | 1073414144 | 1079693312 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1013338112 | 1056579584 | 1075650560 |
| 1 | Code-heavy | cold-startup | 20 | 1238183936 | 1294053376 | 1296297984 |
| 2 | Code-heavy | cold-startup | 19 | 1450688512 | 1514835968 | 1514835968 |
| 0 | Code-heavy | cold-startup | 20 | 1122963456 | 1241522176 | 1254035456 |
| D | Code-heavy | cold-startup | 20 | 1017737216 | 1059766272 | 1060634624 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1450115072 | 1509789696 | 1524211712 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 1021001728 | 1073217536 | 1089363968 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1131773952 | 1220288512 | 1255223296 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1253429248 | 1302126592 | 1378099200 |

## memory.settledServerRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 453718016 | 459173888 | 469180416 |
| 1 | Small | cold-startup | 20 | 455237632 | 466743296 | 471285760 |
| 0 | Small | cold-startup | 20 | 462077952 | 474050560 | 474378240 |
| 2 | Small | cold-startup | 20 | 458117120 | 469753856 | 469753856 |
| D | Small | fresh-profile-after-examples-settled | 20 | 453349376 | 469004288 | 469782528 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 455245824 | 467554304 | 474488832 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 462942208 | 472944640 | 479604736 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 456699904 | 469037056 | 479670272 |
| D | Overview | cold-startup | 20 | 479600640 | 487809024 | 491634688 |
| 0 | Overview | cold-startup | 20 | 487038976 | 500011008 | 507363328 |
| 2 | Overview | cold-startup | 20 | 486989824 | 508579840 | 509091840 |
| 1 | Overview | cold-startup | 20 | 477601792 | 500137984 | 504930304 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 478539776 | 484167680 | 499789824 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 486547456 | 506904576 | 509276160 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 483250176 | 500031488 | 506093568 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 482742272 | 492699648 | 504307712 |
| 1 | Code-heavy | cold-startup | 20 | 915460096 | 966197248 | 1025507328 |
| 2 | Code-heavy | cold-startup | 19 | 1095610368 | 1223413760 | 1223413760 |
| 0 | Code-heavy | cold-startup | 20 | 920375296 | 963493888 | 985030656 |
| D | Code-heavy | cold-startup | 20 | 494825472 | 512737280 | 516481024 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1060151296 | 1166995456 | 1247911936 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 493858816 | 510160896 | 524365824 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 929820672 | 992694272 | 1003405312 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 932638720 | 976150528 | 989708288 |

## memory.settledBrowserRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 936566784 | 943980544 | 944353280 |
| 1 | Small | cold-startup | 20 | 934920192 | 945958912 | 1054146560 |
| 0 | Small | cold-startup | 20 | 933904384 | 940994560 | 943640576 |
| 2 | Small | cold-startup | 20 | 957083648 | 965300224 | 965754880 |
| D | Small | fresh-profile-after-examples-settled | 20 | 934641664 | 1048543232 | 1055674368 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 938688512 | 945008640 | 947154944 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 936054784 | 1050157056 | 1081131008 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 959406080 | 965423104 | 972337152 |
| D | Overview | cold-startup | 20 | 964493312 | 974086144 | 977022976 |
| 0 | Overview | cold-startup | 20 | 966991872 | 973299712 | 1021804544 |
| 2 | Overview | cold-startup | 20 | 999514112 | 1007656960 | 1008062464 |
| 1 | Overview | cold-startup | 20 | 971444224 | 978808832 | 983076864 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 963780608 | 971919360 | 1023762432 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 995741696 | 1007702016 | 1010270208 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 939458560 | 990830592 | 1073414144 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 948928512 | 980017152 | 1019985920 |
| 1 | Code-heavy | cold-startup | 20 | 1209696256 | 1257947136 | 1296297984 |
| 2 | Code-heavy | cold-startup | 19 | 1365184512 | 1417949184 | 1417949184 |
| 0 | Code-heavy | cold-startup | 20 | 1112735744 | 1209061376 | 1241522176 |
| D | Code-heavy | cold-startup | 20 | 971853824 | 982573056 | 982614016 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1365921792 | 1416019968 | 1455468544 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 971763712 | 1053007872 | 1059909632 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1115185152 | 1162043392 | 1216454656 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1220296704 | 1292742656 | 1367293952 |

## memory.settledCombinedRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 1389690880 | 1400635392 | 1407328256 |
| 1 | Small | cold-startup | 20 | 1390526464 | 1408573440 | 1525432320 |
| 0 | Small | cold-startup | 20 | 1394769920 | 1413545984 | 1415372800 |
| 2 | Small | cold-startup | 20 | 1414430720 | 1428705280 | 1435508736 |
| D | Small | fresh-profile-after-examples-settled | 20 | 1388298240 | 1518325760 | 1518563328 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1392218112 | 1404923904 | 1412554752 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1395134464 | 1523101696 | 1560735744 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 1416921088 | 1429053440 | 1435758592 |
| D | Overview | cold-startup | 20 | 1444192256 | 1460350976 | 1464832000 |
| 0 | Overview | cold-startup | 20 | 1450000384 | 1468370944 | 1529167872 |
| 2 | Overview | cold-startup | 20 | 1484132352 | 1497346048 | 1497464832 |
| 1 | Overview | cold-startup | 20 | 1447170048 | 1476874240 | 1482686464 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1442684928 | 1454059520 | 1523552256 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1480089600 | 1502646272 | 1516888064 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1427996672 | 1490862080 | 1563750400 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1435697152 | 1470636032 | 1524293632 |
| 1 | Code-heavy | cold-startup | 20 | 2130141184 | 2182258688 | 2187247616 |
| 2 | Code-heavy | cold-startup | 19 | 2466422784 | 2590449664 | 2590449664 |
| 0 | Code-heavy | cold-startup | 20 | 2039177216 | 2107744256 | 2131476480 |
| D | Code-heavy | cold-startup | 20 | 1463578624 | 1489584128 | 1490149376 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2429296640 | 2540904448 | 2549940224 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 1465831424 | 1559527424 | 1584275456 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2041729024 | 2125160448 | 2164457472 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2134470656 | 2251005952 | 2337456128 |

## cpu.ownedProcessesSeconds

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 16.85 | 18.35 | 18.68 |
| 1 | Small | cold-startup | 20 | 17.11 | 22.88 | 23.6 |
| 0 | Small | cold-startup | 20 | 17.41 | 18.95 | 21.37 |
| 2 | Small | cold-startup | 20 | 17.13 | 20.35 | 23.84 |
| D | Small | fresh-profile-after-examples-settled | 20 | 16.6 | 18.86 | 21.69 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 17.22 | 21.27 | 22.6 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 16.75 | 22.54 | 23.73 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 17.59 | 20.74 | 25.06 |
| D | Overview | cold-startup | 20 | 17.81 | 19.35 | 22.77 |
| 0 | Overview | cold-startup | 20 | 17.78 | 20.8 | 20.93 |
| 2 | Overview | cold-startup | 20 | 19.08 | 21.62 | 27.38 |
| 1 | Overview | cold-startup | 20 | 17.88 | 20.31 | 20.51 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 17.46 | 19.15 | 23.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 19.28 | 23.47 | 26.61 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 17.34 | 21.6 | 21.79 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 18.52 | 24.56 | 25.61 |
| 1 | Code-heavy | cold-startup | 20 | 25.62 | 33.22 | 36.18 |
| 2 | Code-heavy | cold-startup | 19 | 41.12 | 56.93 | 56.93 |
| 0 | Code-heavy | cold-startup | 20 | 26.28 | 34.18 | 34.47 |
| D | Code-heavy | cold-startup | 20 | 17.16 | 21.63 | 22.29 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 40.37 | 47.85 | 53.89 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 17.24 | 21.16 | 23.25 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 26.39 | 34.89 | 36.13 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 25.27 | 35.18 | 35.95 |

## memory.workerHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| D | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 2067284 | 2077168 | 2089972 |
| 1 | Overview | cold-startup | 20 | 1100820 | 1101212 | 1101700 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 2064820 | 2086904 | 2108972 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1100776 | 1101128 | 1101468 |
| 1 | Code-heavy | cold-startup | 20 | 8189916 | 8207596 | 8212512 |
| 2 | Code-heavy | cold-startup | 19 | 21364368 | 37861800 | 37861800 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21364540 | 21801032 | 21837064 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8189752 | 8220704 | 8244080 |

## cache.compilerBytes

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 383707 | 384350 | 384353 |
| 1 | Small | cold-startup | 20 | 383716 | 384364 | 384366 |
| 0 | Small | cold-startup | 20 | 383714 | 384366 | 384368 |
| 2 | Small | cold-startup | 20 | 383716 | 384366 | 384367 |
| D | Small | fresh-profile-after-examples-settled | 20 | 383738 | 384357 | 384361 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 383717 | 384339 | 384342 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 383714 | 384335 | 384337 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 383721 | 384340 | 384341 |
| D | Overview | cold-startup | 20 | 507995 | 508864 | 508865 |
| 0 | Overview | cold-startup | 20 | 507983 | 508845 | 508847 |
| 2 | Overview | cold-startup | 20 | 507983 | 508847 | 508847 |
| 1 | Overview | cold-startup | 20 | 507983 | 508847 | 508847 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 507977 | 508869 | 508874 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 507987 | 508868 | 508871 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 507987 | 508868 | 508871 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 507986 | 508869 | 508871 |
| 1 | Code-heavy | cold-startup | 20 | 466546 | 467124 | 467126 |
| 2 | Code-heavy | cold-startup | 19 | 467110 | 467125 | 467125 |
| 0 | Code-heavy | cold-startup | 20 | 466542 | 467124 | 467125 |
| D | Code-heavy | cold-startup | 20 | 466563 | 467111 | 467112 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 466571 | 467116 | 467118 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 466557 | 467138 | 467138 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 466569 | 467116 | 467119 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 466572 | 467117 | 467118 |

## cache.indexBytes

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| D | Small | cold-startup | 20 | 467 | 467 | 467 |
| 1 | Small | cold-startup | 20 | 467 | 467 | 467 |
| 0 | Small | cold-startup | 20 | 467 | 467 | 467 |
| 2 | Small | cold-startup | 20 | 467 | 467 | 467 |
| D | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| D | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| 0 | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| 2 | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| 1 | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| D | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 1 | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| 2 | Code-heavy | cold-startup | 19 | 467 | 467 | 467 |
| 0 | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| D | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| D | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |

## first.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 22 | 25.1 | 38.9 |
| 1 | Overview | cold-startup | 20 | 30.1 | 35.2 | 38 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 22.4 | 29 | 81.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 28.2 | 38.6 | 42.1 |
| 1 | Code-heavy | cold-startup | 20 | 28.8 | 41.2 | 76.3 |
| 2 | Code-heavy | cold-startup | 20 | 21.7 | 26.8 | 33.3 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21 | 29.1 | 33 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 26 | 38.9 | 56.2 |

## first.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 9.2 | 10.7 | 14.6 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 9.4 | 10.7 | 10.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 92.3 | 318.6 | 373.4 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 90.7 | 262.4 | 425.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## first.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 46.8 | 84 | 88.4 |
| 1 | Overview | cold-startup | 20 | 7.2 | 9 | 10.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 49.3 | 108.9 | 221.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7.5 | 8.2 | 8.3 |
| 1 | Code-heavy | cold-startup | 20 | 93.6 | 328.3 | 344.7 |
| 2 | Code-heavy | cold-startup | 20 | 904.6 | 2290.8 | 3859.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 918 | 2800.5 | 3176.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 94 | 150.1 | 390.1 |

## first.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 90.8 | 152.6 | 164.8 |
| 1 | Overview | cold-startup | 20 | 49.7 | 58.9 | 64.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 93 | 163 | 666.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 47.2 | 61.9 | 80.6 |
| 1 | Code-heavy | cold-startup | 20 | 132.4 | 383.4 | 412.5 |
| 2 | Code-heavy | cold-startup | 20 | 1021.6 | 2573.1 | 4293.6 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1044.1 | 3313.7 | 3378.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 130.1 | 239 | 443.5 |

## second.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 147.6 | 197.3 | 408.7 |
| 0 | Small | cold-startup | 20 | 146.9 | 263.1 | 297.7 |
| 2 | Small | cold-startup | 20 | 131.5 | 164.9 | 231.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 148.1 | 400.3 | 559.6 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 146.9 | 227.1 | 376.4 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 147.6 | 265.4 | 393.7 |
| 1 | Code-heavy | cold-startup | 20 | 918.7 | 1625.3 | 1942.7 |
| 2 | Code-heavy | cold-startup | 19 | 1929.3 | 5149.6 | 5149.6 |
| 0 | Code-heavy | cold-startup | 20 | 1028.8 | 1674 | 1779.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1850.8 | 3558.3 | 4008.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1032.8 | 1954 | 2545.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 917.4 | 2127.6 | 2634.4 |

## second.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 147.4 | 197.3 | 408.7 |
| 0 | Small | cold-startup | 20 | 115 | 248.1 | 280.1 |
| 2 | Small | cold-startup | 20 | 131.5 | 164.9 | 231.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 148.1 | 400.3 | 559.6 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 115.7 | 198.6 | 360.6 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 147.6 | 265.4 | 393.7 |
| 1 | Code-heavy | cold-startup | 20 | 918.7 | 1625.3 | 1942.7 |
| 2 | Code-heavy | cold-startup | 19 | 1929.3 | 5149.6 | 5149.6 |
| 0 | Code-heavy | cold-startup | 20 | 1028.8 | 1674 | 1779.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1850.8 | 3558.3 | 4008.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1032.8 | 1954 | 2545.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 917.4 | 2127.6 | 2634.4 |

## second.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 25.8 | 55.4 | 90.4 |
| 2 | Small | cold-startup | 20 | 19.7 | 23 | 23.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 24.6 | 162 | 371.2 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 20.4 | 26 | 73.4 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 24.7 | 30.4 | 32.8 |
| 2 | Code-heavy | cold-startup | 19 | 20.2 | 34.1 | 34.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 20.4 | 29.4 | 43.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 25.4 | 135.7 | 185.8 |

## second.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 2.7 | 4.2 | 6.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 2.9 | 3.6 | 4.7 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 88.2 | 596.2 | 596.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 85.1 | 176 | 455.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## second.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 2.9 | 3.2 | 4.1 |
| 2 | Small | cold-startup | 20 | 5.4 | 6.2 | 6.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 2.8 | 3.8 | 3.8 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 5.5 | 6.4 | 7.1 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 93.8 | 181.2 | 213.1 |
| 2 | Code-heavy | cold-startup | 19 | 856.3 | 2757.5 | 2757.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 812 | 1949.6 | 1990.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 95.4 | 178.8 | 398.1 |

## second.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 37.1 | 75.8 | 101.9 |
| 2 | Small | cold-startup | 20 | 34.4 | 43.7 | 46.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 35.2 | 197.4 | 382.1 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 38 | 89.2 | 96.3 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 203 | 269.2 | 288.8 |
| 2 | Code-heavy | cold-startup | 19 | 1078.1 | 3171.5 | 3171.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1018.1 | 2299.5 | 2578.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 243.6 | 332.5 | 572.5 |

## retained.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## evicted.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 18.1 | 40.2 | 48.3 |
| 1 | Overview | cold-startup | 20 | 25.8 | 45.8 | 86 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 19.7 | 23.3 | 34.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 26.3 | 108.8 | 239.2 |
| 1 | Code-heavy | cold-startup | 20 | 23.8 | 29.3 | 213.6 |
| 2 | Code-heavy | cold-startup | 19 | 17.6 | 24.9 | 24.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 17.1 | 19.6 | 23.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 22.5 | 60.2 | 219.5 |

## evicted.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 7.7 | 9.7 | 10 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 8 | 13 | 14.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 75.5 | 245.8 | 245.8 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 73.1 | 118 | 184.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## evicted.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 42.7 | 83.5 | 100.9 |
| 1 | Overview | cold-startup | 20 | 7.2 | 7.8 | 8.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 42.1 | 70.5 | 265.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7.1 | 9.3 | 9.4 |
| 1 | Code-heavy | cold-startup | 20 | 86.1 | 122 | 127.8 |
| 2 | Code-heavy | cold-startup | 19 | 763.7 | 1945.9 | 1945.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 764.4 | 913 | 1571.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 85.1 | 144.4 | 188.5 |

## evicted.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 75.4 | 149.4 | 159.5 |
| 1 | Overview | cold-startup | 20 | 40.6 | 99.5 | 124.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 78.2 | 187.9 | 592.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 41.8 | 122.4 | 281.2 |
| 1 | Code-heavy | cold-startup | 20 | 126.9 | 180.6 | 321.9 |
| 2 | Code-heavy | cold-startup | 19 | 867 | 2227.1 | 2227.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 866.6 | 1028.7 | 1793.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 116.8 | 307 | 418.8 |

## sourceFirst.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 79.5 | 90.1 | 94.7 |
| 0 | Small | cold-startup | 20 | 65.3 | 79.4 | 82.5 |
| 2 | Small | cold-startup | 20 | 174.5 | 278.6 | 281.7 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 79.4 | 97.7 | 103.8 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 61.4 | 83.5 | 96.4 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 172.8 | 318.4 | 484.8 |
| 0 | Overview | cold-startup | 20 | 60.8 | 79.9 | 105.4 |
| 2 | Overview | cold-startup | 20 | 116.8 | 320 | 446.7 |
| 1 | Overview | cold-startup | 20 | 40.1 | 45.8 | 45.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 111.8 | 266.5 | 347.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 60.5 | 69.8 | 87.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 40.4 | 46.7 | 49 |
| 1 | Code-heavy | cold-startup | 20 | 59.1 | 203.4 | 322.7 |
| 2 | Code-heavy | cold-startup | 20 | 110.3 | 444.4 | 1081.2 |
| 0 | Code-heavy | cold-startup | 20 | 134.3 | 324.5 | 354.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 117.9 | 678.8 | 1287.3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 133.2 | 353.3 | 771.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 55 | 66.9 | 183.7 |

## sourceFirst.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 25 | 28.4 | 40.2 |
| 2 | Small | cold-startup | 20 | 21 | 24.1 | 24.6 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 24.5 | 33.3 | 34.3 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 20.9 | 33.3 | 38.8 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceFirst.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 5.2 | 5.9 | 6.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 5.5 | 8.3 | 9 |
| 2 | Overview | cold-startup | 20 | 3.6 | 6.2 | 13.9 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 3.5 | 6.1 | 6.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceFirst.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 8.2 | 8.9 | 9.1 |
| 2 | Small | cold-startup | 20 | 95.6 | 200.1 | 200.3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 8.3 | 9.2 | 10.7 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 96.9 | 223.3 | 381.2 |
| 2 | Overview | cold-startup | 20 | 73.5 | 271.9 | 392.8 |
| 1 | Overview | cold-startup | 20 | 6.8 | 7.8 | 9.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 74.7 | 221.6 | 259.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 6.8 | 7.4 | 7.5 |
| 1 | Code-heavy | cold-startup | 20 | 8 | 9.3 | 9.7 |
| 2 | Code-heavy | cold-startup | 20 | 49.5 | 59.4 | 72.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 50.3 | 62.7 | 69.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8 | 8.7 | 8.7 |

## sourceFirst.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 41.9 | 50.4 | 56 |
| 2 | Small | cold-startup | 20 | 130.6 | 239.6 | 242.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 41.4 | 56.6 | 63.5 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 134.1 | 276.6 | 432.3 |
| 2 | Overview | cold-startup | 20 | 77.4 | 276.4 | 399.8 |
| 1 | Overview | cold-startup | 20 | 7 | 8.3 | 9.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 78.1 | 228 | 264.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7.2 | 7.7 | 7.8 |
| 1 | Code-heavy | cold-startup | 20 | 9.1 | 10.2 | 10.9 |
| 2 | Code-heavy | cold-startup | 20 | 51.4 | 74.7 | 1017.8 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 53.6 | 357.8 | 628.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 9.1 | 9.8 | 10 |

## sourceReopen.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.4 |
| 1 | Code-heavy | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Code-heavy | cold-startup | 20 | 0.1 | 0.3 | 5.2 |
| 0 | Code-heavy | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |

## sourceReopen.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## edit.fence.highlightSettledMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 116.973 | 185.622 | 601.502 |
| 0 | Small | cold-startup | 20 | 83.424 | 84.216 | 85.312 |
| 2 | Small | cold-startup | 20 | 151.467 | 181.156 | 185.017 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 117.021 | 332.295 | 464.66 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 83.437 | 101.132 | 316.502 |
| 2 | Small | fresh-profile-after-examples-settled | 20 | 148.012 | 184.862 | 229.815 |
| 0 | Overview | cold-startup | 20 | 100.143 | 108.372 | 150.095 |
| 2 | Overview | cold-startup | 20 | 132.997 | 143.446 | 166.078 |
| 1 | Overview | cold-startup | 20 | 99.916 | 100.557 | 109.351 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 133.481 | 150.414 | 298.6 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 99.84 | 100.986 | 140.698 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 99.944 | 112.131 | 116.185 |
| 1 | Code-heavy | cold-startup | 20 | 670.1 | 1508.08 | 1565.79 |
| 2 | Code-heavy | cold-startup | 19 | 1096.522 | 4588.372 | 4588.372 |
| 0 | Code-heavy | cold-startup | 20 | 787.111 | 1750.537 | 2014.459 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 778.433 | 1439.609 | 1443.584 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 908.007 | 1844.261 | 3347.831 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 684.171 | 1045.763 | 1438.467 |

## first.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 223.1 | 446.8 | 1207.2 |
| 2 | Overview | cold-startup | 20 | 298.1 | 930.1 | 1075.2 |
| 1 | Overview | cold-startup | 20 | 258.1 | 366.5 | 569.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 307.3 | 1115.2 | 1671.7 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 228.8 | 572 | 944.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 251.6 | 629.5 | 1033.9 |
| 1 | Code-heavy | cold-startup | 20 | 671.9 | 1145.2 | 1280.5 |
| 2 | Code-heavy | cold-startup | 19 | 1769.5 | 3911.6 | 3911.6 |
| 0 | Code-heavy | cold-startup | 20 | 910.2 | 1732.8 | 2145.4 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1782.2 | 4119.7 | 4487.3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 894.2 | 1743.6 | 2339.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 675.1 | 1050.9 | 1475.5 |

## first.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 205.6 | 431.4 | 1176.4 |
| 2 | Overview | cold-startup | 20 | 298.1 | 930.1 | 1075.2 |
| 1 | Overview | cold-startup | 20 | 258.1 | 366.5 | 569.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 307.3 | 1115.2 | 1671.7 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 210.1 | 526.3 | 895 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 251.6 | 629.5 | 1033.9 |
| 1 | Code-heavy | cold-startup | 20 | 671.9 | 1145.2 | 1280.5 |
| 2 | Code-heavy | cold-startup | 19 | 1769.5 | 3911.6 | 3911.6 |
| 0 | Code-heavy | cold-startup | 20 | 910.2 | 1732.8 | 2145.4 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1782.2 | 4119.7 | 4487.3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 894.2 | 1743.6 | 2339.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 675.1 | 1050.9 | 1475.5 |

## retained.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 48.4 | 49.7 | 53.7 |
| 2 | Overview | cold-startup | 20 | 48.1 | 56.7 | 57.1 |
| 1 | Overview | cold-startup | 20 | 48.7 | 57.1 | 132.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 48.4 | 54.2 | 65.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 48.3 | 49.7 | 50.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 48.9 | 81.6 | 89.9 |
| 1 | Code-heavy | cold-startup | 20 | 46 | 395 | 459.5 |
| 2 | Code-heavy | cold-startup | 19 | 45.5 | 344.9 | 344.9 |
| 0 | Code-heavy | cold-startup | 20 | 45.8 | 277.6 | 446.4 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45 | 392.6 | 1191 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45.9 | 49.6 | 245.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45.8 | 303.1 | 571.3 |

## retained.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 19 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## evicted.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 148.1 | 348.8 | 365.1 |
| 2 | Overview | cold-startup | 20 | 161.9 | 363.1 | 626.2 |
| 1 | Overview | cold-startup | 20 | 132.5 | 215.8 | 232.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 182.3 | 444.1 | 1102.8 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 148 | 214.2 | 365.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 150 | 363.8 | 531.5 |
| 1 | Code-heavy | cold-startup | 20 | 665.1 | 961.7 | 1745.4 |
| 2 | Code-heavy | cold-startup | 19 | 1420.4 | 2800.8 | 2800.8 |
| 0 | Code-heavy | cold-startup | 20 | 732.3 | 1464 | 1482.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1349.5 | 1537.6 | 2295.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 781.8 | 948.4 | 1162 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 648.8 | 1022.7 | 1136.4 |

## evicted.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 103.5 | 348.8 | 349.1 |
| 2 | Overview | cold-startup | 20 | 161.9 | 363.1 | 626.2 |
| 1 | Overview | cold-startup | 20 | 132.5 | 215.8 | 232.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 20 | 182.3 | 444.1 | 1102.8 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 118.6 | 185.4 | 316.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 150 | 363.8 | 531.5 |
| 1 | Code-heavy | cold-startup | 20 | 665.1 | 961.7 | 1745.4 |
| 2 | Code-heavy | cold-startup | 19 | 1420.4 | 2800.8 | 2800.8 |
| 0 | Code-heavy | cold-startup | 20 | 732.3 | 1464 | 1482.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1349.5 | 1537.6 | 2295.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 781.8 | 948.4 | 1162 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 648.8 | 1022.7 | 1136.4 |
