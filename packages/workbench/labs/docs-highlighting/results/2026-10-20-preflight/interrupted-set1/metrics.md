# Observed metrics

Planned n=20; inspect actual n per cell. Incomplete sets and correctness failures cannot establish a performance verdict; nearest-rank tails remain exploratory.

Nearest rank; display rounded to three decimals. JSON retains full precision. No pooling across fixtures, cache conditions or reports. With n=1 all quantiles equal the single observation.

## first.visibleMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 188.2 | 204.3 | 204.3 |
| 0 | Small | cold-startup | 2 | 187.1 | 354.4 | 354.4 |
| 2 | Small | cold-startup | 2 | 179.3 | 251.4 | 251.4 |
| D | Small | cold-startup | 2 | 188.2 | 208.1 | 208.1 |
| D | Small | fresh-profile-after-examples-settled | 2 | 188.2 | 253 | 253 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 170 | 209.2 | 209.2 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 189.8 | 218.2 | 218.2 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 185.5 | 216.8 | 216.8 |
| 0 | Overview | cold-startup | 2 | 210.9 | 213.8 | 213.8 |
| 2 | Overview | cold-startup | 2 | 196.8 | 224.6 | 224.6 |
| 1 | Overview | cold-startup | 2 | 183.8 | 210.8 | 210.8 |
| D | Overview | cold-startup | 2 | 204.6 | 209.7 | 209.7 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 203.4 | 203.4 | 203.4 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 238.7 | 238.7 | 238.7 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 213 | 213 | 213 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 211.1 | 211.1 | 211.1 |
| 2 | Code-heavy | cold-startup | 1 | 330.2 | 330.2 | 330.2 |
| 1 | Code-heavy | cold-startup | 1 | 344.9 | 344.9 | 344.9 |
| 0 | Code-heavy | cold-startup | 1 | 325.3 | 325.3 | 325.3 |
| D | Code-heavy | cold-startup | 1 | 288.4 | 288.4 | 288.4 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 335.2 | 335.2 | 335.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 263.1 | 263.1 | 263.1 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 337.1 | 337.1 | 337.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 268.9 | 268.9 | 268.9 |

## first.longTaskMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## first.loaderMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 37.1 | 54.7 | 54.7 |
| 0 | Small | cold-startup | 2 | 28.1 | 210.5 | 210.5 |
| 2 | Small | cold-startup | 2 | 24.8 | 107.7 | 107.7 |
| D | Small | cold-startup | 2 | 43.5 | 64 | 64 |
| D | Small | fresh-profile-after-examples-settled | 2 | 42.9 | 106.4 | 106.4 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 17.7 | 53.6 | 53.6 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 36.3 | 70.1 | 70.1 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 42.4 | 70.3 | 70.3 |
| 0 | Overview | cold-startup | 2 | 48.3 | 55 | 55 |
| 2 | Overview | cold-startup | 2 | 40.8 | 79.9 | 79.9 |
| 1 | Overview | cold-startup | 2 | 34.8 | 44.7 | 44.7 |
| D | Overview | cold-startup | 2 | 43.2 | 62 | 62 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 63.2 | 63.2 | 63.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 61.3 | 61.3 | 61.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 70.1 | 70.1 | 70.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 71.1 | 71.1 | 71.1 |
| 2 | Code-heavy | cold-startup | 1 | 186 | 186 | 186 |
| 1 | Code-heavy | cold-startup | 1 | 196.4 | 196.4 | 196.4 |
| 0 | Code-heavy | cold-startup | 1 | 176.1 | 176.1 | 176.1 |
| D | Code-heavy | cold-startup | 1 | 136 | 136 | 136 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 184.5 | 184.5 | 184.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 95.5 | 95.5 | 95.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 187.3 | 187.3 | 187.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 95.3 | 95.3 | 95.3 |

## first.documentRequestMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 1.7 | 2.7 | 2.7 |
| 0 | Small | cold-startup | 2 | 1.8 | 3.2 | 3.2 |
| 2 | Small | cold-startup | 2 | 1.6 | 2.9 | 2.9 |
| D | Small | cold-startup | 2 | 2.2 | 4.8 | 4.8 |
| D | Small | fresh-profile-after-examples-settled | 2 | 2 | 3.8 | 3.8 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 2.3 | 3.5 | 3.5 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 2.2 | 3.2 | 3.2 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 3 | 3.2 | 3.2 |
| 0 | Overview | cold-startup | 2 | 3.6 | 4.2 | 4.2 |
| 2 | Overview | cold-startup | 2 | 2.4 | 3.5 | 3.5 |
| 1 | Overview | cold-startup | 2 | 2.4 | 6.2 | 6.2 |
| D | Overview | cold-startup | 2 | 2.9 | 3 | 3 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 1.8 | 1.8 | 1.8 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 3.5 | 3.5 | 3.5 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 2.3 | 2.3 | 2.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 3.1 | 3.1 | 3.1 |
| 2 | Code-heavy | cold-startup | 1 | 6.6 | 6.6 | 6.6 |
| 1 | Code-heavy | cold-startup | 1 | 5.3 | 5.3 | 5.3 |
| 0 | Code-heavy | cold-startup | 1 | 7.9 | 7.9 | 7.9 |
| D | Code-heavy | cold-startup | 1 | 5.7 | 5.7 | 5.7 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 4.7 | 4.7 | 4.7 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 5 | 5 | 5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 6.4 | 6.4 | 6.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 5.7 | 5.7 | 5.7 |

## first.bytesTransferred

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 216165 | 216165 | 216165 |
| 0 | Small | cold-startup | 2 | 215772 | 215772 | 215772 |
| 2 | Small | cold-startup | 2 | 216165 | 216165 | 216165 |
| D | Small | cold-startup | 2 | 212493 | 212493 | 212493 |
| D | Small | fresh-profile-after-examples-settled | 2 | 212493 | 212493 | 212493 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 216165 | 216165 | 216165 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 216165 | 216165 | 216165 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 215772 | 215772 | 215772 |
| 0 | Overview | cold-startup | 2 | 252475 | 252475 | 252475 |
| 2 | Overview | cold-startup | 2 | 1137577 | 1137577 | 1137577 |
| 1 | Overview | cold-startup | 2 | 336871 | 336871 | 336871 |
| D | Overview | cold-startup | 2 | 247313 | 247313 | 247313 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 247313 | 247313 | 247313 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 1137577 | 1137577 | 1137577 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 252475 | 252475 | 252475 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 336871 | 336871 | 336871 |
| 2 | Code-heavy | cold-startup | 1 | 4690096 | 4690096 | 4690096 |
| 1 | Code-heavy | cold-startup | 1 | 344822 | 344822 | 344822 |
| 0 | Code-heavy | cold-startup | 1 | 760221 | 760221 | 760221 |
| D | Code-heavy | cold-startup | 1 | 255264 | 255264 | 255264 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 255264 | 255264 | 255264 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4690096 | 4690096 | 4690096 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 760221 | 760221 | 760221 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 344822 | 344822 | 344822 |

## first.bytesDecoded

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 211065 | 211065 | 211065 |
| 0 | Small | cold-startup | 2 | 210672 | 210672 | 210672 |
| 2 | Small | cold-startup | 2 | 211065 | 211065 | 211065 |
| D | Small | cold-startup | 2 | 207393 | 207393 | 207393 |
| D | Small | fresh-profile-after-examples-settled | 2 | 207393 | 207393 | 207393 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 211065 | 211065 | 211065 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 211065 | 211065 | 211065 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 210672 | 210672 | 210672 |
| 0 | Overview | cold-startup | 2 | 247675 | 247675 | 247675 |
| 2 | Overview | cold-startup | 2 | 1130077 | 1130077 | 1130077 |
| 1 | Overview | cold-startup | 2 | 328471 | 328471 | 328471 |
| D | Overview | cold-startup | 2 | 242813 | 242813 | 242813 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 242813 | 242813 | 242813 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 1130077 | 1130077 | 1130077 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 247675 | 247675 | 247675 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 328471 | 328471 | 328471 |
| 2 | Code-heavy | cold-startup | 1 | 4662796 | 4662796 | 4662796 |
| 1 | Code-heavy | cold-startup | 1 | 335822 | 335822 | 335822 |
| 0 | Code-heavy | cold-startup | 1 | 754821 | 754821 | 754821 |
| D | Code-heavy | cold-startup | 1 | 250164 | 250164 | 250164 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 250164 | 250164 | 250164 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4662796 | 4662796 | 4662796 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 754821 | 754821 | 754821 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 335822 | 335822 | 335822 |

## first.template-parseMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0.2 | 0.3 | 0.3 |
| 2 | Overview | cold-startup | 2 | 0.1 | 0.3 | 0.3 |
| 1 | Overview | cold-startup | 2 | 0 | 0.1 | 0.1 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0.2 | 0.2 | 0.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0.2 | 0.2 | 0.2 |
| 2 | Code-heavy | cold-startup | 1 | 23.6 | 23.6 | 23.6 |
| 1 | Code-heavy | cold-startup | 1 | 15.601 | 15.601 | 15.601 |
| 0 | Code-heavy | cold-startup | 1 | 12.9 | 12.9 | 12.9 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 19.1 | 19.1 | 19.1 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 12.9 | 12.9 | 12.9 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 15.301 | 15.301 | 15.301 |

## first.code-insertMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0.1 | 0.1 |
| 2 | Overview | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | cold-startup | 2 | 0 | 0.1 | 0.1 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 2 | Code-heavy | cold-startup | 1 | 8.3 | 8.3 | 8.3 |
| 1 | Code-heavy | cold-startup | 1 | 4.301 | 4.301 | 4.301 |
| 0 | Code-heavy | cold-startup | 1 | 5.199 | 5.199 | 5.199 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 6.8 | 6.8 | 6.8 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 5.4 | 5.4 | 5.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4.3 | 4.3 | 4.3 |

