# Performance observations

Planned n=20; inspect actual n per cell. Incomplete sets and correctness failures cannot establish a performance verdict; nearest-rank tails remain exploratory.

Nearest rank, rounded for display; JSON/CSV retains full precision. Values are kept separate by fixture, cache condition and run set.

## first.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 194.3 | 220.1 | 243.2 |
| 0 | Small | cold-startup | 20 | 185.1 | 241.9 | 258.6 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 191.5 | 224.1 | 245.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 195.9 | 239.6 | 257.2 |
| 0 | Overview | cold-startup | 20 | 204.6 | 236 | 238.3 |
| 1 | Overview | cold-startup | 20 | 199.2 | 241.3 | 249.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 202.4 | 252.8 | 282.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 203.8 | 240.4 | 261.2 |
| 0 | Code-heavy | cold-startup | 20 | 254.1 | 292.7 | 327.7 |
| 1 | Code-heavy | cold-startup | 20 | 251 | 267.5 | 638 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 243.6 | 279.3 | 632.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 245.6 | 317.9 | 658.5 |

## first.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 86 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 236 |

## first.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 48.2 | 70.7 | 92.3 |
| 0 | Small | cold-startup | 20 | 41.9 | 98.7 | 101.8 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 46.3 | 77.2 | 94.3 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 46.7 | 85.9 | 104.4 |
| 0 | Overview | cold-startup | 20 | 47.1 | 88.3 | 91.2 |
| 1 | Overview | cold-startup | 20 | 41.5 | 78 | 89.4 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 42.2 | 103.8 | 116.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 45.3 | 90.4 | 115.7 |
| 0 | Code-heavy | cold-startup | 20 | 91.1 | 139.5 | 155.5 |
| 1 | Code-heavy | cold-startup | 20 | 90.6 | 121.8 | 490.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 89.9 | 136.9 | 471.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 90.4 | 168.7 | 425.4 |

## first.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 2.5 | 3.6 | 3.9 |
| 0 | Small | cold-startup | 20 | 2.2 | 3.4 | 3.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 2 | 3.1 | 3.8 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 2.5 | 3.5 | 3.6 |
| 0 | Overview | cold-startup | 20 | 2.6 | 3.1 | 4 |
| 1 | Overview | cold-startup | 20 | 2.7 | 4 | 4.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 2.5 | 3.2 | 3.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 2.6 | 3.9 | 4.1 |
| 0 | Code-heavy | cold-startup | 20 | 4.4 | 5.1 | 5.4 |
| 1 | Code-heavy | cold-startup | 20 | 4.7 | 6.3 | 7.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.2 | 6.6 | 6.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.7 | 5.3 | 9.1 |

## first.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 216197 | 216199 | 216199 |
| 0 | Small | cold-startup | 20 | 215804 | 215806 | 215806 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 216197 | 216199 | 216199 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 215804 | 215806 | 215806 |
| 0 | Overview | cold-startup | 20 | 252507 | 252509 | 252509 |
| 1 | Overview | cold-startup | 20 | 336949 | 336951 | 336951 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 252507 | 252509 | 252509 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 336949 | 336951 | 336951 |
| 0 | Code-heavy | cold-startup | 20 | 760253 | 760255 | 760255 |
| 1 | Code-heavy | cold-startup | 20 | 344900 | 344902 | 344902 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 760253 | 760255 | 760255 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 344900 | 344902 | 344902 |

## first.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 211097 | 211099 | 211099 |
| 0 | Small | cold-startup | 20 | 210704 | 210706 | 210706 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 211097 | 211099 | 211099 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 210704 | 210706 | 210706 |
| 0 | Overview | cold-startup | 20 | 247707 | 247709 | 247709 |
| 1 | Overview | cold-startup | 20 | 328549 | 328551 | 328551 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 247707 | 247709 | 247709 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 328549 | 328551 | 328551 |
| 0 | Code-heavy | cold-startup | 20 | 754853 | 754855 | 754855 |
| 1 | Code-heavy | cold-startup | 20 | 335900 | 335902 | 335902 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754853 | 754855 | 754855 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335900 | 335902 | 335902 |

## first.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.2 | 0.2 | 0.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.2 | 0.2 | 0.2 |
| 0 | Code-heavy | cold-startup | 20 | 12.7 | 13.101 | 13.2 |
| 1 | Code-heavy | cold-startup | 20 | 15 | 15.901 | 19.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 13 | 14 | 16 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 15.1 | 21 | 21.4 |

## first.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.1 |
| 0 | Code-heavy | cold-startup | 20 | 4.801 | 5.199 | 5.3 |
| 1 | Code-heavy | cold-startup | 20 | 4.3 | 4.701 | 5.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.8 | 5.301 | 5.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.3 | 4.8 | 5.8 |

## first.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 86 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 178 |

## first.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 22 | 28.4 | 30.8 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 23 | 27 | 27.2 |
| 1 | Code-heavy | cold-startup | 20 | 22.4 | 29 | 79.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21.8 | 45.1 | 48.5 |

