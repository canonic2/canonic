# Design-system export

**More** › **Download design-system ZIP** packages the whole workbench, not just
the current screen, into an archive an agent or another tool can work from:
every design page and implementation source the workbench points at, what they
import, reference screenshots, and a README per screen.

Use it to give an agent the complete context for a redesign or an
implementation, or to check whether anything changed since the last export.

The export needs the workbench server, in VS Code or
[standalone](extension.md#running-without-vs-code). It doesn't work from a
page opened from disk.

## What goes in

The export starts from every screen's entry points:

- the design page (`src`),
- its [code pointers](lenses.md#point-at-the-code),
- for Storybook, each story's file and component, from Storybook's index.

From there it follows local imports and referenced assets, and includes:

| Included | Notes |
| --- | --- |
| Design pages and their local CSS, scripts, images, and fonts | Paths stay project-relative. |
| Implementation sources and their local imports | Workspace files keep their project-relative paths at the archive root. Sources outside the workspace go under `implementations/`. |
| Linked local packages | Followed through their `package.json` exports, instead of being treated as installed dependencies. |
| Storybook configuration | From `.storybook`, or a `--config-dir`/`-c` named in a local package's Storybook scripts. |
| Asset folders referenced statically | `new URL("./assets/", import.meta.url)` includes the folder, which covers plugins that build sprites or font sets from a directory. |
| SVG sprite sources | Source SVGs for a Vite `iconDirs` folder named through `path.resolve(process.cwd(), "…")`. |
| Package configuration | The `package.json` and configuration files the included sources need. |
| Reference screenshots | One JPEG per design state and per imported story, at each of the screen's viewports. |

TypeScript imports written with `.js` extensions resolve to the `.ts` or
`.tsx` file when the JavaScript file doesn't exist.

Left out: installed dependencies (`node_modules`), build output, secrets,
tests, and any source the workbench doesn't reach.

## Reference screenshots

The export captures a reference JPEG for every declared state of every design
screen, and every story of every imported Storybook title, at each of the
screen's [`viewports`](pages-and-states.md#viewports):

| Viewport | Captured at |
| --- | --- |
| `desktop` | 1512 × 982 |
| `mobile` | 393 × 852 |
| `responsive` | Both 1512 × 982 and 393 × 852 |
| `fit` | 1440 × 900 |

Duplicate sizes are removed, so `desktop` plus `responsive` captures each size
once.

Capture runs as a background job with a progress bar over the canvas. It uses
up to four renderers in parallel and doesn't change what the canvas shows. A
screenshot that fails, or a screen that can't be captured, such as an
[iOS Simulator](ios-simulator.md) screen, is listed under `captureWarnings` in
`canonic-export.json`, and the rest of the export continues.

## The archive

The download is one ZIP, named after the workbench's `name`, containing
numbered parts:

```text
acme-design-system-parts.zip
├── acme-design-system-part-01-of-03.zip
├── acme-design-system-part-02-of-03.zip
└── acme-design-system-part-03-of-03.zip
```

- Each part is at most 10,000,000 bytes, for tools with a 10 MB attachment
  limit.
- Every part extracts into the same top-level folder, and repeats the global
  `README.md` and `canonic-export.json`.
- Each part has a `canonic-export-part-NN.json` listing the files in it.

To use it, extract the outer ZIP. Then either upload all the numbered parts
together, or extract them all into one folder to get the complete tree.

Inside the merged tree:

| File | Contents |
| --- | --- |
| `README.md` | What the export is and how to use it. |
| `canonic-export.json` | The manifest: `files`, external packages under `dependencies`, one record per screen under `screens`, unresolved references under `warnings`, and failed screenshots under `captureWarnings`. |
| `<screen folder>/README.md` | Beside each screen's main page or component: its hash, entry points, included files, reference screenshots, and guidance for an agent. |
| `<screen folder>/screenshots/` | That screen's reference JPEGs. |

When several screens share a folder, their README and screenshot names include
the screen's name, so they don't collide.

## Content hashes

Each screen record, and each screen README, has a SHA-256 hash over the
screen's files: their sorted archive paths and raw contents. If a screen's
hash matches between two exports, its design and source entry points and
everything they import locally are unchanged.

Generated files, screenshots, and package configuration added by the exporter
aren't part of the hash, so recapturing screenshots doesn't change it.

## Tips

- Set `root` on every implementation and add `code` pointers, so the export
  includes implementation sources and not only design pages.
- Check `warnings` in `canonic-export.json` for imports the exporter couldn't
  follow, such as aliases it doesn't understand.
- Check `captureWarnings` for missing screenshots.