## first.longTaskMaxMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## first.initializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 18.1 | 22.7 | 22.7 |
| 1 | Overview | cold-startup | 2 | 23 | 26.3 | 26.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 31.1 | 31.1 | 31.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 28.2 | 28.2 | 28.2 |
| 2 | Code-heavy | cold-startup | 1 | 25 | 25 | 25 |
| 1 | Code-heavy | cold-startup | 1 | 30.1 | 30.1 | 30.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 21.6 | 21.6 | 21.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 28.5 | 28.5 | 28.5 |

## first.grammarInitializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 7.5 | 10.3 | 10.3 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 13.5 | 13.5 | 13.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 134.4 | 134.4 | 134.4 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 78.1 | 78.1 | 78.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## first.tokenizationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 42.7 | 50.4 | 50.4 |
| 1 | Overview | cold-startup | 2 | 7.1 | 7.3 | 7.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 81.6 | 81.6 | 81.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 7.1 | 7.1 | 7.1 |
| 2 | Code-heavy | cold-startup | 1 | 2613.7 | 2613.7 | 2613.7 |
| 1 | Code-heavy | cold-startup | 1 | 107.7 | 107.7 | 107.7 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 811.2 | 811.2 | 811.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 94.7 | 94.7 | 94.7 |

## first.totalWorkerMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 75.1 | 96.3 | 96.3 |
| 1 | Overview | cold-startup | 2 | 38.3 | 42.9 | 42.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 136.3 | 136.3 | 136.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 42.7 | 42.7 | 42.7 |
| 2 | Code-heavy | cold-startup | 1 | 2796 | 2796 | 2796 |
| 1 | Code-heavy | cold-startup | 1 | 149.9 | 149.9 | 149.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 932.3 | 932.3 | 932.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 134.7 | 134.7 | 134.7 |

## first.sharedBrowserHeap

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 7399560 | 7400799 | 7400799 |
| 0 | Small | cold-startup | 2 | 7348304 | 7387325 | 7387325 |
| 2 | Small | cold-startup | 2 | 7394350 | 7400061 | 7400061 |
| D | Small | cold-startup | 2 | 7384766 | 7424098 | 7424098 |
| D | Small | fresh-profile-after-examples-settled | 2 | 7382520 | 7403936 | 7403936 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 7398946 | 7403490 | 7403490 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 7399948 | 7406276 | 7406276 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 7398461 | 7428939 | 7428939 |
| 0 | Overview | cold-startup | 2 | 7759471 | 7773439 | 7773439 |
| 2 | Overview | cold-startup | 2 | 7765718 | 7778539 | 7778539 |
| 1 | Overview | cold-startup | 2 | 7778638 | 7806035 | 7806035 |
| D | Overview | cold-startup | 2 | 7727186 | 7739900 | 7739900 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 7710712 | 7710712 | 7710712 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 7740727 | 7740727 | 7740727 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 7740163 | 7740163 | 7740163 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 7760405 | 7760405 | 7760405 |
| 2 | Code-heavy | cold-startup | 1 | 9379534 | 9379534 | 9379534 |
| 1 | Code-heavy | cold-startup | 1 | 8848405 | 8848405 | 8848405 |
| 0 | Code-heavy | cold-startup | 1 | 9307821 | 9307821 | 9307821 |
| D | Code-heavy | cold-startup | 1 | 7899347 | 7899347 | 7899347 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 7885455 | 7885455 | 7885455 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 9848766 | 9848766 | 9848766 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 9305727 | 9305727 | 9305727 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 8691328 | 8691328 | 8691328 |

## second.visibleMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 114.1 | 130.3 | 130.3 |
| 0 | Small | cold-startup | 2 | 132.4 | 145.6 | 145.6 |
| 2 | Small | cold-startup | 2 | 114.4 | 130.1 | 130.1 |
| D | Small | cold-startup | 2 | 130.6 | 130.7 | 130.7 |
| D | Small | fresh-profile-after-examples-settled | 2 | 133.2 | 146.8 | 146.8 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 114.5 | 129.5 | 129.5 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 113.5 | 130.3 | 130.3 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 129.5 | 146.4 | 146.4 |
| 0 | Overview | cold-startup | 2 | 118.7 | 137.7 | 137.7 |
| 2 | Overview | cold-startup | 2 | 119.1 | 136.5 | 136.5 |
| 1 | Overview | cold-startup | 2 | 118.7 | 119.3 | 119.3 |
| D | Overview | cold-startup | 2 | 122 | 139.2 | 139.2 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 122 | 122 | 122 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 119.8 | 119.8 | 119.8 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 120.2 | 120.2 | 120.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 120.6 | 120.6 | 120.6 |
| 2 | Code-heavy | cold-startup | 1 | 489.6 | 489.6 | 489.6 |
| 1 | Code-heavy | cold-startup | 1 | 151.1 | 151.1 | 151.1 |
| 0 | Code-heavy | cold-startup | 1 | 179.7 | 179.7 | 179.7 |
| D | Code-heavy | cold-startup | 1 | 155.9 | 155.9 | 155.9 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 133.2 | 133.2 | 133.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 142.7 | 142.7 | 142.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 173.4 | 173.4 | 173.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 153.5 | 153.5 | 153.5 |

## second.visibleAndColouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 114.1 | 131.2 | 131.2 |
| 0 | Small | cold-startup | 2 | 132.4 | 145.6 | 145.6 |
| 2 | Small | cold-startup | 2 | 114.4 | 131.1 | 131.1 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 114.5 | 130.2 | 130.2 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 114.4 | 131.3 | 131.3 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 129.5 | 146.4 | 146.4 |
| 2 | Code-heavy | cold-startup | 1 | 5370 | 5370 | 5370 |
| 1 | Code-heavy | cold-startup | 1 | 551.7 | 551.7 | 551.7 |
| 0 | Code-heavy | cold-startup | 1 | 862.5 | 862.5 | 862.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1559.4 | 1559.4 | 1559.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 924.4 | 924.4 | 924.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 554 | 554 | 554 |

## second.colouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 98.9 | 131.2 | 131.2 |
| 0 | Small | cold-startup | 2 | 101.2 | 115.7 | 115.7 |
| 2 | Small | cold-startup | 2 | 97.7 | 131.1 | 131.1 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 97.9 | 130.2 | 130.2 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 114.4 | 131.3 | 131.3 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 99 | 115.4 | 115.4 |
| 2 | Code-heavy | cold-startup | 1 | 5370 | 5370 | 5370 |
| 1 | Code-heavy | cold-startup | 1 | 551.7 | 551.7 | 551.7 |
| 0 | Code-heavy | cold-startup | 1 | 862.5 | 862.5 | 862.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1559.4 | 1559.4 | 1559.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 924.4 | 924.4 | 924.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 554 | 554 | 554 |

## second.longTaskMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## second.loaderMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 302.4 | 302.4 | 302.4 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 24.2 | 24.2 | 24.2 |
| D | Code-heavy | cold-startup | 1 | 11.3 | 11.3 | 11.3 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 18.8 | 18.8 | 18.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 12 | 12 | 12 |

## second.documentRequestMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 1.5 | 1.8 | 1.8 |
| 0 | Small | cold-startup | 2 | 1.7 | 2.3 | 2.3 |
| 2 | Small | cold-startup | 2 | 1.5 | 1.9 | 1.9 |
| D | Small | cold-startup | 2 | 1.4 | 1.8 | 1.8 |
| D | Small | fresh-profile-after-examples-settled | 2 | 1.8 | 3.1 | 3.1 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 1.5 | 1.7 | 1.7 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 1.3 | 1.9 | 1.9 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 1.5 | 2.3 | 2.3 |
| 0 | Overview | cold-startup | 2 | 1.7 | 2.9 | 2.9 |
| 2 | Overview | cold-startup | 2 | 1.5 | 1.8 | 1.8 |
| 1 | Overview | cold-startup | 2 | 1.3 | 1.6 | 1.6 |
| D | Overview | cold-startup | 2 | 1.4 | 1.8 | 1.8 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 2 | 2 | 2 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 1.9 | 1.9 | 1.9 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 1.5 | 1.5 | 1.5 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 1.4 | 1.4 | 1.4 |
| 2 | Code-heavy | cold-startup | 1 | 5.4 | 5.4 | 5.4 |
| 1 | Code-heavy | cold-startup | 1 | 3.4 | 3.4 | 3.4 |
| 0 | Code-heavy | cold-startup | 1 | 5.9 | 5.9 | 5.9 |
| D | Code-heavy | cold-startup | 1 | 3.3 | 3.3 | 3.3 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 4.1 | 4.1 | 4.1 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 3.5 | 3.5 | 3.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 5.3 | 5.3 | 5.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 3.5 | 3.5 | 3.5 |

## second.bytesTransferred

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 174898 | 174898 | 174898 |
| 0 | Small | cold-startup | 2 | 89223 | 89223 | 89223 |
| 2 | Small | cold-startup | 2 | 664615 | 664615 | 664615 |
| D | Small | cold-startup | 2 | 85340 | 85340 | 85340 |
| D | Small | fresh-profile-after-examples-settled | 2 | 85340 | 85340 | 85340 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 664615 | 664615 | 664615 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 174898 | 174898 | 174898 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 89223 | 89223 | 89223 |
| 0 | Overview | cold-startup | 2 | 163655 | 163655 | 163655 |
| 2 | Overview | cold-startup | 2 | 164048 | 164048 | 164048 |
| 1 | Overview | cold-startup | 2 | 164048 | 164048 | 164048 |
| D | Overview | cold-startup | 2 | 160376 | 160376 | 160376 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 160376 | 160376 | 160376 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 164048 | 164048 | 164048 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 163655 | 163655 | 163655 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 164048 | 164048 | 164048 |
| 2 | Code-heavy | cold-startup | 1 | 141342 | 141342 | 141342 |
| 1 | Code-heavy | cold-startup | 1 | 217669 | 217669 | 217669 |
| 0 | Code-heavy | cold-startup | 1 | 633068 | 633068 | 633068 |
| D | Code-heavy | cold-startup | 1 | 128111 | 128111 | 128111 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 128111 | 128111 | 128111 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4562943 | 4562943 | 4562943 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 633068 | 633068 | 633068 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 217669 | 217669 | 217669 |