## first.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## first.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 7.1 | 8.1 | 8.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7 | 7.6 | 7.8 |
| 1 | Code-heavy | cold-startup | 20 | 90.5 | 94.4 | 327.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 90.3 | 119.1 | 140 |

## first.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 39.9 | 48 | 48.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 39.3 | 43.6 | 44.5 |
| 1 | Code-heavy | cold-startup | 20 | 123.2 | 131.7 | 467.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 121.7 | 159.2 | 215.9 |

## first.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 7410333 | 7417264 | 7433676 |
| 0 | Small | cold-startup | 20 | 7407506 | 7417664 | 7443873 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7408918 | 7416198 | 7431954 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7409366 | 7440739 | 7443767 |
| 0 | Overview | cold-startup | 20 | 7764351 | 7781740 | 7789117 |
| 1 | Overview | cold-startup | 20 | 7778604 | 7804060 | 7807687 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 7764055 | 7779303 | 7783401 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7778019 | 7788627 | 7801593 |
| 0 | Code-heavy | cold-startup | 20 | 9261669 | 9610969 | 9619796 |
| 1 | Code-heavy | cold-startup | 20 | 8731671 | 9323714 | 9397431 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 9247296 | 9325707 | 9325777 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8736025 | 9232861 | 9246241 |

## second.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 130.3 | 132.2 | 147 |
| 0 | Small | cold-startup | 20 | 130.7 | 163.4 | 181.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 130.8 | 146.9 | 162.7 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 131 | 149.2 | 161.8 |
| 0 | Overview | cold-startup | 20 | 120.6 | 153.3 | 154.2 |
| 1 | Overview | cold-startup | 20 | 136.5 | 153.2 | 170 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 120.6 | 138.1 | 153.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 122.2 | 167.7 | 168.8 |
| 0 | Code-heavy | cold-startup | 20 | 212.2 | 437.3 | 439.3 |
| 1 | Code-heavy | cold-startup | 20 | 402.2 | 415.8 | 500.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 209 | 419 | 1018.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 402 | 410.8 | 1107.1 |

## second.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 130.6 | 132.2 | 147.9 |
| 0 | Small | cold-startup | 20 | 130.7 | 163.4 | 181.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 131.2 | 147.5 | 162.7 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 131 | 149.2 | 161.8 |
| 0 | Code-heavy | cold-startup | 20 | 931.8 | 1234.4 | 1246.8 |
| 1 | Code-heavy | cold-startup | 20 | 880.1 | 919 | 982.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 927.9 | 1176.5 | 1887 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 875.2 | 895.6 | 1592.1 |

## second.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 115.1 | 131.5 | 147.9 |
| 0 | Small | cold-startup | 20 | 99.7 | 131.3 | 132.8 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 115.9 | 146.8 | 147.5 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 99.5 | 118 | 130.7 |
| 0 | Code-heavy | cold-startup | 20 | 931.8 | 1234.4 | 1246.8 |
| 1 | Code-heavy | cold-startup | 20 | 880.1 | 919 | 982.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 927.9 | 1176.5 | 1887 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 875.2 | 895.6 | 1592.1 |

## second.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 127 | 222 | 225 |
| 1 | Code-heavy | cold-startup | 20 | 271 | 283 | 286 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 119 | 225 | 456 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 273 | 283 | 585 |

## second.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 5.8 |
| 0 | Small | cold-startup | 20 | 0 | 18.5 | 33.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 5.7 | 16.4 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 5.9 | 17.1 |
| 0 | Overview | cold-startup | 20 | 0 | 6.9 | 11.2 |
| 1 | Overview | cold-startup | 20 | 0 | 10.9 | 18.7 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 13.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 17.7 | 19.3 |
| 0 | Code-heavy | cold-startup | 20 | 44.9 | 65.4 | 125.6 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 65.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 37.3 | 64 | 818.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 61.7 | 197 |

## second.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 1.5 | 2.7 | 3.4 |
| 0 | Small | cold-startup | 20 | 1.6 | 2.2 | 2.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1.6 | 2.1 | 2.8 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1.7 | 2.2 | 2.4 |
| 0 | Overview | cold-startup | 20 | 1.8 | 2.1 | 2.6 |
| 1 | Overview | cold-startup | 20 | 1.8 | 2.6 | 2.6 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1.8 | 2.6 | 2.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1.8 | 2.5 | 3.4 |
| 0 | Code-heavy | cold-startup | 20 | 3.3 | 5.3 | 5.9 |
| 1 | Code-heavy | cold-startup | 20 | 3.2 | 5.4 | 5.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.2 | 5.3 | 7.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.2 | 4.7 | 7.1 |

