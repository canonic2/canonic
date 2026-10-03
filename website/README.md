# Workbench website

A static site, built with [Astro](https://astro.build), that documents the
extension's UI: the workbench layout, each toolbar button, page states, markup
and handoff, lenses, and how to install a platform build. The build fills in
release downloads and publishes to GitHub Pages.

| File | Job |
| --- | --- |
| `src/pages/index.astro` | the Workbench page |
| `src/layouts/Base.astro` | the page shell: head, the product bar and section links, and footer |
| `src/components/Install.astro` | the download cards for a workbench release |
| `src/lib/releases.js` | picks the release and checks its download links |
| `src/lib/marks.js` | the Canonic wordmark and product marks read from the stored SVG exports, in product order |
| `src/data/screenshots.json` | the overview's numbered regions and the toolbar strip, written by `screenshots/capture.cjs` |
| `src/styles/global.css` | light and dark tokens from the website design (warm grays, dark bands, Canonic blue) and every component's styles |
| `public/icons/` | original monochrome and blue-accent SVG exports for Workbench, Sandbox, Playground, Shield, and Studio |
| `public/canonic.svg` | the Workbench favicon, with a blue accent and light/dark fills |
| `public/images/` | screenshots of the workbench showing the Acme example |
| `screenshots/` | the Acme fixture and the script that regenerates the screenshots |
| `astro.config.mjs` | the Pages origin and base path |

## Developing

The site uses pnpm; `packageManager` in `package.json` pins its version.
From this folder:

```sh
pnpm install
pnpm run dev      # http://localhost:4321/canonic/
pnpm test         # release selection
pnpm run build    # writes dist/
pnpm run preview  # serves dist/
```

The site is served under `/canonic/`, so link to files in `public/` through
`base` from `src/lib/base.js`, never with a leading `/`.

The page loads Geist and Geist Mono from Google Fonts, with system fonts as
the fallback. A small script on the page marks the download for the visitor's
computer and enables the Copy buttons; without it the page still lists every
download.

The dev server links the Install cards to GitHub's latest release. `pnpm run
build` asks the GitHub API for the newest workbench release that has all six
platform files, or for the release named by `WORKBENCH_TAG`, and links each
file's exact download URL. Set `GITHUB_TOKEN` if the API rate-limits you.

## Publishing

The [Publish website workflow](../.github/workflows/website.yml) builds the
site and deploys `dist/` to GitHub Pages when a `website/v<version>` tag is
pushed (for example, `website/v1.0.1`). A successful `workbench/v<version>`
build and release also calls it, using the newest tagged website version and
the new release's download links. Until the first website tag, this uses the
website on `main`. The repository's Pages source must be set to **GitHub
Actions**. The `github-pages` environment must allow both `website/v*` and
`workbench/v*` tag deployments. The published site is at
`https://canonic2.github.io/canonic/`; with a custom domain, the workflow
passes the new origin and base path to the build. If no workbench release
exists yet, the Install section waits for one. A website tag uses the newest
published workbench release available when it deploys.

## Updating screenshots

When the workbench UI changes, regenerate the images from the repository root:

```sh
node packages/website/screenshots/capture.cjs
```

The script starts `packages/workbench/server.js` on `screenshots/fixture`, drives
headless Chrome through `packages/workbench/capture.js`, and overwrites the screenshots
in `public/images/`. Set `CHROME_PATH` to choose a browser.

- The overview is a 1440 × 860 window. The script also measures the workbench's
  screen list, top bar, canvas, and markup bar there, and writes them to
  `regions` in `src/data/screenshots.json`, where the page draws the numbered
  outlines. Don't edit that file by hand.
- The toolbar section's strip is four crops (`public/images/toolbar-*.png`)
  from the same window rendered at 4×, one per group: Actions, frame and
  screen controls, markup tools, and zoom. The script writes them to `toolbar`
  in `src/data/screenshots.json`, with a hotspot over every control, and the
  page lays them side by side with an invisible button over each hotspot. A
  small script at the end of `index.astro` captions the hotspot you point at,
  tap, or tab to, using the matching entry in the "All controls" list, so each
  description is written once.
- Before writing the strip, the script checks that each "All controls" list's
  `data-label` attributes in `index.astro` match the toolbar's control labels
  in order, and stops with both lists if they don't. When a control is added,
  removed, or renamed, update the list, then rerun.
- The States and Markup shots are 1120 × 860. The canvas zooms the mobile
  frame to fit, and the script scales page coordinates to that zoom when it
  draws the marks.

Keep the fixture on Acme and example.com placeholders.

## Product icons

Store design exports unchanged in `public/icons/<product>.svg` and
`public/icons/<product>-blue.svg`. The website reads its product paths from
these files. Studio is stored for future use; it is not part of the current
product navigation.

Workbench also ships the mark in `../workbench/icon.svg` (white for VS Code),
`../workbench/workbench/canonic.svg` (blue accent with a dark-theme fill),
and `../workbench/icon-source.svg` / `icon.png` (128 px extension listing icon).
These and `public/canonic.svg` use the exported paths, with only presentation
changes for their background, color, or size. Refresh those assets and the
website screenshots when replacing the Workbench exports.