## second.bytesDecoded

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 293057 | 293057 | 293057 |
| 0 | Small | cold-startup | 2 | 210982 | 210982 | 210982 |
| 2 | Small | cold-startup | 2 | 784274 | 784274 | 784274 |
| D | Small | cold-startup | 2 | 207399 | 207399 | 207399 |
| D | Small | fresh-profile-after-examples-settled | 2 | 207399 | 207399 | 207399 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 784274 | 784274 | 784274 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 293057 | 293057 | 293057 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 210982 | 210982 | 210982 |
| 0 | Overview | cold-startup | 2 | 232929 | 232929 | 232929 |
| 2 | Overview | cold-startup | 2 | 233322 | 233322 | 233322 |
| 1 | Overview | cold-startup | 2 | 233322 | 233322 | 233322 |
| D | Overview | cold-startup | 2 | 229650 | 229650 | 229650 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 229650 | 229650 | 229650 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 233322 | 233322 | 233322 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 232929 | 232929 | 232929 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 233322 | 233322 | 233322 |
| 2 | Code-heavy | cold-startup | 1 | 262801 | 262801 | 262801 |
| 1 | Code-heavy | cold-startup | 1 | 335828 | 335828 | 335828 |
| 0 | Code-heavy | cold-startup | 1 | 754827 | 754827 | 754827 |
| D | Code-heavy | cold-startup | 1 | 250170 | 250170 | 250170 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 250170 | 250170 | 250170 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4662802 | 4662802 | 4662802 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 754827 | 754827 | 754827 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 335828 | 335828 | 335828 |

## second.template-parseMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0.1 | 0.1 |
| 0 | Small | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.2 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.1 | 0.1 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 13.4 | 13.4 | 13.4 |
| 0 | Code-heavy | cold-startup | 1 | 11 | 11 | 11 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 19.7 | 19.7 | 19.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 10.899 | 10.899 | 10.899 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 16.201 | 16.201 | 16.201 |

## second.code-insertMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0.1 | 0.1 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0.1 | 0.1 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0.1 | 0.1 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 4.001 | 4.001 | 4.001 |
| 0 | Code-heavy | cold-startup | 1 | 4.5 | 4.5 | 4.5 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 6.399 | 6.399 | 6.399 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4 | 4 | 4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4.099 | 4.099 | 4.099 |

## second.longTaskMaxMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## second.initializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 19.2 | 23.3 | 23.3 |
| 2 | Small | cold-startup | 2 | 17.3 | 19.4 | 19.4 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 17.3 | 19.5 | 19.5 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 22.1 | 23.1 | 23.1 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 22.5 | 22.5 | 22.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 19.3 | 19.3 | 19.3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 22.8 | 22.8 | 22.8 |

## second.grammarInitializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 2 | 2.5 | 2.5 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 2.6 | 2.8 | 2.8 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 86.2 | 86.2 | 86.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## second.tokenizationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 2.7 | 2.8 | 2.8 |
| 2 | Small | cold-startup | 2 | 4.8 | 5.7 | 5.7 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 4.9 | 6.1 | 6.1 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 2.6 | 2.8 | 2.8 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 92 | 92 | 92 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1009.5 | 1009.5 | 1009.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 97 | 97 | 97 |

## second.totalWorkerMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 28.2 | 33.8 | 33.8 |
| 2 | Small | cold-startup | 2 | 30.5 | 35.2 | 35.2 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 31.1 | 35.8 | 35.8 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 32.4 | 32.9 | 32.9 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 122.6 | 122.6 | 122.6 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1125.1 | 1125.1 | 1125.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 129.5 | 129.5 | 129.5 |

## second.sharedBrowserHeap

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 7475223 | 7486510 | 7486510 |
| 0 | Small | cold-startup | 2 | 7425737 | 7462587 | 7462587 |
| 2 | Small | cold-startup | 2 | 7474148 | 7489894 | 7489894 |
| D | Small | cold-startup | 2 | 7402830 | 7411458 | 7411458 |
| D | Small | fresh-profile-after-examples-settled | 2 | 7412475 | 7433832 | 7433832 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 7514634 | 7520569 | 7520569 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 7505768 | 7522176 | 7522176 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 7462907 | 7479913 | 7479913 |
| 0 | Overview | cold-startup | 2 | 7772532 | 7790525 | 7790525 |
| 2 | Overview | cold-startup | 2 | 7935582 | 7964226 | 7964226 |
| 1 | Overview | cold-startup | 2 | 7966230 | 7973409 | 7973409 |
| D | Overview | cold-startup | 2 | 7685243 | 7709261 | 7709261 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 7769641 | 7769641 | 7769641 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 7909198 | 7909198 | 7909198 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 7818929 | 7818929 | 7818929 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 7837180 | 7837180 | 7837180 |
| 2 | Code-heavy | cold-startup | 1 | 10113956 | 10113956 | 10113956 |
| 1 | Code-heavy | cold-startup | 1 | 10161739 | 10161739 | 10161739 |
| 0 | Code-heavy | cold-startup | 1 | 10912817 | 10912817 | 10912817 |
| D | Code-heavy | cold-startup | 1 | 8450942 | 8450942 | 8450942 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 8455691 | 8455691 | 8455691 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 12315242 | 12315242 | 12315242 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 10827399 | 10827399 | 10827399 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 8023393 | 8023393 | 8023393 |

## retained.visibleMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 48.6 | 48.7 | 48.7 |
| 0 | Small | cold-startup | 2 | 49 | 50.1 | 50.1 |
| 2 | Small | cold-startup | 2 | 48.8 | 49 | 49 |
| D | Small | cold-startup | 2 | 49.2 | 49.4 | 49.4 |
| D | Small | fresh-profile-after-examples-settled | 2 | 48.3 | 49.1 | 49.1 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 48.7 | 48.8 | 48.8 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 48.4 | 48.7 | 48.7 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 47.8 | 48.1 | 48.1 |
| 0 | Overview | cold-startup | 2 | 49 | 49.3 | 49.3 |
| 2 | Overview | cold-startup | 2 | 48.7 | 48.9 | 48.9 |
| 1 | Overview | cold-startup | 2 | 48.2 | 49.3 | 49.3 |
| D | Overview | cold-startup | 2 | 49 | 49.9 | 49.9 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 49 | 49 | 49 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 49.3 | 49.3 | 49.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 48.9 | 48.9 | 48.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 49.7 | 49.7 | 49.7 |
| 2 | Code-heavy | cold-startup | 1 | 54.3 | 54.3 | 54.3 |
| 1 | Code-heavy | cold-startup | 1 | 45.4 | 45.4 | 45.4 |
| 0 | Code-heavy | cold-startup | 1 | 44.9 | 44.9 | 44.9 |
| D | Code-heavy | cold-startup | 1 | 46.1 | 46.1 | 46.1 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 47.6 | 47.6 | 47.6 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 44.4 | 44.4 | 44.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 45.5 | 45.5 | 45.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 46.2 | 46.2 | 46.2 |

## retained.longTaskMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.loaderMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.bytesTransferred

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.bytesDecoded

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.template-parseMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.code-insertMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.longTaskMaxMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.initializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.grammarInitializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.tokenizationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.totalWorkerMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## retained.sharedBrowserHeap

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 7768410 | 7774297 | 7774297 |
| 0 | Small | cold-startup | 2 | 7710109 | 7749834 | 7749834 |
| 2 | Small | cold-startup | 2 | 7755711 | 7764325 | 7764325 |
| D | Small | cold-startup | 2 | 7700401 | 8642921 | 8642921 |
| D | Small | fresh-profile-after-examples-settled | 2 | 7712462 | 7724963 | 7724963 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 7774845 | 8562913 | 8562913 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 8561807 | 8578371 | 8578371 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 7733738 | 7736348 | 7736348 |
| 0 | Overview | cold-startup | 2 | 8120992 | 8138671 | 8138671 |
| 2 | Overview | cold-startup | 2 | 8044071 | 8158200 | 8158200 |
| 1 | Overview | cold-startup | 2 | 8055183 | 8374470 | 8374470 |
| D | Overview | cold-startup | 2 | 8036964 | 8067366 | 8067366 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 8147534 | 8147534 | 8147534 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 8269428 | 8269428 | 8269428 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 8009150 | 8009150 | 8009150 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 8233213 | 8233213 | 8233213 |
| 2 | Code-heavy | cold-startup | 1 | 11512580 | 11512580 | 11512580 |
| 1 | Code-heavy | cold-startup | 1 | 11456731 | 11456731 | 11456731 |
| 0 | Code-heavy | cold-startup | 1 | 11975161 | 11975161 | 11975161 |
| D | Code-heavy | cold-startup | 1 | 8694188 | 8694188 | 8694188 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 8925890 | 8925890 | 8925890 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 11860376 | 11860376 | 11860376 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 11889069 | 11889069 | 11889069 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 9324101 | 9324101 | 9324101 |