## second.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 174976 | 174978 | 174978 |
| 0 | Small | cold-startup | 20 | 89255 | 89257 | 89257 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 174976 | 174978 | 174978 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 89255 | 89257 | 89257 |
| 0 | Overview | cold-startup | 20 | 163727 | 163734 | 163734 |
| 1 | Overview | cold-startup | 20 | 164120 | 164127 | 164127 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 163727 | 163734 | 163734 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 164120 | 164127 | 164127 |
| 0 | Code-heavy | cold-startup | 20 | 633100 | 633102 | 633102 |
| 1 | Code-heavy | cold-startup | 20 | 217747 | 217749 | 217749 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 633100 | 633102 | 633102 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 217747 | 217749 | 217749 |

## second.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 293135 | 293137 | 293137 |
| 0 | Small | cold-startup | 20 | 211014 | 211016 | 211016 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 293135 | 293137 | 293137 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 211014 | 211016 | 211016 |
| 0 | Overview | cold-startup | 20 | 233001 | 233008 | 233008 |
| 1 | Overview | cold-startup | 20 | 233394 | 233401 | 233401 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 233001 | 233008 | 233008 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 233394 | 233401 | 233401 |
| 0 | Code-heavy | cold-startup | 20 | 754859 | 754861 | 754861 |
| 1 | Code-heavy | cold-startup | 20 | 335906 | 335908 | 335908 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754859 | 754861 | 754861 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335906 | 335908 | 335908 |

## second.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 10.7 | 12.2 | 12.499 |
| 1 | Code-heavy | cold-startup | 20 | 12.6 | 15 | 15.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 10.9 | 12.7 | 14.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 13.7 | 18.601 | 26.7 |

## second.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 0 | Small | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 4.4 | 4.9 | 5.101 |
| 1 | Code-heavy | cold-startup | 20 | 3.9 | 4.3 | 4.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.6 | 5.099 | 5.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.1 | 5 | 7.299 |

## second.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 127 | 171 | 171 |
| 1 | Code-heavy | cold-startup | 20 | 207 | 220 | 232 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 119 | 171 | 456 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 208 | 217 | 585 |

## second.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 21.4 | 23.7 | 24.3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 20.8 | 24.2 | 24.5 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 21.9 | 24.7 | 94.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 21.1 | 24.7 | 35.9 |

## second.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## second.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 2.6 | 2.8 | 2.9 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 2.7 | 2.8 | 3 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 90.3 | 123.1 | 135.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 90.5 | 120.2 | 133.5 |

## second.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 30.7 | 34.1 | 34.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 30.2 | 34.1 | 34.6 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 190.1 | 385.8 | 467.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 185.7 | 206.2 | 385 |

## second.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 7517227 | 7547438 | 7554450 |
| 0 | Small | cold-startup | 20 | 7466075 | 7497491 | 7509411 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7502497 | 7549822 | 7561404 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7463060 | 7488231 | 7490788 |
| 0 | Overview | cold-startup | 20 | 7813891 | 7828188 | 7849248 |
| 1 | Overview | cold-startup | 20 | 7881465 | 7956722 | 7969312 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 7817375 | 7930735 | 7953437 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 7878166 | 7956653 | 7979394 |
| 0 | Code-heavy | cold-startup | 20 | 20373802 | 23776509 | 23902793 |
| 1 | Code-heavy | cold-startup | 20 | 20322252 | 24488855 | 24533711 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 20484853 | 23576357 | 25266721 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 20495544 | 24493977 | 24534279 |

## retained.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 48.8 | 50.1 | 50.1 |
| 0 | Small | cold-startup | 20 | 48.9 | 50.5 | 51.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 48.7 | 50.6 | 52.3 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 48.7 | 50.9 | 53.6 |
| 0 | Overview | cold-startup | 20 | 48.3 | 52.3 | 52.5 |
| 1 | Overview | cold-startup | 20 | 49 | 50.1 | 53.5 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 48.9 | 54.9 | 59.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 48.9 | 52.5 | 54.9 |
| 0 | Code-heavy | cold-startup | 20 | 45.7 | 238 | 240.5 |
| 1 | Code-heavy | cold-startup | 20 | 46.2 | 280.9 | 301.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45.9 | 49.3 | 92.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 266.5 | 299.9 | 302.9 |

## retained.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 197 | 197 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 256 | 258 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 178 | 261 | 264 |

## retained.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 143 | 145 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 191 | 194 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 178 | 196 | 196 |

## retained.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## retained.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 7798831 | 8584399 | 8589313 |
| 0 | Small | cold-startup | 20 | 7746760 | 8549914 | 8684234 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7804203 | 8599279 | 8599549 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 7759079 | 8531407 | 8672746 |
| 0 | Overview | cold-startup | 20 | 8099556 | 8232496 | 8233633 |
| 1 | Overview | cold-startup | 20 | 8251239 | 8365496 | 8383393 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 8140422 | 8255040 | 8268462 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 8247483 | 8370265 | 8377634 |
| 0 | Code-heavy | cold-startup | 20 | 24144840 | 31403931 | 33266615 |
| 1 | Code-heavy | cold-startup | 20 | 21404151 | 41378609 | 41506149 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 23830276 | 33087400 | 33352079 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 34766062 | 41409008 | 41411941 |

