# Website — Package Instructions

The Canonic website, served at `https://canonic.sh/`. It covers four products
(Workbench, Sandbox, Playground, Shield). Only Workbench has pages so far:
an overview (the home page), Docs, Install, and Changelog. `README.md`
explains the build, the screenshots, and publishing.

## Layout

- `src/pages/index.astro`: the Workbench overview.
- `src/pages/workbench/`: the Docs pages (built from the Workbench guides),
  the Install page, and the Changelog.
- `src/layouts/Base.astro`: the product bar, section nav, and footer.
- `src/lib/`: `workbench.js` (page URLs, nav, release data), `docs.js` (guide
  order and link rewriting), `releases.js` (release selection and notes),
  `marks.js` (logo and product marks), each with tests where they have logic.
- `src/styles/global.css` and `docs.css`: tokens and components.
- `src/data/screenshots.json`, `public/images/`, `screenshots/`: Workbench
  screenshots, their regions, and the fixture and script that capture them.

## Rules

- Astro with static output and pnpm. No UI framework or client islands; a
  small `is:inline` script is fine for progressive enhancement, and pages must
  work without it. Add a dependency only when the user agrees.
- Every page uses `Base.astro`. Link files in `public/` through `base` from
  `src/lib/base.js`, never with a leading `/`.
- Every color is a token in `global.css` with light and dark values.
- The Workbench guides live in the extension's `docs/`. Edit them there, not
  on the site.
- Versions, download links, and the changelog come from GitHub's releases at
  build time. Never hardcode a version.
- Links that leave the site open in a new tab (`target="_blank"
  rel="noopener"`) and carry a `↗`.
- Never hand-edit `src/data/screenshots.json`; `screenshots/capture.cjs`
  writes it. Use Acme and example.com in copy and screenshots, and keep copy
  plain and factual.
- Use only exported product marks; never redraw one.

## Commands

From this folder (or with `pnpm --dir packages/website` from the repository
root):

| Change | Command |
| --- | --- |
| Any change | `pnpm test` and `pnpm run build` |
| Local preview | `pnpm run dev` |
| Screenshots | `node screenshots/capture.cjs` (needs the extension beside this folder), then view `public/images/` |

## Publishing

Pushing a `website/v<major>.<minor>.<patch>` tag deploys the site. GitHub
Pages names each deployment after its commit, and a second deploy from an
already-deployed commit leaves the first one live. Put each website tag on a
commit that has never been deployed, never on a `workbench/v*` tag's commit.
Never push a tag without the user's go-ahead.