## evicted.visibleMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 131.7 | 131.9 | 131.9 |
| 0 | Small | cold-startup | 2 | 150.3 | 301.5 | 301.5 |
| 2 | Small | cold-startup | 2 | 115.3 | 131.9 | 131.9 |
| D | Small | cold-startup | 2 | 132 | 165 | 165 |
| D | Small | fresh-profile-after-examples-settled | 2 | 114.5 | 148.4 | 148.4 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 115.1 | 148.5 | 148.5 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 132.1 | 148.6 | 148.6 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 148.7 | 165.1 | 165.1 |
| 0 | Overview | cold-startup | 2 | 130.3 | 131.9 | 131.9 |
| 2 | Overview | cold-startup | 2 | 116 | 132.8 | 132.8 |
| 1 | Overview | cold-startup | 2 | 131.7 | 132 | 132 |
| D | Overview | cold-startup | 2 | 114.7 | 998.1 | 998.1 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 132.5 | 132.5 | 132.5 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 131.1 | 131.1 | 131.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 148 | 148 | 148 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 196.6 | 196.6 | 196.6 |
| 2 | Code-heavy | cold-startup | 1 | 417.1 | 417.1 | 417.1 |
| 1 | Code-heavy | cold-startup | 1 | 148 | 148 | 148 |
| 0 | Code-heavy | cold-startup | 1 | 130.8 | 130.8 | 130.8 |
| D | Code-heavy | cold-startup | 1 | 165 | 165 | 165 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 160.5 | 160.5 | 160.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 131.9 | 131.9 | 131.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 132.1 | 132.1 | 132.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 131.8 | 131.8 | 131.8 |

## evicted.longTaskMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 52 | 52 | 52 |
| 1 | Code-heavy | cold-startup | 1 | 57 | 57 | 57 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 149 | 149 | 149 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 64 | 64 | 64 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 121 | 121 | 121 |

## evicted.loaderMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 165.8 | 165.8 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 17.3 | 17.3 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 5.3 | 5.3 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 22.7 | 22.7 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 854.1 | 854.1 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 50.7 | 50.7 | 50.7 |
| 2 | Code-heavy | cold-startup | 1 | 227.1 | 227.1 | 227.1 |
| 1 | Code-heavy | cold-startup | 1 | 6.1 | 6.1 | 6.1 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 11.9 | 11.9 | 11.9 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 23.6 | 23.6 | 23.6 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## evicted.documentRequestMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 1.3 | 1.8 | 1.8 |
| 0 | Small | cold-startup | 2 | 1.8 | 1.8 | 1.8 |
| 2 | Small | cold-startup | 2 | 1.4 | 1.6 | 1.6 |
| D | Small | cold-startup | 2 | 1.2 | 2.7 | 2.7 |
| D | Small | fresh-profile-after-examples-settled | 2 | 1.5 | 1.7 | 1.7 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 1.4 | 2.1 | 2.1 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 1.4 | 1.7 | 1.7 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 1.9 | 2.5 | 2.5 |
| 0 | Overview | cold-startup | 2 | 1.4 | 1.7 | 1.7 |
| 2 | Overview | cold-startup | 2 | 1.4 | 1.5 | 1.5 |
| 1 | Overview | cold-startup | 2 | 1.3 | 1.4 | 1.4 |
| D | Overview | cold-startup | 2 | 1.5 | 1.8 | 1.8 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 1.5 | 1.5 | 1.5 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 2.8 | 2.8 | 2.8 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 1.9 | 1.9 | 1.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 4.6 | 4.6 | 4.6 |
| 2 | Code-heavy | cold-startup | 1 | 5.4 | 5.4 | 5.4 |
| 1 | Code-heavy | cold-startup | 1 | 3.1 | 3.1 | 3.1 |
| 0 | Code-heavy | cold-startup | 1 | 2.8 | 2.8 | 2.8 |
| D | Code-heavy | cold-startup | 1 | 3.9 | 3.9 | 3.9 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 4.4 | 4.4 | 4.4 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 3 | 3 | 3 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 3 | 3 | 3 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 3.1 | 3.1 | 3.1 |

## evicted.bytesTransferred

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 85708 | 85708 | 85708 |
| 0 | Small | cold-startup | 2 | 85315 | 85315 | 85315 |
| 2 | Small | cold-startup | 2 | 85708 | 85708 | 85708 |
| D | Small | cold-startup | 2 | 82036 | 82036 | 82036 |
| D | Small | fresh-profile-after-examples-settled | 2 | 82036 | 82036 | 82036 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 85708 | 85708 | 85708 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 85708 | 85708 | 85708 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 85315 | 85315 | 85315 |
| 0 | Overview | cold-startup | 2 | 155807 | 155807 | 155807 |
| 2 | Overview | cold-startup | 2 | 1040909 | 1040909 | 1040909 |
| 1 | Overview | cold-startup | 2 | 240203 | 240203 | 240203 |
| D | Overview | cold-startup | 2 | 150645 | 150645 | 150645 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 150645 | 150645 | 150645 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 1040909 | 1040909 | 1040909 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 155807 | 155807 | 155807 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 240203 | 240203 | 240203 |
| 2 | Code-heavy | cold-startup | 1 | 95267 | 95267 | 95267 |
| 1 | Code-heavy | cold-startup | 1 | 171594 | 171594 | 171594 |
| 0 | Code-heavy | cold-startup | 1 | 586993 | 586993 | 586993 |
| D | Code-heavy | cold-startup | 1 | 82036 | 82036 | 82036 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 82036 | 82036 | 82036 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4516868 | 4516868 | 4516868 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 586993 | 586993 | 586993 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 171594 | 171594 | 171594 |

## evicted.bytesDecoded

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 211065 | 211065 | 211065 |
| 0 | Small | cold-startup | 2 | 210672 | 210672 | 210672 |
| 2 | Small | cold-startup | 2 | 211065 | 211065 | 211065 |
| D | Small | cold-startup | 2 | 207393 | 207393 | 207393 |
| D | Small | fresh-profile-after-examples-settled | 2 | 207393 | 207393 | 207393 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 211065 | 211065 | 211065 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 211065 | 211065 | 211065 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 210672 | 210672 | 210672 |
| 0 | Overview | cold-startup | 2 | 247675 | 247675 | 247675 |
| 2 | Overview | cold-startup | 2 | 1130077 | 1130077 | 1130077 |
| 1 | Overview | cold-startup | 2 | 328471 | 328471 | 328471 |
| D | Overview | cold-startup | 2 | 242813 | 242813 | 242813 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 242813 | 242813 | 242813 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 1130077 | 1130077 | 1130077 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 247675 | 247675 | 247675 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 328471 | 328471 | 328471 |
| 2 | Code-heavy | cold-startup | 1 | 262795 | 262795 | 262795 |
| 1 | Code-heavy | cold-startup | 1 | 335822 | 335822 | 335822 |
| 0 | Code-heavy | cold-startup | 1 | 754821 | 754821 | 754821 |
| D | Code-heavy | cold-startup | 1 | 250164 | 250164 | 250164 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 250164 | 250164 | 250164 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4662796 | 4662796 | 4662796 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 754821 | 754821 | 754821 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 335822 | 335822 | 335822 |

## evicted.template-parseMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0.1 | 0.2 | 0.2 |
| 2 | Overview | cold-startup | 2 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 2 | 0.1 | 0.2 | 0.2 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0.3 | 0.3 | 0.3 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 11.6 | 11.6 | 11.6 |
| 0 | Code-heavy | cold-startup | 1 | 10.3 | 10.3 | 10.3 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 16.699 | 16.699 | 16.699 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 10.399 | 10.399 | 10.399 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 15.1 | 15.1 | 15.1 |

## evicted.code-insertMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0.1 | 0.1 |
| 2 | Overview | cold-startup | 2 | 0 | 0.1 | 0.1 |
| 1 | Overview | cold-startup | 2 | 0 | 0.1 | 0.1 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 3.801 | 3.801 | 3.801 |
| 0 | Code-heavy | cold-startup | 1 | 4.5 | 4.5 | 4.5 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 7.4 | 7.4 | 7.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4.101 | 4.101 | 4.101 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 4.6 | 4.6 | 4.6 |

## evicted.longTaskMaxMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 52 | 52 | 52 |
| 1 | Code-heavy | cold-startup | 1 | 57 | 57 | 57 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 149 | 149 | 149 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 64 | 64 | 64 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 121 | 121 | 121 |

## evicted.initializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 16 | 18.5 | 18.5 |
| 1 | Overview | cold-startup | 2 | 21.8 | 23.3 | 23.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 22.2 | 22.2 | 22.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 41.3 | 41.3 | 41.3 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 21.3 | 21.3 | 21.3 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 17 | 17 | 17 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 22.9 | 22.9 | 22.9 |

## evicted.grammarInitializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 7.5 | 7.7 | 7.7 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 9.6 | 9.6 | 9.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 112.8 | 112.8 | 112.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## evicted.tokenizationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 40.7 | 41.2 | 41.2 |
| 1 | Overview | cold-startup | 2 | 7.1 | 7.1 | 7.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 43 | 43 | 43 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 7.9 | 7.9 | 7.9 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 83.6 | 83.6 | 83.6 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 789.4 | 789.4 | 789.4 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 84.2 | 84.2 | 84.2 |

## evicted.totalWorkerMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 71.1 | 72.8 | 72.8 |
| 1 | Overview | cold-startup | 2 | 36.1 | 36.7 | 36.7 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 81.4 | 81.4 | 81.4 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 62 | 62 | 62 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 119.2 | 119.2 | 119.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 932 | 932 | 932 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 121.9 | 121.9 | 121.9 |

## evicted.sharedBrowserHeap

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 10502075 | 24115484 | 24115484 |
| 0 | Small | cold-startup | 2 | 8210444 | 10474833 | 10474833 |
| 2 | Small | cold-startup | 2 | 10222802 | 10657780 | 10657780 |
| D | Small | cold-startup | 2 | 10433672 | 10434596 | 10434596 |
| D | Small | fresh-profile-after-examples-settled | 2 | 10444435 | 23901736 | 23901736 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 10524833 | 24096655 | 24096655 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 10485697 | 24525929 | 24525929 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 24013116 | 24025750 | 24025750 |
| 0 | Overview | cold-startup | 2 | 9041555 | 24429020 | 24429020 |
| 2 | Overview | cold-startup | 2 | 10054522 | 24918912 | 24918912 |
| 1 | Overview | cold-startup | 2 | 24845857 | 24876874 | 24876874 |
| D | Overview | cold-startup | 2 | 10221092 | 24286084 | 24286084 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 8212472 | 8212472 | 8212472 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 9528675 | 9528675 | 9528675 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 8291173 | 8291173 | 8291173 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 25110936 | 25110936 | 25110936 |
| 2 | Code-heavy | cold-startup | 1 | 10149974 | 10149974 | 10149974 |
| 1 | Code-heavy | cold-startup | 1 | 17916335 | 17916335 | 17916335 |
| 0 | Code-heavy | cold-startup | 1 | 24959158 | 24959158 | 24959158 |
| D | Code-heavy | cold-startup | 1 | 12105044 | 12105044 | 12105044 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 12361971 | 12361971 | 12361971 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 14788795 | 14788795 | 14788795 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 28591793 | 28591793 | 28591793 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 60022878 | 60022878 | 60022878 |