## evicted.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 132 | 147.5 | 148.7 |
| 0 | Small | cold-startup | 20 | 132 | 147.7 | 148.4 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 131.9 | 133.4 | 133.7 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 131.9 | 148.3 | 148.6 |
| 0 | Overview | cold-startup | 20 | 132.1 | 133.8 | 139.9 |
| 1 | Overview | cold-startup | 20 | 116.2 | 133.7 | 136.6 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 132 | 132.3 | 148.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 116.4 | 133.2 | 148.6 |
| 0 | Code-heavy | cold-startup | 20 | 133.3 | 149 | 149.1 |
| 1 | Code-heavy | cold-startup | 20 | 132.2 | 149.3 | 150.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 132.5 | 148.6 | 150.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 132.5 | 149.3 | 150.4 |

## evicted.longTaskMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 122 | 135 |
| 1 | Code-heavy | cold-startup | 20 | 88 | 179 | 211 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 67 | 128 | 129 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 113 | 120 |

## evicted.loaderMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 5.7 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 6.1 | 6.3 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 5.9 | 6.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 5.8 | 7.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 5.6 | 5.8 |

## evicted.documentRequestMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 1.3 | 1.7 | 2.2 |
| 0 | Small | cold-startup | 20 | 1.3 | 2.1 | 2.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1.3 | 1.5 | 2.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1.3 | 1.8 | 2 |
| 0 | Overview | cold-startup | 20 | 1.6 | 2.1 | 2.2 |
| 1 | Overview | cold-startup | 20 | 1.6 | 2 | 2.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1.5 | 1.9 | 2.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1.6 | 2.2 | 2.8 |
| 0 | Code-heavy | cold-startup | 20 | 3.3 | 4.1 | 5.4 |
| 1 | Code-heavy | cold-startup | 20 | 3.2 | 4.3 | 4.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.3 | 4.6 | 4.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 3.1 | 4.1 | 5.5 |

## evicted.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 85708 | 85708 | 85708 |
| 0 | Small | cold-startup | 20 | 85315 | 85315 | 85315 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 85708 | 85708 | 85708 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 85315 | 85315 | 85315 |
| 0 | Overview | cold-startup | 20 | 155807 | 155807 | 155807 |
| 1 | Overview | cold-startup | 20 | 240249 | 240249 | 240249 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 155807 | 155807 | 155807 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 240249 | 240249 | 240249 |
| 0 | Code-heavy | cold-startup | 20 | 586993 | 586993 | 586993 |
| 1 | Code-heavy | cold-startup | 20 | 171640 | 171640 | 171640 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 586993 | 586993 | 586993 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 171640 | 171640 | 171640 |

## evicted.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 211097 | 211099 | 211099 |
| 0 | Small | cold-startup | 20 | 210704 | 210706 | 210706 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 211097 | 211099 | 211099 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 210704 | 210706 | 210706 |
| 0 | Overview | cold-startup | 20 | 247707 | 247709 | 247709 |
| 1 | Overview | cold-startup | 20 | 328549 | 328551 | 328551 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 247707 | 247709 | 247709 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 328549 | 328551 | 328551 |
| 0 | Code-heavy | cold-startup | 20 | 754853 | 754855 | 754855 |
| 1 | Code-heavy | cold-startup | 20 | 335900 | 335902 | 335902 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 754853 | 754855 | 754855 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 335900 | 335902 | 335902 |

## evicted.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.3 |
| 0 | Code-heavy | cold-startup | 20 | 10.1 | 12 | 12.6 |
| 1 | Code-heavy | cold-startup | 20 | 11.8 | 13.9 | 14.599 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 10.1 | 10.801 | 12.399 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 11.899 | 15.1 | 15.9 |

## evicted.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 1 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 0 | Code-heavy | cold-startup | 20 | 4.5 | 4.8 | 4.801 |
| 1 | Code-heavy | cold-startup | 20 | 4 | 4.4 | 4.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4.401 | 5 | 5.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 4 | 4.6 | 5.5 |

## evicted.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 122 | 135 |
| 1 | Code-heavy | cold-startup | 20 | 88 | 161 | 179 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 67 | 128 | 129 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 113 | 120 |

## evicted.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 20.6 | 24.4 | 32.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 20.6 | 22.8 | 26.6 |
| 1 | Code-heavy | cold-startup | 20 | 20.3 | 23.7 | 26.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 19.9 | 25.4 | 26.1 |

## evicted.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## evicted.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 6.6 | 7.2 | 7.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 6.6 | 7.2 | 7.3 |
| 1 | Code-heavy | cold-startup | 20 | 80.5 | 82.7 | 82.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 79.7 | 87 | 344.1 |

## evicted.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 33.7 | 38.2 | 46.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 33.4 | 36.1 | 39.6 |
| 1 | Code-heavy | cold-startup | 20 | 108 | 116.4 | 116.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 107.6 | 119.1 | 380.6 |

