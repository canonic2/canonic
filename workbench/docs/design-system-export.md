# Design-system export

**More** › **Download design-system ZIP** packages the whole workbench, not just
the current screen, into an archive an agent or another tool can work from:
every design page, implementation source, and TypeScript preview the workbench
points at, what they import, reference screenshots, and a README per screen.

Use it to give an agent the complete context for a redesign or an
implementation, or to check whether anything changed since the last export.

The export needs the workbench server, in VS Code or
[standalone](extension.md#running-without-vs-code). It doesn't work from a
page opened from disk.

## What goes in

The export starts from every screen's entry points:

- the design page (`src`),
- its [code pointers](lenses.md#point-at-the-code),
- for Storybook, each story's file and component, from Storybook's index,
- for a [TypeScript preview](workbench-previews.md), its definition, its source
  file, and the local files the compiler resolves for it.

From there it follows local imports and referenced assets, and includes:

| Included | Notes |
| --- | --- |
| `workbench.yaml` | At the archive root. |
| Design pages and their local CSS, scripts, images, and fonts | Paths stay project-relative. |
| Implementation sources and their local imports | Workspace files keep their project-relative paths at the archive root. Sources outside the workspace go under `implementations/`. |
| Linked local packages | Followed through their `package.json` exports, instead of being treated as installed dependencies. |
| Storybook configuration | From `.storybook`, or a `--config-dir`/`-c` named in a local package's Storybook scripts. |
| Asset folders referenced statically | `new URL("./assets/", import.meta.url)` includes the folder, which covers plugins that build sprites or font sets from a directory. |
| SVG sprite sources | Source SVGs for a Vite `iconDirs` folder named through `path.resolve(process.cwd(), "…")`. |
| Package configuration | The `package.json`, `tsconfig`/`jsconfig`, and Vite, Tailwind, and PostCSS configuration files the included sources need. |
| Reference screenshots | One JPEG per design state, imported story, and TypeScript preview state, at each of the screen's viewports. |
| Compiled TypeScript previews | A `browser/` viewer you can open without Workbench. See [TypeScript previews in the export](#typescript-previews-in-the-export). |

TypeScript imports written with `.js` extensions resolve to the `.ts` or
`.tsx` file when the JavaScript file doesn't exist. Imports starting with `@/`
or `~/` resolve to the `src/` folder.

When the project has no root `package.json`, the export adds one that lists the
external packages the sources import, so you can install them before running
the copied setup.

Left out: installed dependencies (`node_modules`), build output, secrets,
tests, and any source the workbench doesn't reach.

## Reference screenshots

The export captures a reference JPEG for every declared state of every design
screen, every story of every imported Storybook title, and every state of every
TypeScript preview, at each of the screen's
[`viewports`](pages-and-states.md#viewports):

| Viewport | Captured at |
| --- | --- |
| `desktop` | 1512 × 982 |
| `mobile` | 393 × 852 |
| `responsive` | Both 1512 × 982 and 393 × 852 |
| `fit` | 1440 × 900 |

A screen without `viewports` is captured at all three sizes. Duplicate sizes
are removed, so `desktop` plus `responsive` captures each size once.

Design screens are captured as designed, with actions off; lenses on a design
screen aren't captured.

Capture runs as a background job with a progress bar over the canvas. It uses
up to four renderers in parallel and doesn't change what the canvas shows. A
screenshot that fails, or a screen that can't be captured, such as an
[iOS Simulator](ios-simulator.md) screen or one whose design file is missing,
is listed under `captureWarnings` in `canonic-export.json`, and the rest of the
export continues. When the download finishes, a message gives the total
number of screenshot and TypeScript preview build warnings.

## TypeScript previews in the export

When the project has [TypeScript previews](workbench-previews.md), the archive
also contains a `browser/` folder with each preview compiled for the browser.
To view them, serve the extracted archive with any static HTTP server and open
`browser/index.html`. You don't need Workbench, Electron, or the project's
packages installed.

The viewer lets you search previews, pick a state and a viewport, and use the
same preview controls as the canvas. See
[Portable exports](workbench-previews.md#portable-exports) for its controls and
what each adapter supports there. To build the viewer on its own, without the
rest of the export, use the `build` command in
[Command-line tools](workbench-previews.md#command-line-tools).

The original definitions and sources stay editable at their project-relative
paths. When the project has no `workbench-env.d.ts`, the export adds one with
the `@canonic/workbench` types. `browser/workbench.json` lists the previews and
their states, and `canonic-export.json` lists the compiled entries and any
previews that failed to build under `browser`.

## The archive

When everything fits in one 10 MB ZIP, the download is that ZIP, named after
the workbench's `name`, such as `acme-design-system.zip`. A larger export
downloads as one outer ZIP of numbered parts:

```text
acme-design-system-parts.zip
├── README.md
├── acme-design-system-part-01-of-03.zip
├── acme-design-system-part-02-of-03.zip
└── acme-design-system-part-03-of-03.zip
```

- Each part is at most 10,000,000 bytes, for tools with a 10 MB attachment
  limit.
- Every part extracts into the same top-level folder, and repeats the global
  `README.md` and `canonic-export.json`.
- Each part has a `canonic-export-part-NN.json` listing the files in it.

To use a split export, extract the outer ZIP. Then either upload all the
numbered parts together, or extract them all into one folder to get the
complete tree.

Inside the extracted tree:

| File | Contents |
| --- | --- |
| `README.md` | What the export is and how to use it. |
| `canonic-export.json` | The manifest: `files`, external packages under `dependencies`, one record per screen under `screens`, unresolved references under `warnings`, failed screenshots under `captureWarnings`, compiled TypeScript previews under `browser`, and the archive's `parts`. |
| `<screen folder>/README.md` | Beside each screen's main page or component: its hash, entry points, included files, reference screenshots, and guidance for an agent. |
| `<screen folder>/screenshots/` | That screen's reference JPEGs. |
| `browser/` | The compiled TypeScript preview viewer, when the project has previews. |

When several screens share a folder, each gets its own
`<screen name>.README.md` and `screenshots/<screen name>/` folder, so they
don't collide. A screen with no design or source entry point goes under
`screens/`.

## Content hashes

Each screen record, and each screen README, has a SHA-256 hash over the
screen's files: their sorted archive paths and raw contents. If a screen's
hash matches between two exports, its design and source entry points and
everything they import locally are unchanged.

Generated READMEs, screenshots, archive metadata, and package configuration
added by the exporter aren't part of the hash, so recapturing screenshots
doesn't change it.

## Tips

- Set `root` on every implementation and add `code` pointers, so the export
  includes implementation sources and not only design pages.
- Check `warnings` in `canonic-export.json` for imports the exporter couldn't
  follow, such as aliases it doesn't understand.
- Check `captureWarnings` for missing screenshots, and `browser.warnings` for
  TypeScript previews that didn't build.
- A preview that fails to build is a warning, but if the preview worker can't
  build the viewer at all, the whole export fails. Read **Workbench: Show Log**,
  or run the `check` command in
  [Command-line tools](workbench-previews.md#command-line-tools) to find the
  failing preview.
