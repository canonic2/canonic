# Shared Acme fixture

`acme-app.mjs` generates a deterministic Acme project for Workbench labs, and
`formats.mjs` generates the `formats` size. The same size and seed always
produce the same files. Generated projects and their
installed dependencies live in a lab run's temporary directory; nothing
generated is committed.

```sh
node labs/fixtures/cli.mjs <empty-directory> --size large --seed 1 --install
```

`--install` runs `npm ci` against the committed lockfile in `locks/`, or
`npm install` when none exists and copies the resolved lockfile there for
review. npm's cache is a lab-owned folder under the system temporary
directory, and install scripts do not run.

## Sizes

| Size | Shape |
| --- | --- |
| `small` | 5 component previews in `src/ui/`, each with two states. React only, no fonts or images |
| `large` | 40 page previews plus 2 broken previews. An environment wraps every preview in an Apollo provider and an app shell that eagerly registers 60 features, so most code is shared by every page, as in an app whose route tree imports every feature |
| `large-system` | The `large` shape with system fonts only, and the extended isolation sentinels: each page leaves a listener, an interval, a portal, a `<body>` class, a global and a request mock, and records in `<html data-acme-leaks>` what an earlier page left. The check runs after mount, so it does not delay readiness. Uses the `large` lockfile |
| `large-web` | `large-system` plus the environment's web fonts (Inter variable, Noto Sans JP with 320 characters from across the CJK block, a variable icon font with ligatures), JetBrains Mono on every fourth page only, and a deterministic 1200×800 PNG of about 2.5 MB per page. Its own lockfile |
| `huge` | `large-system` with 200 pages. Uses the `large` lockfile |
| `formats` | One preview or page in every native format: React pages, a Vue single-file component, an Astro component, an HTML page, a React Native Web screen, a preview through a project adapter, a docs page with web and React Native Web lenses, and 2 broken previews. Project plugins in esbuild and Rspack form, and named edit targets |

Each large page has its own sections (14–34 modules), an Apollo query
answered by a request mock (with an `empty` state), two SVG images, a page
stylesheet, a dynamically imported country locale, and, on some pages, a
TanStack table or phone-number formatting. Its `Interactive.tsx` holds a
counter and a text field for Fast Refresh checks, plus an edit marker.

At seed 1, a large page built alone by Workbench 0.13.1's compiler is about
4.7–5.2 MB of JavaScript, close to the selected project that motivated the
[preview loading lab](../preview-loading/README.md#hypothesis).

## The formats size

The two React pages share `src/shared/format.ts` and `src/shared/constants.ts`,
import the global stylesheet through the React environment and a CSS module,
fetch `/api/greeting` from a request mock, and hold a counter, a text field
and a `Revision 0` marker. The Vue component has a scoped style, a counter and
a text field. The Astro component has props, two states and a style block, and
no islands, which Workbench rejects. The HTML page links a stylesheet, shows an
image and runs an inline script. The `acme-canvas` adapter is registered in
`workbench.config.ts`. The docs page is a `defineDocs` definition whose canvas
address is its Markdown file, `src/docs/buttons.md`, with `~native` for its
other lens.

`workbench.config.ts` has three project plugins, each in the esbuild form
under `plugins` and the Rspack form under `rspack`, which the shipping compiler
ignores:

- `virtual:acme-build-info`, a module the plugin serves;
- a transform that replaces `__ACME_GREETING__` in project sources while they
  load, as a loader rule in `workbench/greeting-loader.cjs` for Rspack;
- `@acme/platform`, resolved to `src/platform/native.ts` from files under
  `src/native/` and to `src/platform/web.ts` elsewhere.

The Rspack forms take Rspack's classes from the compiler they are applied to,
because the config is bundled with packages external. The pages import CSS
modules as a default export, so an Rspack build sets `namedExports: false`
for them.

Every working preview leaves a window listener, a timer, a portal node, a
`<body>` class, a `window` global and a request mock behind on purpose, and on
entering records in `data-acme-leaks` on the document element any of these
that another preview left. A fresh document per preview keeps it empty.

`acme-manifest.json` lists the previews, the docs page, what each plugin
shows, the selectors for state that hot updates should keep, and the edit
targets: for each, the file, the exact text to find and replace, and the
element, text, colour or attribute that shows the result.

## Checks built into the large and small sizes

- `src/workbench/environment.tsx` sets `data-acme-seen-previews` on the
  document element to the number of preview IDs its module instance has
  rendered, and `data-acme-listeners` to its live window listeners. A fresh
  document per preview keeps both at 1.
- Each page stylesheet sets `--acme-page` on its root class, so a stylesheet
  left behind by another preview can be detected.
- `pages/broken-compile` imports a missing module; `pages/broken-mount`
  throws during render.
- `workbench.config.ts` enables a plugin that loads every project source file
  itself when `ACME_SLOW_PLUGIN=1`.
- `acme-manifest.json` in the generated project lists every preview with the
  files labs edit.

## Dependencies

The formats size pins the versions Workbench's own tests use: React 19.3.0,
Vue 3.5.43, Astro 7.3.5 and React Native Web 0.21.3.

Pinned in `acme-app.mjs` to versions published at least two weeks before the
labs' plans: React 19.3.0, Apollo Client 4.3.1 with GraphQL 17.0.2 and RxJS
7.8.2, lodash-es, date-fns, libphonenumber-js, i18n-iso-countries, TanStack
Table 8.21.3 (v9 replaced the table hook API), Floating UI, Zustand and
Inter from Fontsource (SIL Open Font License).