## evicted.sharedBrowserHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 10821943 | 25127074 | 25132177 |
| 0 | Small | cold-startup | 20 | 11141107 | 25161088 | 25243829 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 10986555 | 25179902 | 25225231 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 10778758 | 25119922 | 25164734 |
| 0 | Overview | cold-startup | 20 | 14697213 | 25619068 | 25622946 |
| 1 | Overview | cold-startup | 20 | 14824732 | 25500476 | 25811643 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 14669449 | 25568960 | 25684978 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 14720735 | 25800949 | 25987315 |
| 0 | Code-heavy | cold-startup | 20 | 28643917 | 64483929 | 69961413 |
| 1 | Code-heavy | cold-startup | 20 | 29608680 | 42195462 | 49752335 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 25378481 | 42145835 | 56856375 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 29545454 | 77592181 | 78904147 |

## sourceFirst.readableMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 14.4 | 16.3 | 16.4 |
| 0 | Small | cold-startup | 20 | 14.6 | 16.1 | 16.6 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 14.1 | 15.3 | 16.7 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 14.4 | 14.9 | 15.5 |
| 0 | Overview | cold-startup | 20 | 14.4 | 15.6 | 15.7 |
| 1 | Overview | cold-startup | 20 | 12.3 | 20.3 | 21.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 14.1 | 14.8 | 15.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 12.1 | 12.7 | 12.9 |
| 0 | Code-heavy | cold-startup | 20 | 18.3 | 22.6 | 36.8 |
| 1 | Code-heavy | cold-startup | 20 | 19.8 | 23.2 | 40.3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 9 | 22.9 | 23.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 17.7 | 24.3 | 71.3 |

## sourceFirst.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Code-heavy | cold-startup | 20 | 2.1 | 2.2 | 2.2 |
| 1 | Code-heavy | cold-startup | 20 | 2.1 | 2.3 | 3.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2.1 | 2.2 | 2.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2.1 | 2.4 | 2.9 |

## sourceFirst.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.1 | 0.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 1 | Overview | cold-startup | 20 | 0 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0.1 | 0.1 |
| 0 | Code-heavy | cold-startup | 20 | 0.6 | 0.7 | 0.7 |
| 1 | Code-heavy | cold-startup | 20 | 0.6 | 0.8 | 0.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.6 | 0.7 | 0.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.6 | 0.7 | 0.9 |

## sourceFirst.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceFirst.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 87208 | 87208 | 87208 |
| 0 | Small | cold-startup | 20 | 1540 | 1540 | 1540 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 87208 | 87208 | 87208 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1540 | 1540 | 1540 |
| 0 | Overview | cold-startup | 20 | 4697 | 4697 | 4697 |
| 1 | Overview | cold-startup | 20 | 4448 | 4448 | 4448 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 4697 | 4697 | 4697 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 4448 | 4448 | 4448 |
| 0 | Code-heavy | cold-startup | 20 | 236384 | 236384 | 236384 |
| 1 | Code-heavy | cold-startup | 20 | 200228 | 200228 | 200228 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 236384 | 236384 | 236384 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 200228 | 200228 | 200228 |

## sourceFirst.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 82708 | 82708 | 82708 |
| 0 | Small | cold-startup | 20 | 940 | 940 | 940 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 82708 | 82708 | 82708 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 940 | 940 | 940 |
| 0 | Overview | cold-startup | 20 | 4097 | 4097 | 4097 |
| 1 | Overview | cold-startup | 20 | 3848 | 3848 | 3848 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 4097 | 4097 | 4097 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 3848 | 3848 | 3848 |
| 0 | Code-heavy | cold-startup | 20 | 235784 | 235784 | 235784 |
| 1 | Code-heavy | cold-startup | 20 | 199628 | 199628 | 199628 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 235784 | 235784 | 235784 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 199628 | 199628 | 199628 |

## sourceFirst.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 73.5 | 77.8 | 78.9 |
| 0 | Small | cold-startup | 20 | 61.4 | 67.3 | 72.3 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 74.2 | 78.4 | 82.6 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 59.5 | 66.7 | 68.3 |
| 0 | Overview | cold-startup | 20 | 57.1 | 59.1 | 59.8 |
| 1 | Overview | cold-startup | 20 | 39.9 | 48.3 | 49.5 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 57.2 | 64.7 | 65.8 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 39.8 | 40.6 | 40.8 |
| 0 | Code-heavy | cold-startup | 20 | 109.3 | 120.1 | 130.3 |
| 1 | Code-heavy | cold-startup | 20 | 50.2 | 59.8 | 77 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 108.7 | 127 | 213.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 53.3 | 61.1 | 113.9 |