## sourceFirst.readableMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 14.5 | 15.1 | 15.1 |
| 0 | Small | cold-startup | 2 | 13.4 | 16.6 | 16.6 |
| 2 | Small | cold-startup | 2 | 11.3 | 13.4 | 13.4 |
| D | Small | cold-startup | 2 | 14.6 | 15.2 | 15.2 |
| D | Small | fresh-profile-after-examples-settled | 2 | 13.5 | 14.8 | 14.8 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 12.1 | 14.4 | 14.4 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 13 | 14.6 | 14.6 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 14.4 | 16 | 16 |
| 0 | Overview | cold-startup | 2 | 13.7 | 13.8 | 13.8 |
| 2 | Overview | cold-startup | 2 | 11.2 | 22 | 22 |
| 1 | Overview | cold-startup | 2 | 11.2 | 11.6 | 11.6 |
| D | Overview | cold-startup | 2 | 13.9 | 14.6 | 14.6 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 15.1 | 15.1 | 15.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 19.7 | 19.7 | 19.7 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 12.6 | 12.6 | 12.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 8.8 | 8.8 | 8.8 |
| 2 | Code-heavy | cold-startup | 1 | 25 | 25 | 25 |
| 1 | Code-heavy | cold-startup | 1 | 6.8 | 6.8 | 6.8 |
| 0 | Code-heavy | cold-startup | 1 | 27.2 | 27.2 | 27.2 |
| D | Code-heavy | cold-startup | 1 | 25.7 | 25.7 | 25.7 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 11.3 | 11.3 | 11.3 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 6.7 | 6.7 | 6.7 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 73.7 | 73.7 | 73.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 8 | 8 | 8 |

## sourceFirst.template-parseMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 0 | Small | cold-startup | 2 | 0.1 | 0.2 | 0.2 |
| 2 | Small | cold-startup | 2 | 0.1 | 0.2 | 0.2 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.1 | 0.1 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.2 | 0.2 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.2 | 0.2 |
| 0 | Overview | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 2 | Overview | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | cold-startup | 2 | 0 | 0.1 | 0.1 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 2 | Code-heavy | cold-startup | 1 | 3.4 | 3.4 | 3.4 |
| 1 | Code-heavy | cold-startup | 1 | 2.2 | 2.2 | 2.2 |
| 0 | Code-heavy | cold-startup | 1 | 2 | 2 | 2 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 3.1 | 3.1 | 3.1 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 2.2 | 2.2 | 2.2 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 2.1 | 2.1 | 2.1 |

## sourceFirst.code-insertMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 0 | Small | cold-startup | 2 | 0 | 0.1 | 0.1 |
| 2 | Small | cold-startup | 2 | 0 | 0.1 | 0.1 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.1 | 0.1 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0.1 | 0.2 | 0.2 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 2 | Code-heavy | cold-startup | 1 | 1.3 | 1.3 | 1.3 |
| 1 | Code-heavy | cold-startup | 1 | 0.6 | 0.6 | 0.6 |
| 0 | Code-heavy | cold-startup | 1 | 0.6 | 0.6 | 0.6 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0.9 | 0.9 | 0.9 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0.6 | 0.6 | 0.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0.6 | 0.6 | 0.6 |

## sourceFirst.longTaskMaxMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 50 | 50 | 50 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceFirst.bytesTransferred

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 87162 | 87162 | 87162 |
| 0 | Small | cold-startup | 2 | 1540 | 1540 | 1540 |
| 2 | Small | cold-startup | 2 | 766934 | 766934 | 766934 |
| D | Small | cold-startup | 2 | 638 | 638 | 638 |
| D | Small | fresh-profile-after-examples-settled | 2 | 638 | 638 | 638 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 766934 | 766934 | 766934 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 87162 | 87162 | 87162 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 1540 | 1540 | 1540 |
| 0 | Overview | cold-startup | 2 | 4697 | 4697 | 4697 |
| 2 | Overview | cold-startup | 2 | 198727 | 198727 | 198727 |
| 1 | Overview | cold-startup | 2 | 4448 | 4448 | 4448 |
| D | Overview | cold-startup | 2 | 2224 | 2224 | 2224 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 2224 | 2224 | 2224 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 198727 | 198727 | 198727 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 4697 | 4697 | 4697 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 4448 | 4448 | 4448 |
| 2 | Code-heavy | cold-startup | 1 | 200228 | 200228 | 200228 |
| 1 | Code-heavy | cold-startup | 1 | 200228 | 200228 | 200228 |
| 0 | Code-heavy | cold-startup | 1 | 236384 | 236384 | 236384 |
| D | Code-heavy | cold-startup | 1 | 100114 | 100114 | 100114 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 100114 | 100114 | 100114 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 200228 | 200228 | 200228 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 236384 | 236384 | 236384 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 200228 | 200228 | 200228 |

## sourceFirst.bytesDecoded

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 82662 | 82662 | 82662 |
| 0 | Small | cold-startup | 2 | 940 | 940 | 940 |
| 2 | Small | cold-startup | 2 | 763934 | 763934 | 763934 |
| D | Small | cold-startup | 2 | 338 | 338 | 338 |
| D | Small | fresh-profile-after-examples-settled | 2 | 338 | 338 | 338 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 763934 | 763934 | 763934 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 82662 | 82662 | 82662 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 940 | 940 | 940 |
| 0 | Overview | cold-startup | 2 | 4097 | 4097 | 4097 |
| 2 | Overview | cold-startup | 2 | 197827 | 197827 | 197827 |
| 1 | Overview | cold-startup | 2 | 3848 | 3848 | 3848 |
| D | Overview | cold-startup | 2 | 1924 | 1924 | 1924 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 1924 | 1924 | 1924 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 197827 | 197827 | 197827 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 4097 | 4097 | 4097 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 3848 | 3848 | 3848 |
| 2 | Code-heavy | cold-startup | 1 | 199628 | 199628 | 199628 |
| 1 | Code-heavy | cold-startup | 1 | 199628 | 199628 | 199628 |
| 0 | Code-heavy | cold-startup | 1 | 235784 | 235784 | 235784 |
| D | Code-heavy | cold-startup | 1 | 99814 | 99814 | 99814 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 99814 | 99814 | 99814 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 199628 | 199628 | 199628 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 235784 | 235784 | 235784 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 199628 | 199628 | 199628 |

## sourceFirst.colouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 73.3 | 75.2 | 75.2 |
| 0 | Small | cold-startup | 2 | 60.3 | 82.3 | 82.3 |
| 2 | Small | cold-startup | 2 | 154.1 | 164.7 | 164.7 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 157.7 | 169.5 | 169.5 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 77.5 | 88.6 | 88.6 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 59.5 | 68.3 | 68.3 |
| 0 | Overview | cold-startup | 2 | 56.3 | 63.1 | 63.1 |
| 2 | Overview | cold-startup | 2 | 103.6 | 107.6 | 107.6 |
| 1 | Overview | cold-startup | 2 | 39.5 | 39.9 | 39.9 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 116.7 | 116.7 | 116.7 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 59.7 | 59.7 | 59.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 36.4 | 36.4 | 36.4 |
| 2 | Code-heavy | cold-startup | 1 | 237.8 | 237.8 | 237.8 |
| 1 | Code-heavy | cold-startup | 1 | 41.9 | 41.9 | 41.9 |
| 0 | Code-heavy | cold-startup | 1 | 147.6 | 147.6 | 147.6 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 83.5 | 83.5 | 83.5 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 200.6 | 200.6 | 200.6 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 42.6 | 42.6 | 42.6 |

## sourceFirst.initializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 20.5 | 21.8 | 21.8 |
| 2 | Small | cold-startup | 2 | 17.3 | 20.4 | 20.4 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 18 | 23.1 | 23.1 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 22 | 31.2 | 31.2 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceFirst.grammarInitializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 4.4 | 4.7 | 4.7 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 4.7 | 5.6 | 5.6 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 2.7 | 3.5 | 3.5 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 3.3 | 3.3 | 3.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceFirst.tokenizationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 7.8 | 8.1 | 8.1 |
| 2 | Small | cold-startup | 2 | 88.4 | 91.4 | 91.4 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 89.6 | 93.8 | 93.8 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 7.8 | 8.1 | 8.1 |
| 2 | Overview | cold-startup | 2 | 69.1 | 76.1 | 76.1 |
| 1 | Overview | cold-startup | 2 | 6.5 | 7 | 7 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 71.3 | 71.3 | 71.3 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 6.4 | 6.4 | 6.4 |
| 2 | Code-heavy | cold-startup | 1 | 193.6 | 193.6 | 193.6 |
| 1 | Code-heavy | cold-startup | 1 | 7.5 | 7.5 | 7.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 46.8 | 46.8 | 46.8 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 8 | 8 | 8 |

