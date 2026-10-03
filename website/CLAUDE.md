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
- The extension's `specs/` holds internal specifications and is not published.
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

---

# Product specifications and documentation

Use `product-specs` to maintain internal requirements, workflows, system contracts,
decisions, discoveries, acceptance criteria, and known gaps. Use `product-docs`
to maintain reader-facing guides, references, and troubleshooting. The package's
writing rule defines its source locations, audience, publishing, and validation.

- Read the relevant specification before changing product behavior. Update it
  when a requirement, decision, workflow, or material discovery changes. Separate
  agreed requirements, proposals, verified behavior, implementation gaps, and
  open questions. Existing code or passing tests do not establish product intent.
- Record durable discoveries with evidence and rationale. Label hypotheses;
  include dates and conditions for measurements where relevant. Keep current
  requirements prominent rather than accumulating a conversation transcript.
- Update affected documentation when supported behavior changes. Verify claims,
  examples, labels, commands, defaults, and recovery instructions against the
  product. A specification does not establish that a proposed feature is available.
- Docs, READMEs, and agent instructions describe only what exists. When a
  feature, key, command, file, or route is removed or replaced, delete or rewrite
  the text about it; do not keep it with a note that it was removed, renamed, or
  is "no longer" supported, and avoid "previously", "formerly", and "now" phrasing.
  History belongs in the changelog, release notes, and commit messages.
- Keep internal requirements and investigation notes in specs. Include technical
  detail in docs when it helps the intended audience use, configure, integrate,
  or troubleshoot the product. Prefer existing topics and cross-links over copies.
- Check links and run the package's relevant validation. Do not add tests that
  merely assert documentation text. Writing a spec does not authorize implementing
  proposals; writing docs does not itself authorize publishing or distribution.

---

# Website writing locations and checks

- Internal specs for the website itself: `packages/website/specs/`, indexed by
  `specs/README.md` when its first specification is written. These cover website
  behavior, navigation, content generation, accessibility, and product presentation;
  each product's own requirements stay in that product's specs.
- Reader-facing website content lives in `packages/website/src/pages/` and its
  existing content sources. `packages/website/README.md` is the maintainer guide
  for building, screenshots, and publishing. There is no separate website docs
  collection today; do not create duplicate content to fit a generic directory.
- Workbench user guides are owned by `packages/workbench/docs/` and loaded by
  `src/content.config.ts`. Edit those guides at their source. Website specs and
  maintainer notes must not enter the Workbench collection or search index.
- Follow `website-update` for site structure and `website-workbench` for the
  Workbench overview and screenshots. Match supported product capabilities;
  download and release data come from GitHub rather than invented versions.
- For site content or behavior changes, run `pnpm --dir packages/website test`
  and `pnpm --dir packages/website run build`, then inspect affected output.
  Follow the relevant website skill for visual checks when layout or screenshots
  change. Internal Markdown-only specs need link review and `git diff --check`.
- Writing content does not itself authorize deployment. Use `website-release`
  when publishing is requested.
