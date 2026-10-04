# Workbench website

A static site, built with [Astro](https://astro.build), that documents the
extension's UI: the workbench layout, each toolbar button, page states, markup
and handoff, lenses, and how to install a platform build. The build fills in
release downloads and publishes to GitHub Pages.

| File | Job |
| --- | --- |
| `src/pages/index.astro` | the Workbench overview |
| `src/pages/workbench/install.astro` | the Install page: the download for this computer, every platform's file, and the setup steps |
| `src/pages/workbench/changelog.astro` | the Changelog page: every complete release, newest first, with its notes, links, and builds |
| `src/layouts/Base.astro` | the page shell: head, the product bar and section links, and footer |
| `src/components/Install.astro` | the download cards for a workbench release |
| `src/components/DownloadScripts.astro` | the inline script that recommends this computer's download, and the Copy buttons |
| `src/lib/workbench.js` | what every Workbench page shares: page URLs, the section links (Overview, Docs, Install, Changelog, GitHub), and the release data, fetched once per build |
| `src/lib/releases.js` | lists complete releases, picks the one to link, reads their notes, and checks their download links |
| `src/lib/marks.js` | the Canonic wordmark and product marks read from the stored SVG exports, in product order |
| `src/data/screenshots.json` | the overview's numbered regions and the toolbar strip, written by `screenshots/capture.cjs` |
| `src/styles/global.css` | light and dark tokens from the website design (warm grays, dark bands, Canonic blue) and every component's styles |
| `src/content.config.ts` | loads Markdown directly from `../workbench/docs/` |
| `src/pages/workbench/docs/[...slug].astro` | generated docs with guide navigation, section links, and previous/next links |
| `src/lib/docs.js` | docs grouping, URLs, and Markdown link conversion |
| `src/styles/docs.css` | responsive docs layout and Markdown typography |
| `public/icons/` | original monochrome and blue-accent SVG exports for Workbench, Sandbox, Playground, Shield, Studio, and Link |
| `public/logos/` | original Canonic symbol, wordmark, lockup, and app icon exports |
| `public/canonic.svg` | the Canonic symbol favicon, with a blue accent and light/dark fills |
| `public/images/` | screenshots of the workbench showing the Acme example |
| `screenshots/` | the Acme fixture and the script that regenerates the screenshots |
| `astro.config.mjs` | the Pages origin and base path |

## Developing

The Overview, Install page, and page shell have Astro `.workbench.ts` definitions
in `previews/`. They are the **Website** project of the repository's
`workbench.yaml`, served from this folder: pick **Website** in the Workbench
view's project switcher, or run `node packages/workbench/server.js .` from the
repository root and open the address printed beside *Website*. The shell exposes
editable props and a versioned state. These previews use the project's installed
Astro compiler; they do not start the Astro application server. Content-backed
docs and application integrations can still be reviewed through a URL lens.

The previews are mocks of the pages. Each definition's `links` maps the site's
routes that have a preview, `/` and `/workbench/install/`, so with Actions on
the header and buttons move between them on the canvas. Links to the docs, the
changelog, GitHub, and the `.vsix` downloads are recorded under **Actions**
instead of followed. When you add a page preview, add its route to the other
definitions' `links`.

The site uses pnpm; `packageManager` in `package.json` pins its version.
From this folder:

```sh
pnpm install
pnpm run dev      # http://localhost:4321/
pnpm test         # release selection
pnpm run build    # writes dist/
pnpm run preview  # serves dist/
```

The site is served at `https://canonic.sh/`, with `/` as its base path. Link
to files in `public/` through `base` from `src/lib/base.js` so alternate
deployment base paths also work.

The page loads Geist and Geist Mono from Google Fonts, with system fonts as
the fallback. A small script on the page marks the download for the visitor's
computer and enables the Copy buttons; without it the page still lists every
download.

The dev server links the Install cards to GitHub's latest release. `pnpm run
build` asks the GitHub API for the newest workbench release that has all six
platform files, or for the release named by `WORKBENCH_TAG`, and links each
file's exact download URL. Set `GITHUB_TOKEN` if the API rate-limits you.
The pages share one request per build.

The Changelog page lists every complete release from the same request. Each
entry shows the release's notes from GitHub as plain text. Those notes are
the version's entry in `workbench/CHANGELOG.md`, which the release workflow
publishes. The generated "Full Changelog" line becomes the entry's changelog
button, and headings and the New Contributors list are left out. A release
with no notes of its own shows only its links and builds. To correct a
published release's notes, follow the `workbench-release` skill, then deploy
the site again. In dev the changelog still asks GitHub, and shows a link to the
releases if it can't reach them.

## Workbench documentation

The docs live at `/workbench/docs/`. Astro's content collection reads every
Markdown file in `../workbench/docs/`, which contains the public user guides.
Edit those files directly; the website keeps no duplicate content and requires
no frontmatter. In dev mode, Astro watches them for updates.

Internal specifications live separately in `../workbench/specs/` and are not
included in website pages, navigation, or search.

`README.md` becomes the docs overview.
Other filenames become page URLs. Titles and the "On this page" list come from
Markdown headings. `src/lib/docs.js` groups the main guides in reading order;
additional files appear in the "More guides" group automatically. Relative guide
links are converted to website URLs, preserving heading fragments. Links to
source files outside the docs point to GitHub. The search field filters guides
using their full Markdown text, loaded only when needed.

The layout follows the Workbench docs reference: grouped left navigation, a
reading column, a right-hand section list, dark code examples, and previous/next
cards. On smaller screens, documentation navigation is collapsible. The docs
remain readable and navigable without JavaScript; search is an enhancement.

Website builds need the adjacent `workbench/docs/` directory. Both the full
repository and the public `packages/` checkout supply this layout, including
the GitHub Pages workflow.

## Publishing

The [Publish website workflow](../.github/workflows/website.yml) builds the
site and deploys `dist/` to GitHub Pages when a `website/v<version>` tag is
pushed (for example, `website/v1.0.1`). A successful `workbench/v<version>`
build and release also calls it, using the newest tagged website version and
the new release's download links. Until the first website tag, this uses the
website on `main`. The repository's Pages source must be set to **GitHub
Actions**. The `github-pages` environment must allow both `website/v*` and
`workbench/v*` tag deployments. The published site is at
`https://canonic.sh/`; set **Custom domain** to `canonic.sh` in the repository's
Pages settings and enable **Enforce HTTPS** once the certificate is available.
The workflow passes the configured origin and base path to the build, while
local builds default to `https://canonic.sh/` at `/`. GitHub Actions publishing
does not require a `CNAME` file. If no workbench release
exists yet, the Install section waits for one. A website tag uses the newest
published workbench release available when it deploys.

GitHub Pages names each deployment after its commit, and a second deploy from
an already-deployed commit leaves the first one live. Put each website tag on
a commit that has never been deployed: never on a `workbench/v*` tag's commit,
whose release has already deployed the site.

## Updating screenshots

When the workbench UI changes, regenerate the images from the repository root:

```sh
node packages/website/screenshots/capture.cjs
```

The script starts `packages/workbench/server.js` on `screenshots/fixture`, drives
headless Chrome through `packages/workbench/scripts/chrome.cjs`, and overwrites the screenshots
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

Store original design exports unchanged in the repository's root `assets/`
folder. Run `node scripts/refresh-brand-assets.cjs` from the repository root
to refresh the self-contained package copies and presentation variants.
The website reads paths and view boxes from `public/icons/` and `public/logos/`.
Studio and Link are stored for future use outside the current product navigation.

Workbench also ships the mark in `../workbench/icon.svg` (white for VS Code),
`../workbench/workbench/canonic.svg` (blue accent with a dark-theme fill),
and `../workbench/icon-source.svg` / `icon.png` (128 px extension listing icon).
These and `public/canonic.svg` use the exported paths, with only presentation
changes for their background, color, or size. Refresh those assets and the
website screenshots when replacing the Workbench exports.
