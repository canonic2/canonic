# Canonic website

A static site, built with [Astro](https://astro.build), with Workbench and Shield
overview pages and guides. Workbench also has platform downloads and a changelog
filled from GitHub releases. The site publishes to GitHub Pages.

The website source is public; `specs/` is private. Building uses public sources:
this package and the adjacent `../workbench/docs/` and `../shield/docs/` directories.

| File | Job |
| --- | --- |
| `src/pages/workbench/index.astro` | the Workbench overview |
| `src/pages/shield/index.astro` | the Shield overview and source-install instructions |
| `src/pages/shield/docs/` | Shield guides and their search index |
| `src/lib/shield.js` | Shield page URLs and section navigation |
| `src/components/DocsPage.astro` | the shared product guide layout |
| `src/pages/workbench/install.astro` | the Install page: the download for this computer, every platform's file, and the setup steps |
| `src/pages/workbench/changelog.astro` | the Changelog page: every published release, newest first, with its notes, links, and builds where it has them |
| `src/layouts/Base.astro` | the page shell: head, the product bar and section links, and footer |
| `src/components/` | the site's components, each with a preview in `previews/components/`; see [Components](#components) |
| `src/scripts/` | browser code the components share: platform detection, copying, and the `<canonic-downloads>` element |
| `src/lib/workbench.js` | what every Workbench page shares: page URLs, the section links (Overview, Docs, Install, Changelog, GitHub), and the release data, fetched once per build |
| `src/lib/releases.js` | lists releases, picks the complete one to link, reads their notes, and checks their download links |
| `src/lib/marks.js` | the Canonic wordmark and product marks read from the stored SVG exports, in product order |
| `src/data/screenshots.json` | the overview's numbered regions and the controls strip, written by `screenshots/capture.cjs` |
| `src/styles/global.css` | light and dark tokens from the website design (warm grays, dark bands, Canonic blue) and every component's styles |
| `src/content.config.ts` | loads Markdown directly from `../workbench/docs/` and `../shield/docs/` |
| `src/pages/workbench/docs/[...slug].astro` | generated docs with guide navigation, section links, and previous/next links |
| `src/lib/docs.js` | docs navigation, URLs, and the Markdown link and highlighting settings |
| `src/lib/doc-groups.js` | the docs sidebar's groups, by guide id |
| `src/styles/docs.css` | responsive docs layout and Markdown typography |
| `public/icons/` | original monochrome and blue-accent SVG exports for Workbench, Testrun, Playground, Shield, Studio, and Link |
| `public/logos/` | original Canonic symbol, wordmark, lockup, and app icon exports |
| `public/canonic.svg` | the Canonic symbol favicon, with a blue accent and light/dark fills |
| `public/images/` | screenshots of the workbench showing the Acme example |
| `screenshots/` | the script that regenerates the screenshots from the repository's Acme demo |
| `astro.config.mjs` | the Pages origin and base path |

## Components

Everything a page repeats, and everything with behavior, is a component in
`src/components/`. Behavior lives in a custom element defined in the
component's own `<script>`: the component renders working HTML on the
server, and the element enhances it in the browser. Pages work without
JavaScript.

| Component | Element | Job |
| --- | --- | --- |
| `CodeBlock` | `<canonic-code-block>` | a code sample with its file name and a Copy button |
| `InstallCommand` | `<canonic-install-command>` | the `code --install-extension` command, naming this computer's `.vsix`, with a Copy button |
| `DownloadCards` | `<canonic-downloads>` | a card per platform's `.vsix`, marking this computer's |
| `PlatformTable` | `<canonic-downloads>` | every platform's `.vsix` in a table, marking this computer's |
| `RecommendedDownload` | `<canonic-recommended-download>` | the Install page's panel for this computer's `.vsix` |
| `ControlStrip` | `<canonic-control-strip>` | the control screenshots, captioning each control from the control list in its slot |
| `DocsSidebar` | `<canonic-docs-sidebar>` | the docs navigation and search |
| `SectionHead`, `PageHero`, `Feature`, `ReleaseEntry` | none | static markup the pages repeat |

Custom elements are named `canonic-*` and share browser code from
`src/scripts/`. They don't use shadow DOM, so the scoped and global styles
apply. A component's `<script>` must not be `is:inline`, so it is bundled
and runs once per page. `global.css` gives the elements a `display`.

Each component has a Workbench docs page in `previews/components/`: a
`defineDocs` definition, its Markdown, and a folder of `.astro` examples, one
file per example. `fixtures.ts` holds the sample release and guide data, and
`environment.ts` marks the page with `js`, as `Base.astro` does, and answers
the docs sidebar's search index request.

## Developing

Every page and the page shell have Astro `.workbench.ts` definitions in
`previews/`: Workbench Overview, Install, Changelog and Docs, plus Shield Overview
and Docs. Each docs preview has a scenario per guide in its sidebar.
They are the **Website** space of the repository's
`workbench.yaml`, served from this folder: pick **Website** in the Workbench
view's space switcher, or run `node packages/workbench/server/node/server.js .` from the
repository root and open the address printed beside *Website*. The shell exposes
editable props and a versioned scenario. These previews use the project's installed
Astro compiler; they do not start the Astro application server.

The docs route reads the guides through `astro:content`, which needs Astro's
application pipeline. `workbench.config.ts` stands in for it: it renders every
guide with `@astrojs/markdown-satteri`, Astro's own Markdown processor (a dev
dependency pinned to the version Astro uses), with the site's link plugins and
Shiki settings from `src/lib/docs.js`. `previews/docs-page.astro` gives the
docs route the props its `getStaticPaths` would. The rendered guides aren't
tied to their files, so after editing a guide, restart the Workbench server to
see the change in the preview.