## sourceFirst.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 20.5 | 23.9 | 24 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 21 | 24.9 | 25.6 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceFirst.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceFirst.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 7.9 | 8 | 8.5 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 7.9 | 8.2 | 8.6 |
| 1 | Overview | cold-startup | 20 | 6.5 | 6.9 | 7 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 6.6 | 6.8 | 6.8 |
| 1 | Code-heavy | cold-startup | 20 | 7.6 | 8.2 | 8.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 7.6 | 8.3 | 9.1 |

## sourceFirst.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 35.5 | 39.8 | 40 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 36.2 | 40.5 | 43.8 |
| 1 | Overview | cold-startup | 20 | 6.9 | 7.2 | 7.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 6.8 | 7 | 7.1 |
| 1 | Code-heavy | cold-startup | 20 | 8.6 | 9.3 | 10.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8.6 | 9.3 | 10.9 |

## sourceReopen.readableMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 15.6 | 16.7 | 17.1 |
| 0 | Small | cold-startup | 20 | 15.7 | 16.7 | 17.1 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 15.6 | 16.5 | 17.1 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 15.7 | 16.3 | 16.7 |
| 0 | Overview | cold-startup | 20 | 15.3 | 16.8 | 17.4 |
| 1 | Overview | cold-startup | 20 | 15.3 | 17 | 17.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 15.5 | 17.2 | 17.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 15.5 | 16.8 | 17.5 |
| 0 | Code-heavy | cold-startup | 20 | 14.6 | 15.5 | 229.3 |
| 1 | Code-heavy | cold-startup | 20 | 13.9 | 15 | 15.6 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 14.3 | 15.8 | 15.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 13.8 | 15.2 | 15.8 |

## sourceReopen.template-parseMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.code-insertMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.longTaskMaxMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 174 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.bytesTransferred

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.bytesDecoded

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 0 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.1 |
| 0 | Code-heavy | cold-startup | 20 | 0.1 | 0.1 | 0.2 |
| 1 | Code-heavy | cold-startup | 20 | 0.1 | 0.2 | 0.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.1 | 0.1 | 0.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0.1 | 0.2 | 0.6 |

## sourceReopen.initializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.grammarInitializationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.tokenizationMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## sourceReopen.totalWorkerMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## edit.fence.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 44.641 | 45.753 | 46.467 |
| 0 | Small | cold-startup | 20 | 44.281 | 46.153 | 46.52 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 44 | 45.843 | 46.125 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 44.977 | 47.061 | 48.774 |
| 0 | Overview | cold-startup | 20 | 44.17 | 46.853 | 46.921 |
| 1 | Overview | cold-startup | 20 | 45.255 | 51.78 | 52.024 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 44.363 | 52.242 | 52.649 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 45.391 | 49.424 | 50.896 |
| 0 | Code-heavy | cold-startup | 20 | 71.165 | 429.106 | 430.543 |
| 1 | Code-heavy | cold-startup | 20 | 69.4 | 584.785 | 592.907 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 65.588 | 426.728 | 427.872 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 67.384 | 568.308 | 579.638 |

## edit.fence.highlightSettledMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 116.616 | 118.679 | 118.765 |
| 0 | Small | cold-startup | 20 | 82.606 | 84.839 | 85.069 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 115.925 | 118.029 | 118.407 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 83.43 | 84.39 | 85.258 |
| 0 | Overview | cold-startup | 20 | 100.193 | 101.201 | 102.155 |
| 1 | Overview | cold-startup | 20 | 99.955 | 101.97 | 110.533 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 99.914 | 101.291 | 101.464 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 100.068 | 101.898 | 101.934 |
| 0 | Code-heavy | cold-startup | 20 | 864.669 | 1146.953 | 1147.15 |
| 1 | Code-heavy | cold-startup | 20 | 663.396 | 790.366 | 799.781 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 795.502 | 1155.886 | 1172.019 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 663.502 | 836.871 | 879.587 |

## edit.example.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 159.266 | 170.117 | 175.081 |
| 0 | Small | cold-startup | 20 | 160.626 | 170.392 | 174.577 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 159.757 | 166.924 | 168.363 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 159.885 | 167.808 | 174.534 |
| 0 | Overview | cold-startup | 20 | 103.776 | 123.556 | 126.149 |
| 1 | Overview | cold-startup | 20 | 112.168 | 125.01 | 133.197 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 108.536 | 125.143 | 132.121 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 106.744 | 124.929 | 131.652 |
| 0 | Code-heavy | cold-startup | 20 | 204.581 | 260.081 | 285.246 |
| 1 | Code-heavy | cold-startup | 20 | 210.561 | 251.118 | 272.797 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 193.411 | 267.09 | 279.699 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 205.596 | 248.298 | 248.894 |