## sourceFirst.totalWorkerMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 35.3 | 37.2 | 37.2 |
| 2 | Small | cold-startup | 2 | 117.6 | 125.6 | 125.6 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 119.2 | 132.6 | 132.6 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 37.6 | 50 | 50 |
| 2 | Overview | cold-startup | 2 | 72 | 80 | 80 |
| 1 | Overview | cold-startup | 2 | 7 | 7.2 | 7.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 75.2 | 75.2 | 75.2 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 6.8 | 6.8 | 6.8 |
| 2 | Code-heavy | cold-startup | 1 | 196.8 | 196.8 | 196.8 |
| 1 | Code-heavy | cold-startup | 1 | 8.5 | 8.5 | 8.5 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 48.7 | 48.7 | 48.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 8.9 | 8.9 | 8.9 |

## sourceReopen.readableMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 15.3 | 16 | 16 |
| 0 | Small | cold-startup | 2 | 14.9 | 16.1 | 16.1 |
| 2 | Small | cold-startup | 2 | 14.9 | 15.4 | 15.4 |
| D | Small | cold-startup | 2 | 14.5 | 15.5 | 15.5 |
| D | Small | fresh-profile-after-examples-settled | 2 | 15.1 | 15.7 | 15.7 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 15.8 | 16.5 | 16.5 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 15.2 | 16 | 16 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 15.1 | 16.1 | 16.1 |
| 0 | Overview | cold-startup | 2 | 15.7 | 15.7 | 15.7 |
| 2 | Overview | cold-startup | 2 | 14.7 | 16.5 | 16.5 |
| 1 | Overview | cold-startup | 2 | 15.9 | 17 | 17 |
| D | Overview | cold-startup | 2 | 14.3 | 15.3 | 15.3 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 15.2 | 15.2 | 15.2 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 15.5 | 15.5 | 15.5 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 15.6 | 15.6 | 15.6 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 15.3 | 15.3 | 15.3 |
| 2 | Code-heavy | cold-startup | 1 | 11.8 | 11.8 | 11.8 |
| 1 | Code-heavy | cold-startup | 1 | 13.5 | 13.5 | 13.5 |
| 0 | Code-heavy | cold-startup | 1 | 13.5 | 13.5 | 13.5 |
| D | Code-heavy | cold-startup | 1 | 14 | 14 | 14 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 14.4 | 14.4 | 14.4 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 13.4 | 13.4 | 13.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 14.1 | 14.1 | 14.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 13.5 | 13.5 | 13.5 |

## sourceReopen.template-parseMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceReopen.code-insertMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceReopen.longTaskMaxMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceReopen.bytesTransferred

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceReopen.bytesDecoded

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceReopen.colouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0.1 | 0.2 | 0.2 |
| 0 | Small | cold-startup | 2 | 0 | 0.2 | 0.2 |
| 2 | Small | cold-startup | 2 | 0 | 0.1 | 0.1 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.2 | 0.2 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0.1 | 0.1 | 0.1 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0.1 | 0.1 |
| 0 | Overview | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 2 | Overview | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | cold-startup | 2 | 0.1 | 0.1 | 0.1 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0.1 | 0.1 | 0.1 |
| 1 | Code-heavy | cold-startup | 1 | 0.2 | 0.2 | 0.2 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0.1 | 0.1 | 0.1 |

## sourceReopen.initializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceReopen.grammarInitializationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceReopen.tokenizationMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## sourceReopen.totalWorkerMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## edit.fence.visibleMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 45.321 | 49.956 | 49.956 |
| 0 | Small | cold-startup | 2 | 46.605 | 79.752 | 79.752 |
| 2 | Small | cold-startup | 2 | 46.098 | 46.955 | 46.955 |
| D | Small | cold-startup | 2 | 45.566 | 46.268 | 46.268 |
| D | Small | fresh-profile-after-examples-settled | 2 | 34.657 | 46.892 | 46.892 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 44.666 | 45.97 | 45.97 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 45.235 | 46.309 | 46.309 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 35.304 | 46.343 | 46.343 |
| 0 | Overview | cold-startup | 2 | 45.49 | 45.924 | 45.924 |
| 2 | Overview | cold-startup | 2 | 45.441 | 45.603 | 45.603 |
| 1 | Overview | cold-startup | 2 | 46.965 | 47.237 | 47.237 |
| D | Overview | cold-startup | 2 | 35.294 | 44.846 | 44.846 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 45.742 | 45.742 | 45.742 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 43.513 | 43.513 | 43.513 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 36.745 | 36.745 | 36.745 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 49.465 | 49.465 | 49.465 |
| 2 | Code-heavy | cold-startup | 1 | 85.232 | 85.232 | 85.232 |
| 1 | Code-heavy | cold-startup | 1 | 586.116 | 586.116 | 586.116 |
| 0 | Code-heavy | cold-startup | 1 | 400.822 | 400.822 | 400.822 |
| D | Code-heavy | cold-startup | 1 | 46.709 | 46.709 | 46.709 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 45.947 | 45.947 | 45.947 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 844.922 | 844.922 | 844.922 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 70.313 | 70.313 | 70.313 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 73.824 | 73.824 | 73.824 |

## edit.fence.colouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 101.738 | 117.479 | 117.479 |
| 0 | Small | cold-startup | 2 | 80.831 | 84.637 | 84.637 |
| 2 | Small | cold-startup | 2 | 140.815 | 146.382 | 146.382 |
| D | Small | cold-startup | 2 | 46.136 | 46.757 | 46.757 |
| D | Small | fresh-profile-after-examples-settled | 2 | 35.178 | 47.903 | 47.903 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 139.579 | 146.068 | 146.068 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 117.314 | 118.447 | 118.447 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 50.924 | 84.6 | 84.6 |
| 0 | Overview | cold-startup | 2 | 99.356 | 99.962 | 99.962 |
| 2 | Overview | cold-startup | 2 | 133.418 | 133.918 | 133.918 |
| 1 | Overview | cold-startup | 2 | 100.136 | 100.233 | 100.233 |
| D | Overview | cold-startup | 2 | 35.78 | 45.343 | 45.343 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 46.376 | 46.376 | 46.376 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 132.518 | 132.518 | 132.518 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 66.055 | 66.055 | 66.055 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 91.599 | 91.599 | 91.599 |
| 2 | Code-heavy | cold-startup | 1 | 897.062 | 897.062 | 897.062 |
| 1 | Code-heavy | cold-startup | 1 | 682.898 | 682.898 | 682.898 |
| 0 | Code-heavy | cold-startup | 1 | 1265.242 | 1265.242 | 1265.242 |
| D | Code-heavy | cold-startup | 1 | 47.218 | 47.218 | 47.218 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 58.912 | 58.912 | 58.912 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1103.697 | 1103.697 | 1103.697 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 646.417 | 646.417 | 646.417 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 965.882 | 965.882 | 965.882 |

## edit.example.visibleMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 157.321 | 705.682 | 705.682 |
| 0 | Small | cold-startup | 2 | 146.739 | 593.83 | 593.83 |
| 2 | Small | cold-startup | 2 | 165.665 | 166.092 | 166.092 |
| D | Small | cold-startup | 2 | 164.239 | 175.658 | 175.658 |
| D | Small | fresh-profile-after-examples-settled | 2 | 194.439 | 233.25 | 233.25 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 149.935 | 183.518 | 183.518 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 156.233 | 208.816 | 208.816 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 152.022 | 245.558 | 245.558 |
| 0 | Overview | cold-startup | 2 | 99.773 | 121.283 | 121.283 |
| 2 | Overview | cold-startup | 2 | 101.832 | 135.214 | 135.214 |
| 1 | Overview | cold-startup | 2 | 91.148 | 103.71 | 103.71 |
| D | Overview | cold-startup | 2 | 93.439 | 108.663 | 108.663 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 110.088 | 110.088 | 110.088 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 125.562 | 125.562 | 125.562 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 110.355 | 110.355 | 110.355 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 152.233 | 152.233 | 152.233 |
| 2 | Code-heavy | cold-startup | 1 | 344.906 | 344.906 | 344.906 |
| 1 | Code-heavy | cold-startup | 1 | 259.972 | 259.972 | 259.972 |
| 0 | Code-heavy | cold-startup | 1 | 198.383 | 198.383 | 198.383 |
| D | Code-heavy | cold-startup | 1 | 207.887 | 207.887 | 207.887 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 195.535 | 195.535 | 195.535 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 222.149 | 222.149 | 222.149 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 213.245 | 213.245 | 213.245 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 210.113 | 210.113 | 210.113 |

## edit.shared.visibleMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 98.628 | 199.159 | 199.159 |
| 0 | Small | cold-startup | 2 | 98.529 | 275.395 | 275.395 |
| 2 | Small | cold-startup | 2 | 96.233 | 102.716 | 102.716 |
| D | Small | cold-startup | 2 | 90.69 | 146.247 | 146.247 |
| D | Small | fresh-profile-after-examples-settled | 2 | 90.104 | 112.711 | 112.711 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 91.407 | 156.753 | 156.753 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 102.399 | 139.346 | 139.346 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 93.711 | 122.196 | 122.196 |
| 0 | Overview | cold-startup | 2 | 102.77 | 111.986 | 111.986 |
| 2 | Overview | cold-startup | 2 | 93.328 | 107.121 | 107.121 |
| 1 | Overview | cold-startup | 2 | 93.091 | 115.367 | 115.367 |
| D | Overview | cold-startup | 2 | 105.937 | 111.192 | 111.192 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 110.209 | 110.209 | 110.209 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 115.931 | 115.931 | 115.931 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 114.676 | 114.676 | 114.676 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 328.813 | 328.813 | 328.813 |
| 2 | Code-heavy | cold-startup | 1 | 197.506 | 197.506 | 197.506 |
| 1 | Code-heavy | cold-startup | 1 | 213.542 | 213.542 | 213.542 |
| 0 | Code-heavy | cold-startup | 1 | 132.806 | 132.806 | 132.806 |
| D | Code-heavy | cold-startup | 1 | 132.877 | 132.877 | 132.877 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 128.052 | 128.052 | 128.052 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 144.877 | 144.877 | 144.877 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 194.878 | 194.878 | 194.878 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 143.238 | 143.238 | 143.238 |