The previews are mocks of the pages. Each definition's `links` maps the site's
routes that have a preview, including the Workbench pages, `/shield/` and their
guides under `/workbench/docs/` and `/shield/docs/`, so with Actions on the header, buttons,
and docs navigation move between them on the canvas. Links to GitHub and the
`.vsix` downloads are recorded under **Actions** instead of followed. When you add a page preview,
add its route to the other definitions' `links`. The Changelog preview answers
the GitHub releases request with fixed releases, with a scenario for each case:
releases with and without builds, no releases, and GitHub unavailable. In
development, release data isn't cached, so each preview scenario gets its own
answer.

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
the version's entry in `packages/workbench/CHANGELOG.md`, which the release script
publishes. A "Full Changelog" line, when a release has one, becomes the entry's
changelog button, and headings and the New Contributors list are left out. A release
with no notes of its own shows only its links and builds. To correct a
published release's notes, edit them on the GitHub release, then deploy
the site again. In dev the changelog still asks GitHub, and shows a link to the
releases if it can't reach them.

## Product documentation

The site root redirects to `/workbench/`, where the Workbench overview lives.
The guides live at `/workbench/docs/` and `/shield/docs/`. Astro's content
collections read Markdown from `../workbench/docs/` and `../shield/docs/`.
Edit those files directly; the website keeps no duplicate content and requires
no frontmatter. In dev mode, Astro watches them for updates.

Internal specifications live in each package's private `specs/` directory and
are excluded from website pages, navigation and search.

`README.md` becomes the docs overview.
Other filenames become page URLs. Titles and the "On this page" list come from
Markdown headings. `src/lib/doc-groups.js` groups each product's guides in reading order;
additional files appear in the "More guides" group automatically. Relative guide
links are converted to website URLs, preserving heading fragments. Links to
source files outside the docs point to GitHub. The search field filters guides
using their full Markdown text, loaded only when needed.

The layout follows the Workbench docs reference: grouped left navigation, a
reading column, a right-hand section list, dark code examples, and previous/next
cards. On smaller screens, documentation navigation is collapsible. The docs
remain readable and navigable without JavaScript; search is an enhancement.

Source links use the public repository's `packages/<product>/` paths. Keep
public guides independent of private specifications and implementation files.

## Publishing

`scripts/deploy-website.sh [<workbench tag>]`, run from the repository root,
installs, tests and builds the site, then pushes `dist/` with a `CNAME` for
`canonic.sh` as the only commit of the `gh-pages` branch of `canonic2/canonic`,
which GitHub Pages serves at `https://canonic.sh/`.
`scripts/release-workbench.sh` runs it after publishing a release, with that
release's tag, so the download links name it before GitHub lists it as the
latest. The repository's Pages source is the `gh-pages` branch, with
**Custom domain** `canonic.sh` and **Enforce HTTPS**. If no workbench release
exists yet, the Install section waits for one.

Each deployment creates a new commit containing the built site and replaces
the `gh-pages` branch. Website publication does not need a source tag.

## Updating screenshots

When the workbench UI changes, regenerate the images from the repository root:

```sh
node packages/website/screenshots/capture.cjs
```

Screenshot regeneration needs the private development workspace and the Workbench
implementation; building the website uses the checked-in images.
The script starts the shipping server host for the explicit `acme` space
(`demo/` and the root `workbench.yaml`), drives headless
Chrome through `packages/workbench/scripts/chrome.cjs`, and overwrites the screenshots
in `public/images/`. Set `CHROME_PATH` to choose a browser. The screenshots show
the same demo people try in the editor, so a change to the demo can change them.

- The overview is a 1440 × 860 window. The script also measures the workbench's
  page list, top bar, canvas, and toolbar there, and writes them to
  `regions` in `src/data/screenshots.json`, where the page draws the numbered
  outlines. Don't edit that file by hand.
- The controls section's strip is five crops (`public/images/controls-*.png`)
  from the same window rendered at 4×, one per group: the size switcher, the
  Actions switch, the page actions, the annotation tools, and the view
  controls. Controls inside Workbench's Web Components are found in their open
  shadow roots, by `aria-label`, else `title`.
  The script writes them to `controls`
  in `src/data/screenshots.json`, with a hotspot over every control, and the
  page lays them side by side with an invisible button over each hotspot. A
  small script at the end of `index.astro` captions the hotspot you point at,
  tap, or tab to, using the matching entry in the "All controls" list, so each
  description is written once.
- Before writing the strip, the script checks that each "All controls" list's
  `data-label` attributes in `index.astro` match the workbench's control labels
  in order, and stops with both lists if they don't. When a control is added,
  removed, or renamed, update the list, then rerun.
- The Scenarios and Annotations shots are 1120 × 860. Scenarios shows Feedback's
  five artboards together. Annotations shows the sign-in page's Wrong password
  scenario at mobile width; the script scales page coordinates to the canvas zoom
  when it draws the annotations.
- The Workbench previews and Docs shots are 1120 × 860: the demo's Button
  preview with its controls open, then the same page through its Docs lens
  once every example has rendered.

Keep the demo on Acme and example.com placeholders.

## Product icons

The icons and logos are copies generated from the original design exports,
which are kept outside this package; don't edit them by hand.
The website reads paths and view boxes from `public/icons/` and `public/logos/`.
Studio and Link are stored for future use outside the current product navigation.

Workbench also ships the mark in `../workbench/extension/icon.svg` (white for VS Code),
`../workbench/ui/legacy/canonic.svg` (blue accent with a dark-theme fill),
and `../workbench/extension/icon-source.svg` / `icon.png` (128 px extension listing icon).
These and `public/canonic.svg` use the exported paths, with only presentation
changes for their background, color, or size. Refresh those assets and the
website screenshots when replacing the Workbench exports.