## edit.shared.visibleMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 91.897 | 104.102 | 111.687 |
| 0 | Small | cold-startup | 20 | 93.495 | 105.151 | 107.22 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 93.7 | 101.902 | 104.08 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 92.477 | 117.144 | 118.57 |
| 0 | Overview | cold-startup | 20 | 98.533 | 113.44 | 127.378 |
| 1 | Overview | cold-startup | 20 | 100.624 | 123.373 | 131.796 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 100.155 | 114.884 | 143.873 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 107.213 | 127.222 | 128.325 |
| 0 | Code-heavy | cold-startup | 20 | 128.078 | 164.77 | 198.006 |
| 1 | Code-heavy | cold-startup | 20 | 127.171 | 144.255 | 161.825 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 128.561 | 183.88 | 223.491 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 127.232 | 148.306 | 161.475 |

## memory.peakServerRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 450072576 | 456151040 | 460005376 |
| 0 | Small | cold-startup | 20 | 456916992 | 471494656 | 471650304 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 450895872 | 459677696 | 460521472 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 457580544 | 469536768 | 476835840 |
| 0 | Overview | cold-startup | 20 | 479707136 | 486785024 | 489885696 |
| 1 | Overview | cold-startup | 20 | 472010752 | 483196928 | 483979264 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 480862208 | 491429888 | 492761088 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 472846336 | 479666176 | 484024320 |
| 0 | Code-heavy | cold-startup | 20 | 916463616 | 997560320 | 1001603072 |
| 1 | Code-heavy | cold-startup | 20 | 937041920 | 961478656 | 982192128 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 914739200 | 984363008 | 995504128 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 882761728 | 957952000 | 972025856 |

## memory.peakBrowserRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 984829952 | 997670912 | 1002602496 |
| 0 | Small | cold-startup | 20 | 980041728 | 997310464 | 1005072384 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 985694208 | 1004277760 | 1006903296 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 978440192 | 1004019712 | 1007751168 |
| 0 | Overview | cold-startup | 20 | 1013424128 | 1037824000 | 1044815872 |
| 1 | Overview | cold-startup | 20 | 1004376064 | 1035489280 | 1037680640 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1014091776 | 1038155776 | 1043259392 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1011634176 | 1031254016 | 1043742720 |
| 0 | Code-heavy | cold-startup | 20 | 1085313024 | 1231724544 | 1256521728 |
| 1 | Code-heavy | cold-startup | 20 | 1147088896 | 1268768768 | 1269690368 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1090060288 | 1257476096 | 1267433472 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1137811456 | 1262342144 | 1269956608 |

## memory.settledServerRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 450031616 | 451502080 | 451809280 |
| 0 | Small | cold-startup | 20 | 456212480 | 463392768 | 471650304 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 449929216 | 453545984 | 456241152 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 457138176 | 463687680 | 476835840 |
| 0 | Overview | cold-startup | 20 | 478920704 | 484216832 | 485974016 |
| 1 | Overview | cold-startup | 20 | 471859200 | 477724672 | 483196928 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 480555008 | 484306944 | 487698432 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 472715264 | 479666176 | 481595392 |
| 0 | Code-heavy | cold-startup | 20 | 910151680 | 996225024 | 997560320 |
| 1 | Code-heavy | cold-startup | 20 | 931938304 | 959725568 | 982192128 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 908861440 | 982228992 | 995504128 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 880951296 | 957952000 | 968376320 |

## memory.settledBrowserRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 928124928 | 937771008 | 940400640 |
| 0 | Small | cold-startup | 20 | 925552640 | 938127360 | 938491904 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 929210368 | 940064768 | 940429312 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 925683712 | 936583168 | 938082304 |
| 0 | Overview | cold-startup | 20 | 938827776 | 967118848 | 967741440 |
| 1 | Overview | cold-startup | 20 | 945582080 | 973369344 | 974356480 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 938659840 | 966799360 | 968663040 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 944734208 | 974934016 | 975728640 |
| 0 | Code-heavy | cold-startup | 20 | 1085313024 | 1212243968 | 1225183232 |
| 1 | Code-heavy | cold-startup | 20 | 1132285952 | 1247047680 | 1252380672 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1088274432 | 1221980160 | 1226686464 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 1126830080 | 1262342144 | 1269956608 |

## memory.settledCombinedRSS

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 1378476032 | 1387343872 | 1391902720 |
| 0 | Small | cold-startup | 20 | 1382535168 | 1396879360 | 1406844928 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 1378955264 | 1391976448 | 1393192960 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 1384378368 | 1397899264 | 1414918144 |
| 0 | Overview | cold-startup | 20 | 1418563584 | 1449480192 | 1451335680 |
| 1 | Overview | cold-startup | 20 | 1419411456 | 1452081152 | 1456566272 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1419124736 | 1452969984 | 1453170688 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1419419648 | 1449676800 | 1455300608 |
| 0 | Code-heavy | cold-startup | 20 | 2045325312 | 2111127552 | 2126876672 |
| 1 | Code-heavy | cold-startup | 20 | 2075791360 | 2189422592 | 2220494848 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2007797760 | 2149761024 | 2190327808 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 2049105920 | 2150707200 | 2165092352 |