## memory.peakServerRSS

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 325656576 | 397606912 | 397606912 |
| 0 | Small | cold-startup | 2 | 321310720 | 544874496 | 544874496 |
| 2 | Small | cold-startup | 2 | 340221952 | 628146176 | 628146176 |
| D | Small | cold-startup | 2 | 332455936 | 682553344 | 682553344 |
| D | Small | fresh-profile-after-examples-settled | 2 | 287657984 | 516104192 | 516104192 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 319598592 | 666603520 | 666603520 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 314793984 | 461807616 | 461807616 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 300531712 | 636014592 | 636014592 |
| 0 | Overview | cold-startup | 2 | 360951808 | 565850112 | 565850112 |
| 2 | Overview | cold-startup | 2 | 329322496 | 399503360 | 399503360 |
| 1 | Overview | cold-startup | 2 | 363663360 | 497655808 | 497655808 |
| D | Overview | cold-startup | 2 | 346927104 | 355594240 | 355594240 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 372645888 | 372645888 | 372645888 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 361455616 | 361455616 | 361455616 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 369270784 | 369270784 | 369270784 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 322781184 | 322781184 | 322781184 |
| 2 | Code-heavy | cold-startup | 1 | 632401920 | 632401920 | 632401920 |
| 1 | Code-heavy | cold-startup | 1 | 713719808 | 713719808 | 713719808 |
| 0 | Code-heavy | cold-startup | 1 | 775315456 | 775315456 | 775315456 |
| D | Code-heavy | cold-startup | 1 | 337408000 | 337408000 | 337408000 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 338771968 | 338771968 | 338771968 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 978788352 | 978788352 | 978788352 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 869597184 | 869597184 | 869597184 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 961179648 | 961179648 | 961179648 |

## memory.peakBrowserRSS

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 994369536 | 1016299520 | 1016299520 |
| 0 | Small | cold-startup | 2 | 850132992 | 1014841344 | 1014841344 |
| 2 | Small | cold-startup | 2 | 1031180288 | 1033330688 | 1033330688 |
| D | Small | cold-startup | 2 | 946028544 | 1000669184 | 1000669184 |
| D | Small | fresh-profile-after-examples-settled | 2 | 957501440 | 988905472 | 988905472 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 1015205888 | 1036148736 | 1036148736 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 1006084096 | 1012580352 | 1012580352 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 993935360 | 1003016192 | 1003016192 |
| 0 | Overview | cold-startup | 2 | 986447872 | 1009270784 | 1009270784 |
| 2 | Overview | cold-startup | 2 | 1055813632 | 1055862784 | 1055862784 |
| 1 | Overview | cold-startup | 2 | 1026723840 | 1029926912 | 1029926912 |
| D | Overview | cold-startup | 2 | 1036107776 | 1036152832 | 1036152832 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 1008771072 | 1008771072 | 1008771072 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 1010241536 | 1010241536 | 1010241536 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 1032142848 | 1032142848 | 1032142848 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 1026117632 | 1026117632 | 1026117632 |
| 2 | Code-heavy | cold-startup | 1 | 1204707328 | 1204707328 | 1204707328 |
| 1 | Code-heavy | cold-startup | 1 | 1077202944 | 1077202944 | 1077202944 |
| 0 | Code-heavy | cold-startup | 1 | 1069428736 | 1069428736 | 1069428736 |
| D | Code-heavy | cold-startup | 1 | 1036500992 | 1036500992 | 1036500992 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 1011142656 | 1011142656 | 1011142656 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1321979904 | 1321979904 | 1321979904 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1041518592 | 1041518592 | 1041518592 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1205194752 | 1205194752 | 1205194752 |

## memory.settledServerRSS

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 325656576 | 397606912 | 397606912 |
| 0 | Small | cold-startup | 2 | 321310720 | 544874496 | 544874496 |
| 2 | Small | cold-startup | 2 | 340221952 | 628146176 | 628146176 |
| D | Small | cold-startup | 2 | 332455936 | 682553344 | 682553344 |
| D | Small | fresh-profile-after-examples-settled | 2 | 287657984 | 516104192 | 516104192 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 319598592 | 666603520 | 666603520 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 314793984 | 461807616 | 461807616 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 300531712 | 636014592 | 636014592 |
| 0 | Overview | cold-startup | 2 | 360951808 | 565850112 | 565850112 |
| 2 | Overview | cold-startup | 2 | 329322496 | 399503360 | 399503360 |
| 1 | Overview | cold-startup | 2 | 363663360 | 497655808 | 497655808 |
| D | Overview | cold-startup | 2 | 346927104 | 355594240 | 355594240 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 372645888 | 372645888 | 372645888 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 361455616 | 361455616 | 361455616 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 369270784 | 369270784 | 369270784 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 322781184 | 322781184 | 322781184 |
| 2 | Code-heavy | cold-startup | 1 | 610263040 | 610263040 | 610263040 |
| 1 | Code-heavy | cold-startup | 1 | 713719808 | 713719808 | 713719808 |
| 0 | Code-heavy | cold-startup | 1 | 775315456 | 775315456 | 775315456 |
| D | Code-heavy | cold-startup | 1 | 337408000 | 337408000 | 337408000 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 338771968 | 338771968 | 338771968 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 978788352 | 978788352 | 978788352 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 864006144 | 864006144 | 864006144 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 961179648 | 961179648 | 961179648 |

## memory.settledBrowserRSS

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 931385344 | 942084096 | 942084096 |
| 0 | Small | cold-startup | 2 | 850132992 | 922234880 | 922234880 |
| 2 | Small | cold-startup | 2 | 945188864 | 953331712 | 953331712 |
| D | Small | cold-startup | 2 | 922861568 | 928079872 | 928079872 |
| D | Small | fresh-profile-after-examples-settled | 2 | 919252992 | 935956480 | 935956480 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 943570944 | 961097728 | 961097728 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 931192832 | 942858240 | 942858240 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 930275328 | 938053632 | 938053632 |
| 0 | Overview | cold-startup | 2 | 942190592 | 960847872 | 960847872 |
| 2 | Overview | cold-startup | 2 | 970772480 | 996458496 | 996458496 |
| 1 | Overview | cold-startup | 2 | 966094848 | 968159232 | 968159232 |
| D | Overview | cold-startup | 2 | 943792128 | 967598080 | 967598080 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 940953600 | 940953600 | 940953600 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 974180352 | 974180352 | 974180352 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 941604864 | 941604864 | 941604864 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 968355840 | 968355840 | 968355840 |
| 2 | Code-heavy | cold-startup | 1 | 1023275008 | 1023275008 | 1023275008 |
| 1 | Code-heavy | cold-startup | 1 | 1008877568 | 1008877568 | 1008877568 |
| 0 | Code-heavy | cold-startup | 1 | 1062297600 | 1062297600 | 1062297600 |
| D | Code-heavy | cold-startup | 1 | 976850944 | 976850944 | 976850944 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 980455424 | 980455424 | 980455424 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1268822016 | 1268822016 | 1268822016 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1041518592 | 1041518592 | 1041518592 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1205194752 | 1205194752 | 1205194752 |

## memory.settledCombinedRSS

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 1267740672 | 1328992256 | 1328992256 |
| 0 | Small | cold-startup | 2 | 1171443712 | 1467109376 | 1467109376 |
| 2 | Small | cold-startup | 2 | 1293553664 | 1573335040 | 1573335040 |
| D | Small | cold-startup | 2 | 1260535808 | 1605414912 | 1605414912 |
| D | Small | fresh-profile-after-examples-settled | 2 | 1223614464 | 1435357184 | 1435357184 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 1280696320 | 1610174464 | 1610174464 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 1257652224 | 1393000448 | 1393000448 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 1238585344 | 1566289920 | 1566289920 |
| 0 | Overview | cold-startup | 2 | 1303142400 | 1526697984 | 1526697984 |
| 2 | Overview | cold-startup | 2 | 1300094976 | 1395961856 | 1395961856 |
| 1 | Overview | cold-startup | 2 | 1329758208 | 1465815040 | 1465815040 |
| D | Overview | cold-startup | 2 | 1290719232 | 1323192320 | 1323192320 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 1313599488 | 1313599488 | 1313599488 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 1335635968 | 1335635968 | 1335635968 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 1310875648 | 1310875648 | 1310875648 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 1291137024 | 1291137024 | 1291137024 |
| 2 | Code-heavy | cold-startup | 1 | 1633538048 | 1633538048 | 1633538048 |
| 1 | Code-heavy | cold-startup | 1 | 1722597376 | 1722597376 | 1722597376 |
| 0 | Code-heavy | cold-startup | 1 | 1837613056 | 1837613056 | 1837613056 |
| D | Code-heavy | cold-startup | 1 | 1314258944 | 1314258944 | 1314258944 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 1319227392 | 1319227392 | 1319227392 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 2247610368 | 2247610368 | 2247610368 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1905524736 | 1905524736 | 1905524736 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 2166374400 | 2166374400 | 2166374400 |

