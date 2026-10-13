# Rspack pipeline lab, stage 1, 2026-10-12

Formats fixture (seed 1), variants 0, 2 and 3, every sequence once, headless
Chrome through the standalone server, worker on the bundled Electron runtime.
Workbench commit 63a096f plus the lab's uncommitted files. The plan and the
analysis are in [the lab README](../../README.md#stage-1-report).

| Variant | Checks | Failed |
| --- | ---: | --- |
| 0 | 64 | edit cssGlobal, edit cssModule, edit vueScoped, edit reactComponent, edit vueTemplate, edit htmlStylesheet, edit docsMarkdown, edit docsExample, edit syntaxError, no-polling |
| 2 | 64 | none |
| 3 | 64 | none |

The control's failures are the checks that need hot updates, in-place docs
updates or push: it reloads the preview on every edit and polls revisions.

Edit to visible:

| Edit | 0 control | 2 warm spare | 3 warm document |
| --- | --- | --- | --- |
| Global CSS | 777 ms, re-render, state lost | 80 ms, hot, state kept | 77 ms, hot, state kept |
| CSS module | 921 ms, re-render, state lost | 55 ms, hot, state kept | 52 ms, hot, state kept |
| Vue scoped style | 858 ms, re-render, state lost | 54 ms, hot, state kept | 54 ms, hot, state kept |
| React component | 843 ms, re-render, state lost | 189 ms, hot, state kept | 82 ms, hot, state kept |
| Module exporting values | 932 ms, re-render, state lost | 79 ms, hot, state kept | 78 ms, hot, state kept |
| Vue template | 639 ms, re-render, state lost | 52 ms, hot, state kept | 52 ms, hot, state kept |
| Vue script | 795 ms, re-render, state lost | 52 ms, hot, state reset by Vue | 53 ms, hot, state reset by Vue |
| HTML markup | 797 ms, re-render | 52 ms, re-render in place | 52 ms, re-render in place |
| HTML linked stylesheet | 773 ms, re-render | 52 ms, hot | 53 ms, hot |
| Astro component | 801 ms, re-render | 80 ms, re-render in place | 80 ms, re-render in place |
| Docs Markdown | 752 ms, page reload | 51 ms, in place | 51 ms, in place |
| Docs example | 961 ms, page reload | 80 ms, in place | 80 ms, in place |
| Preview definition | 996 ms, re-render, state lost | 106 ms, preview reload, state lost | 103 ms, preview reload, state lost |
| Environment | 931 ms, re-render, state lost | 106 ms, preview reload, state lost | 133 ms, preview reload, state lost |
| `workbench.config.ts` | 964 ms, re-render, state lost | 375 ms, compilers replaced, reload, state lost | 384 ms, compilers replaced, reload, state lost |
| Module shared by every page, and a hidden page then shown | 937 ms, re-render, state lost | 78 ms, hot, state kept; hidden page current | 78 ms, hot, state kept; hidden page current |
| Syntax error, then its fix | did not recover within 30 s | error shown, recovered in 317 ms | error shown, recovered in 266 ms |
| Two saves 150 ms apart | ends on the second save | ends on the second save | ends on the second save |
