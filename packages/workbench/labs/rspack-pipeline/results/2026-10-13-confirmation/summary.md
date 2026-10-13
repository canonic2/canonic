# Rspack pipeline stage 2, run set confirmation

Samples per cell: 10, warmups: 3.

| Fixture | Sequence | Cell | Variant | n | errors | p50 ms | p95 ms | max ms |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| formats | N7 | between adapters | 0 | 23 | 0 | 154 | 201 | 248 |
| formats | E1 | cssGlobal | 0 | 10 | 0 | 74 | 115 | 115 |
| formats | E1 | cssModule | 0 | 10 | 0 | 58 | 73 | 73 |
| formats | E1 | vueScoped | 0 | 10 | 0 | 65 | 110 | 110 |
| formats | E2 | reactComponent | 0 | 10 | 0 | 85 | 100 | 100 |
| formats | E3 | nonComponentModule | 0 | 10 | 0 | 93 | 100 | 100 |
| formats | E4 | vueScript | 0 | 10 | 0 | 69 | 93 | 93 |
| formats | E4 | vueTemplate | 0 | 10 | 0 | 74 | 77 | 77 |
| formats | E5 | htmlMarkup | 0 | 10 | 0 | 145 | 184 | 184 |
| formats | E5 | htmlStylesheet | 0 | 10 | 0 | 58 | 69 | 69 |
| formats | E6 | astroComponent | 0 | 10 | 0 | 173 | 178 | 178 |
| formats | E7 | docsExample | 0 | 10 | 0 | 98 | 145 | 145 |
| formats | E7 | docsMarkdown | 0 | 10 | 0 | 50 | 58 | 58 |
| formats | E8 | config | 0 | 10 | 0 | 598 | 750 | 750 |
| formats | E8 | definition | 0 | 10 | 0 | 248 | 631 | 631 |
| formats | E8 | environment | 0 | 10 | 0 | 235 | 260 | 260 |
| formats | E9 | shared | 0 | 10 | 0 | 353 | 631 | 631 |
| huge | A1 | cold, from server start | 0 | 10 | 0 | 13352 | 23230 | 23230 |
| huge | A2 | hot, from server start | 0 | 10 | 0 | 10421 | 17042 | 17042 |
| huge | A3 | from URL | 0 | 10 | 0 | 9944 | 16229 | 16229 |
| huge | A3-cold | from URL | 0 | 10 | 0 | 12881 | 22337 | 22337 |
| huge | A4 | after pre-warm | 0 | 10 | 0 | 5227 | 10921 | 10921 |
| huge | N1 | second page | 0 | 10 | 0 | 195 | 224 | 224 |
| huge | N2 | paced | 0 | 10 | 0 | 138 | 154 | 154 |
| huge | N3 | back to back | 0 | 10 | 0 | 128 | 139 | 139 |
| huge | N4 | retained | 0 | 10 | 0 | 106 | 120 | 120 |
| huge | N5 | size | 0 | 10 | 0 | 33 | 36 | 36 |
| huge | N5 | state | 0 | 10 | 0 | 106 | 119 | 119 |
| huge | N6 | reload | 0 | 10 | 0 | 60 | 64 | 64 |
| large-system | A1 | cold, from server start | 0 | 10 | 0 | 8769 | 25131 | 25131 |
| large-system | A2 | hot, from server start | 0 | 10 | 0 | 4382 | 12622 | 12622 |
| large-system | A3 | from URL | 0 | 10 | 0 | 3968 | 11304 | 11304 |
| large-system | A3-cold | from URL | 0 | 10 | 0 | 8241 | 23473 | 23473 |
| large-system | A4 | after pre-warm | 0 | 10 | 0 | 2278 | 8150 | 8150 |
| large-system | N1 | second page | 0 | 10 | 0 | 168 | 191 | 191 |
| large-system | N2 | paced | 0 | 10 | 0 | 137 | 171 | 171 |
| large-system | N3 | back to back | 0 | 10 | 0 | 123 | 167 | 167 |
| large-system | N4 | retained | 0 | 10 | 0 | 101 | 118 | 118 |
| large-system | N5 | size | 0 | 10 | 0 | 18 | 31 | 31 |
| large-system | N5 | state | 0 | 10 | 0 | 95 | 109 | 109 |
| large-system | N6 | reload | 0 | 10 | 0 | 61 | 71 | 71 |
| large-system | E2 | React component | 0 | 10 | 0 | 171 | 985 | 985 |
| large-system | E3 | module exporting more than components | 0 | 10 | 0 | 359 | 1120 | 1120 |
| large-system | E8 | environment | 0 | 10 | 0 | 591 | 759 | 759 |
| large-system | E8 | preview definition | 0 | 10 | 0 | 1053 | 1824 | 1824 |
| large-system | E9 | module shared by every page | 0 | 10 | 0 | 206 | 354 | 354 |
| large-web | N1 | second page | 0 | 10 | 0 | 200 | 232 | 232 |
| large-web | N2 | paced | 0 | 10 | 0 | 164 | 200 | 200 |
| large-web | N2 | paced, +150 ms | 0 | 10 | 0 | 344 | 385 | 385 |
| large-web | N3 | back to back | 0 | 10 | 0 | 175 | 208 | 208 |
| large-web | N4 | retained | 0 | 10 | 0 | 140 | 160 | 160 |
| large-web | N5 | size | 0 | 10 | 0 | 20 | 32 | 32 |
| large-web | N5 | state | 0 | 10 | 0 | 104 | 111 | 111 |
| large-web | N6 | reload | 0 | 10 | 0 | 60 | 70 | 70 |
| large-web | N8 | new font | 0 | 10 | 0 | 232 | 297 | 297 |
| large-web | N8 | new font, +150 ms | 0 | 10 | 0 | 483 | 530 | 530 |
| small | A1 | cold, from server start | 0 | 10 | 0 | 13954 | 24326 | 24326 |
| small | A2 | hot, from server start | 0 | 10 | 0 | 4516 | 17630 | 17630 |
| small | A3 | from URL | 0 | 10 | 0 | 3331 | 11087 | 11087 |
| small | A3-cold | from URL | 0 | 10 | 0 | 12737 | 20894 | 20894 |
| small | A4 | after pre-warm | 0 | 10 | 0 | 1082 | 2059 | 2059 |
| small | N1 | second page | 0 | 10 | 0 | 48 | 58 | 58 |
| small | N2 | paced | 0 | 1 | 0 | 46 | 46 | 46 |
| small | N4 | retained | 0 | 10 | 0 | 41 | 49 | 49 |
| small | N5 | size | 0 | 10 | 0 | 24 | 34 | 34 |
| small | N5 | state | 0 | 10 | 0 | 38 | 40 | 40 |
| small | N6 | reload | 0 | 10 | 0 | 37 | 40 | 40 |
| small | E2 | React component | 0 | 10 | 0 | 87 | 116 | 116 |