## cpu.ownedProcessesSeconds

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 17.1 | 19.1 | 19.1 |
| 0 | Small | cold-startup | 2 | 17.32 | 21.91 | 21.91 |
| 2 | Small | cold-startup | 2 | 18.05 | 19.18 | 19.18 |
| D | Small | cold-startup | 2 | 17.58 | 18.79 | 18.79 |
| D | Small | fresh-profile-after-examples-settled | 2 | 17.62 | 18.79 | 18.79 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 18.21 | 19.95 | 19.95 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 17.57 | 19.18 | 19.18 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 17.8 | 19.14 | 19.14 |
| 0 | Overview | cold-startup | 2 | 17.69 | 18.08 | 18.08 |
| 2 | Overview | cold-startup | 2 | 18.64 | 20.5 | 20.5 |
| 1 | Overview | cold-startup | 2 | 18.09 | 18.84 | 18.84 |
| D | Overview | cold-startup | 2 | 18.06 | 18.94 | 18.94 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 18.07 | 18.07 | 18.07 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 19.88 | 19.88 | 19.88 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 18.98 | 18.98 | 18.98 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 21.51 | 21.51 | 21.51 |
| 2 | Code-heavy | cold-startup | 1 | 38.02 | 38.02 | 38.02 |
| 1 | Code-heavy | cold-startup | 1 | 28.08 | 28.08 | 28.08 |
| 0 | Code-heavy | cold-startup | 1 | 27.6 | 27.6 | 27.6 |
| D | Code-heavy | cold-startup | 1 | 19.58 | 19.58 | 19.58 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 20.38 | 20.38 | 20.38 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 38.63 | 38.63 | 38.63 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 28.16 | 28.16 | 28.16 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 25.8 | 25.8 | 25.8 |

## memory.workerHeap

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 0 | Small | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | cold-startup | 2 | 0 | 0 | 0 |
| D | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 0 | 0 | 0 |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 2054268 | 2062140 | 2062140 |
| 1 | Overview | cold-startup | 2 | 1101112 | 1101444 | 1101444 |
| D | Overview | cold-startup | 2 | 0 | 0 | 0 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 2066104 | 2066104 | 2066104 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 1100080 | 1100080 | 1100080 |
| 2 | Code-heavy | cold-startup | 1 | 24970416 | 24970416 | 24970416 |
| 1 | Code-heavy | cold-startup | 1 | 8190280 | 8190280 | 8190280 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 21364692 | 21364692 | 21364692 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 8228788 | 8228788 | 8228788 |

## cache.compilerBytes

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 383710 | 383712 | 383712 |
| 0 | Small | cold-startup | 2 | 383707 | 383711 | 383711 |
| 2 | Small | cold-startup | 2 | 383712 | 383713 | 383713 |
| D | Small | cold-startup | 2 | 383723 | 383725 | 383725 |
| D | Small | fresh-profile-after-examples-settled | 2 | 383735 | 383735 | 383735 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 383730 | 383735 | 383735 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 383728 | 383731 | 383731 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 383728 | 383728 | 383728 |
| 0 | Overview | cold-startup | 2 | 507979 | 507981 | 507981 |
| 2 | Overview | cold-startup | 2 | 507984 | 507987 | 507987 |
| 1 | Overview | cold-startup | 2 | 507985 | 507985 | 507985 |
| D | Overview | cold-startup | 2 | 507988 | 507989 | 507989 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 507997 | 507997 | 507997 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 507975 | 507975 | 507975 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 507977 | 507977 | 507977 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 507975 | 507975 | 507975 |
| 2 | Code-heavy | cold-startup | 1 | 466560 | 466560 | 466560 |
| 1 | Code-heavy | cold-startup | 1 | 466557 | 466557 | 466557 |
| 0 | Code-heavy | cold-startup | 1 | 466556 | 466556 | 466556 |
| D | Code-heavy | cold-startup | 1 | 466577 | 466577 | 466577 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 466567 | 466567 | 466567 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 466561 | 466561 | 466561 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 466561 | 466561 | 466561 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 466558 | 466558 | 466558 |

## cache.indexBytes

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Small | cold-startup | 2 | 466 | 467 | 467 |
| 0 | Small | cold-startup | 2 | 466 | 467 | 467 |
| 2 | Small | cold-startup | 2 | 466 | 467 | 467 |
| D | Small | cold-startup | 2 | 466 | 467 | 467 |
| D | Small | fresh-profile-after-examples-settled | 2 | 466 | 467 | 467 |
| 2 | Small | fresh-profile-after-examples-settled | 2 | 466 | 467 | 467 |
| 1 | Small | fresh-profile-after-examples-settled | 2 | 467 | 467 | 467 |
| 0 | Small | fresh-profile-after-examples-settled | 2 | 467 | 467 | 467 |
| 0 | Overview | cold-startup | 2 | 1049 | 1049 | 1049 |
| 2 | Overview | cold-startup | 2 | 1049 | 1049 | 1049 |
| 1 | Overview | cold-startup | 2 | 1049 | 1049 | 1049 |
| D | Overview | cold-startup | 2 | 1049 | 1049 | 1049 |
| D | Overview | fresh-profile-after-examples-settled | 1 | 1049 | 1049 | 1049 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 1049 | 1049 | 1049 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 1049 | 1049 | 1049 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 1049 | 1049 | 1049 |
| 2 | Code-heavy | cold-startup | 1 | 467 | 467 | 467 |
| 1 | Code-heavy | cold-startup | 1 | 467 | 467 | 467 |
| 0 | Code-heavy | cold-startup | 1 | 467 | 467 | 467 |
| D | Code-heavy | cold-startup | 1 | 467 | 467 | 467 |
| D | Code-heavy | fresh-profile-after-examples-settled | 1 | 467 | 467 | 467 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 467 | 467 | 467 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 467 | 467 | 467 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 467 | 467 | 467 |

## first.visibleAndColouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 2 | 210.9 | 213.8 | 213.8 |
| 2 | Overview | cold-startup | 2 | 248.2 | 297.4 | 297.4 |
| 1 | Overview | cold-startup | 2 | 201.9 | 211.6 | 211.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 331.9 | 331.9 | 331.9 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 213.7 | 213.7 | 213.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 230.4 | 230.4 | 230.4 |
| 2 | Code-heavy | cold-startup | 1 | 3413.8 | 3413.8 | 3413.8 |
| 1 | Code-heavy | cold-startup | 1 | 746.3 | 746.3 | 746.3 |
| 0 | Code-heavy | cold-startup | 1 | 1091.7 | 1091.7 | 1091.7 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1464.2 | 1464.2 | 1464.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1038.7 | 1038.7 | 1038.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 652.9 | 652.9 | 652.9 |

## first.colouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 2 | 182.7 | 194.8 | 194.8 |
| 2 | Overview | cold-startup | 2 | 248.2 | 297.4 | 297.4 |
| 1 | Overview | cold-startup | 2 | 201.9 | 211.6 | 211.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 331.9 | 331.9 | 331.9 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 213.7 | 213.7 | 213.7 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 230.4 | 230.4 | 230.4 |
| 2 | Code-heavy | cold-startup | 1 | 3413.8 | 3413.8 | 3413.8 |
| 1 | Code-heavy | cold-startup | 1 | 746.3 | 746.3 | 746.3 |
| 0 | Code-heavy | cold-startup | 1 | 1091.7 | 1091.7 | 1091.7 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1464.2 | 1464.2 | 1464.2 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1038.7 | 1038.7 | 1038.7 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 652.9 | 652.9 | 652.9 |

## retained.visibleAndColouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 2 | 49 | 49.3 | 49.3 |
| 2 | Overview | cold-startup | 2 | 48.7 | 48.9 | 48.9 |
| 1 | Overview | cold-startup | 2 | 48.2 | 49.3 | 49.3 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 49.3 | 49.3 | 49.3 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 48.9 | 48.9 | 48.9 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 49.7 | 49.7 | 49.7 |
| 2 | Code-heavy | cold-startup | 1 | 54.3 | 54.3 | 54.3 |
| 1 | Code-heavy | cold-startup | 1 | 45.4 | 45.4 | 45.4 |
| 0 | Code-heavy | cold-startup | 1 | 44.9 | 44.9 | 44.9 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 44.4 | 44.4 | 44.4 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 45.5 | 45.5 | 45.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 46.2 | 46.2 | 46.2 |

## retained.colouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 1 | Overview | cold-startup | 2 | 0 | 0 | 0 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | cold-startup | 1 | 0 | 0 | 0 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 0 | 0 | 0 |

## evicted.visibleAndColouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 2 | 130.3 | 131.9 | 131.9 |
| 2 | Overview | cold-startup | 2 | 138.7 | 158.9 | 158.9 |
| 1 | Overview | cold-startup | 2 | 132.2 | 132.6 | 132.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 181.6 | 181.6 | 181.6 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 148 | 148 | 148 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 214.5 | 214.5 | 214.5 |
| 2 | Code-heavy | cold-startup | 1 | 5334.8 | 5334.8 | 5334.8 |
| 1 | Code-heavy | cold-startup | 1 | 599.1 | 599.1 | 599.1 |
| 0 | Code-heavy | cold-startup | 1 | 715.2 | 715.2 | 715.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1536.1 | 1536.1 | 1536.1 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 699.5 | 699.5 | 699.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 664.9 | 664.9 | 664.9 |

## evicted.colouredMs

| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Overview | cold-startup | 2 | 100.8 | 102.2 | 102.2 |
| 2 | Overview | cold-startup | 2 | 138.7 | 158.9 | 158.9 |
| 1 | Overview | cold-startup | 2 | 132.2 | 132.6 | 132.6 |
| 2 | Overview | fresh-profile-after-examples-settled | 1 | 181.6 | 181.6 | 181.6 |
| 0 | Overview | fresh-profile-after-examples-settled | 1 | 99 | 99 | 99 |
| 1 | Overview | fresh-profile-after-examples-settled | 1 | 214.5 | 214.5 | 214.5 |
| 2 | Code-heavy | cold-startup | 1 | 5334.8 | 5334.8 | 5334.8 |
| 1 | Code-heavy | cold-startup | 1 | 599.1 | 599.1 | 599.1 |
| 0 | Code-heavy | cold-startup | 1 | 715.2 | 715.2 | 715.2 |
| 2 | Code-heavy | fresh-profile-after-examples-settled | 1 | 1536.1 | 1536.1 | 1536.1 |
| 0 | Code-heavy | fresh-profile-after-examples-settled | 1 | 699.5 | 699.5 | 699.5 |
| 1 | Code-heavy | fresh-profile-after-examples-settled | 1 | 664.9 | 664.9 | 664.9 |