## cpu.ownedProcessesSeconds

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 15.28 | 16.08 | 16.33 |
| 0 | Small | cold-startup | 20 | 15.23 | 16.27 | 17.03 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 15.35 | 16.3 | 16.93 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 15.35 | 16.09 | 17.26 |
| 0 | Overview | cold-startup | 20 | 15.71 | 16.25 | 16.26 |
| 1 | Overview | cold-startup | 20 | 15.94 | 17.09 | 17.09 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 15.61 | 16.87 | 16.96 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 15.92 | 16.74 | 16.75 |
| 0 | Code-heavy | cold-startup | 20 | 23.56 | 25.88 | 26.4 |
| 1 | Code-heavy | cold-startup | 20 | 23.47 | 24.16 | 29.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 23.77 | 25.56 | 37.99 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 22.78 | 24.26 | 31.4 |

## memory.workerHeap

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 1100988 | 1101376 | 1101376 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1100876 | 1101128 | 1101340 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 8190720 | 8235576 | 8270952 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 8193136 | 8232748 | 8239628 |

## cache.compilerBytes

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 392736 | 393493 | 393494 |
| 0 | Small | cold-startup | 20 | 392734 | 393492 | 393495 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 392760 | 393486 | 393489 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 392760 | 393487 | 393488 |
| 0 | Overview | cold-startup | 20 | 517690 | 518469 | 518469 |
| 1 | Overview | cold-startup | 20 | 517690 | 518467 | 518468 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 517679 | 518477 | 518477 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 517679 | 518477 | 518478 |
| 0 | Code-heavy | cold-startup | 20 | 475552 | 476236 | 476239 |
| 1 | Code-heavy | cold-startup | 20 | 475553 | 476241 | 476241 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 475564 | 476256 | 476257 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 475556 | 476257 | 476257 |

## cache.indexBytes

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 20 | 467 | 467 | 467 |
| 0 | Small | cold-startup | 20 | 467 | 467 | 467 |
| 1 | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 0 | Small | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 0 | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| 1 | Overview | cold-startup | 20 | 1049 | 1049 | 1049 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 1049 | 1049 | 1049 |
| 0 | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| 1 | Code-heavy | cold-startup | 20 | 467 | 467 | 467 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 467 | 467 | 467 |

## first.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 204.6 | 236 | 238.3 |
| 1 | Overview | cold-startup | 20 | 209.2 | 249.3 | 258.6 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 202.4 | 252.8 | 282.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 210.2 | 257.5 | 262 |
| 0 | Code-heavy | cold-startup | 20 | 812 | 880.3 | 883.4 |
| 1 | Code-heavy | cold-startup | 20 | 633.6 | 658.7 | 1256 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 808.6 | 978.6 | 1733.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 631.6 | 703.5 | 1276 |

## first.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 174.7 | 220.1 | 223.9 |
| 1 | Overview | cold-startup | 20 | 209.2 | 243.5 | 258.6 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 172.3 | 223.6 | 267.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 210.2 | 257.5 | 262 |
| 0 | Code-heavy | cold-startup | 20 | 812 | 880.3 | 883.4 |
| 1 | Code-heavy | cold-startup | 20 | 633.6 | 658.7 | 1256 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 808.6 | 978.6 | 1733.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 631.6 | 703.5 | 1276 |

## retained.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 48.3 | 52.3 | 52.5 |
| 1 | Overview | cold-startup | 20 | 49 | 50.1 | 53.5 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 48.9 | 54.9 | 59.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 48.9 | 52.5 | 54.9 |
| 0 | Code-heavy | cold-startup | 20 | 45.7 | 238 | 240.5 |
| 1 | Code-heavy | cold-startup | 20 | 46.2 | 280.9 | 301.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 45.9 | 49.3 | 92.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 266.5 | 299.9 | 302.9 |

## retained.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 20 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 0 | 0 | 0 |

## evicted.visibleAndColouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 132.1 | 133.8 | 139.9 |
| 1 | Overview | cold-startup | 20 | 116.7 | 134.2 | 136.9 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 132 | 132.3 | 148.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 116.8 | 133.7 | 149.1 |
| 0 | Code-heavy | cold-startup | 20 | 664.6 | 768.8 | 772.4 |
| 1 | Code-heavy | cold-startup | 20 | 599.1 | 717.5 | 741.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 697.5 | 802.9 | 809 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 549.7 | 669.7 | 832.5 |

## evicted.colouredMs

| Variant | Fixture | Cache | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 20 | 102 | 103.9 | 109.9 |
| 1 | Overview | cold-startup | 20 | 116 | 134.2 | 136.9 |
| 0 | Overview | fresh-profile-after-examples-settled | 20 | 82.5 | 102.9 | 103.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 20 | 116.1 | 133.7 | 149.1 |
| 0 | Code-heavy | cold-startup | 20 | 664.6 | 768.8 | 772.4 |
| 1 | Code-heavy | cold-startup | 20 | 599.1 | 717.5 | 741.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 20 | 697.5 | 802.9 | 809 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 20 | 549.7 | 669.7 | 832.5 |