## Values

- small 0 cache compile cache bytes after cold start #3: 0.0 MB
- small 0 cache compile cache bytes after cold start #4: 0.0 MB
- small 0 cache compile cache bytes after cold start #5: 0.0 MB
- small 0 cache compile cache bytes after cold start #6: 0.0 MB
- small 0 cache compile cache bytes after cold start #7: 0.0 MB
- small 0 cache compile cache bytes after cold start #8: 0.0 MB
- small 0 cache compile cache bytes after cold start #9: 0.0 MB
- small 0 cache compile cache bytes after cold start #10: 0.0 MB
- small 0 cache compile cache bytes after cold start #11: 0.0 MB
- small 0 cache compile cache bytes after cold start #12: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #3: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #4: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #5: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #6: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #7: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #8: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #9: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #10: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #11: 0.0 MB
- large-system 0 cache compile cache bytes after cold start #12: 0.0 MB
- huge 0 cache compile cache bytes after cold start #3: 0.0 MB
- huge 0 cache compile cache bytes after cold start #4: 0.0 MB
- huge 0 cache compile cache bytes after cold start #5: 0.0 MB
- huge 0 cache compile cache bytes after cold start #6: 0.0 MB
- huge 0 cache compile cache bytes after cold start #7: 0.0 MB
- huge 0 cache compile cache bytes after cold start #8: 0.0 MB
- huge 0 cache compile cache bytes after cold start #9: 0.0 MB
- huge 0 cache compile cache bytes after cold start #10: 0.0 MB
- huge 0 cache compile cache bytes after cold start #11: 0.0 MB
- huge 0 cache compile cache bytes after cold start #12: 0.0 MB
- small 0 heap after N2 and N3: 7.7 MB
- small 0 worker peak RSS: 557.4 MB
- large-system 0 heap after N2 and N3: 163.2 MB
- large-system 0 worker peak RSS: 1142.6 MB
- large-web 0 heap after N2 and N3: 137.7 MB
- large-web 0 worker peak RSS: 1565.5 MB
- huge 0 heap after N2 and N3: 141.4 MB
- huge 0 worker peak RSS: 2101.6 MB
- formats 0 worker peak RSS: 651.3 MB

## Checks

- 0 formats console: 2 pass, 0 fail
- 0 formats double-save: 1 pass, 0 fail
- 0 formats freshness: 1 pass, 0 fail
- 0 formats isolation: 22 pass, 0 fail
- 0 formats no-polling: 1 pass, 0 fail
- 0 formats syntax-error: 1 pass, 0 fail
- 0 huge console: 1 pass, 0 fail
- 0 huge console-start: 1 pass, 0 fail
- 0 huge error-recovery: 1 pass, 0 fail
- 0 huge error-shown: 2 pass, 0 fail
- 0 huge isolation: 13 pass, 0 fail
- 0 huge no-polling: 1 pass, 0 fail
- 0 huge readiness: 26 pass, 0 fail
- 0 large-system console: 2 pass, 0 fail
- 0 large-system console-start: 1 pass, 0 fail
- 0 large-system error-recovery: 1 pass, 0 fail
- 0 large-system error-shown: 2 pass, 0 fail
- 0 large-system freshness: 1 pass, 0 fail
- 0 large-system isolation: 13 pass, 0 fail
- 0 large-system no-polling: 1 pass, 0 fail
- 0 large-system readiness: 26 pass, 0 fail
- 0 large-web console: 1 pass, 0 fail
- 0 large-web error-recovery: 1 pass, 0 fail
- 0 large-web error-shown: 2 pass, 0 fail
- 0 large-web isolation: 13 pass, 0 fail
- 0 large-web no-polling: 1 pass, 0 fail
- 0 large-web readiness: 26 pass, 0 fail
- 0 small console: 2 pass, 0 fail
- 0 small console-start: 1 pass, 0 fail
- 0 small error-recovery: 1 pass, 0 fail
- 0 small freshness: 1 pass, 0 fail
- 0 small no-polling: 1 pass, 0 fail
- 0 small readiness: 4 pass, 0 fail
- all formats no-project-writes: 1 pass, 0 fail
- all huge no-project-writes: 1 pass, 0 fail
- all large-system no-project-writes: 1 pass, 0 fail
- all large-web no-project-writes: 1 pass, 0 fail
- all small no-project-writes: 1 pass, 0 fail
