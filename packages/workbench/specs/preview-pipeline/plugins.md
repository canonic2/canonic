# Plugins

Part of the [preview pipeline](README.md). This spec covers how each native
format becomes Rspack input, and the plugin contract projects use in
`workbench.config.ts`. The rest of the config (adapters, aliases, `dedupe`,
`resolveExtensions`, `define`, `environment`) keeps its meaning, as defined in
[previews](../previews.md#adapters-and-project-configuration).

## Requirements

### Workbench format handling

Workbench supplies these as Rspack loaders, rules and plugins in every
compiler, before any project plugin:

| Concern | Handling |
| --- | --- |
| `@canonic2/workbench` | Resolved to Workbench's bundled API module, whatever the project installs |
| Preview runtime | `/_workbench/preview-runtime.js` is an external read from a window global set by the preview module, so every preview in a document shares one runtime |
| TypeScript, JSX, TSX | Rspack's SWC loader, with React Refresh for project sources outside `node_modules` and generated entries |
| Vue single-file components | Vue Loader, using Workbench's bundled `@vue/compiler-sfc`. Style blocks go through `style-loader` and `css-loader`, because Vue Loader's style proxy does not work with Rspack's native CSS ([finding 7](../../labs/rspack-pipeline/README.md#findings)). A `<style module>` block's class names are its default export, read as `$style` |
| CSS | Rspack's native CSS, with hot updates. Files named `*.module.css` are CSS modules importable as a default object (`namedExports: false`) |
| HTML previews | A Workbench loader that turns the HTML into a string module and turns its local references into imports: linked stylesheets, module scripts, and `src`, `poster` and `srcset` assets. It strips `<title>`, `<meta>` and `<base>` |
| Assets | Images, fonts and media imported by sources are emitted with content-hashed names |
| `tsconfig.json` paths | The project's root `tsconfig.json`, with its references, resolves `paths` such as `@/*` |
| Aliases | `aliases` from the config. Relative targets resolve from the project root, and keys match the whole specifier |
| `dedupe` | Each listed package resolves to the project root's copy |
| `define` | Merged with `process.env.NODE_ENV` set to `development` and Vue's feature flags |
| React Native Web | The `react-native-web` profile ([compiler](compiler.md#one-compiler-per-project-and-profile)) |

### Astro

- Astro components render on the server in the compile worker, through the
  Astro container, as they do today ([previews](../previews.md#data-request-mocks-and-the-project-environment)).
- The rendered HTML and its client resources become virtual modules in the
  compiler, handled by the HTML loader, so the client side gets hot updates
  and shared chunks like any other format.
- Root-relative URLs in the rendered HTML resolve against the project's
  public folder.
- The worker watches each Astro preview's dependencies. On a change it renders
  again and replaces the virtual module only when the output changed; the
  page then re-renders in place ([hot updates](hot-updates.md#edit-behavior)).

### Project plugins

The owner decided on 2026-10-12 that project plugins are Rspack plugins, and
on 2026-10-13 to implement the pipeline with the keys below.

```ts
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({
  plugins: [new AcmeBuildInfo()],                  // Rspack plugins
  rules: [{ test: /\.tsx?$/, enforce: 'pre', loader: './workbench/greeting-loader.cjs' }],
  adapters: {
    'acme-canvas': { runtime: './workbench/canvas-runtime.ts', plugins: [], rules: [] },
  },
});
```

- `plugins` is a list of Rspack plugins: objects with `apply(compiler)`. They
  are applied after Workbench's own plugins, in every compiler of the space.
  An adapter's `plugins` apply only to the compiler that builds that adapter's
  previews.
- `rules` is a list of Rspack module rules, applied after Workbench's own
  rules. Loader paths resolve from the project root.
- Workbench evaluates `workbench.config.ts` with packages external
  ([compiler](compiler.md#what-stays-on-esbuild)). A plugin therefore takes
  Rspack's classes from the compiler it is applied to (`compiler.rspack`),
  rather than importing its own copy of `@rspack/core`, which the project need
  not install.
- A plugin object with esbuild's shape (`name` and `setup`) and no `apply` is
  reported as a problem naming the plugin, and the other previews still load.
- A plugin that throws, or a rule that fails, shows as a build error in the
  previews whose graph it affects ([compiler](compiler.md#build-errors)).

Common esbuild plugin patterns and their Rspack forms, as used by the
[formats fixture](../../labs/fixtures/README.md#the-formats-size):

| esbuild plugin | Rspack form |
| --- | --- |
| A virtual module served by the plugin | `NormalModuleReplacementPlugin` rewriting the request to a module the plugin supplies, such as a `data:` URI with the generated source |
| A source transform while loading files | A loader in `rules`, with `enforce: 'pre'` when it must see the original source |
| An import redirected by the importing file | `NormalModuleReplacementPlugin` testing the request and the issuer |

### Portable output

The portable build and the export's docs examples build with an Rspack
production compiler that uses the same format handling and project plugins,
so projects write one plugin form (decided 2026-10-13,
[pipeline decisions](README.md#decisions)).

## Status

Implemented: Workbench's format handling as Rspack loaders and rules, the
project `plugins` and `rules` keys, problems for esbuild-shaped plugins, and
portable output from an Rspack production build. HTML previews bundle every
inline and local script and run it per render, with top-level declarations
private. An `aliases` entry stands in for an Astro application module, such as
`astro:content`. Gaps:

- Astro frontmatter and Astro input edits still bundle with esbuild, without
  project plugins or rules.

## Discoveries

- **2026-10-11, compiler speed lab.** No candidate bundler runs Workbench's
  esbuild plugins or projects' esbuild plugins unchanged. Rspack needs both
  rewritten as loaders and plugins
  ([capability checklist](../../labs/compiler-speed/README.md#capability-checklist-1)).
- **2026-10-12, stage 1.** All of the table above worked in the prototype,
  and all three of the formats fixture's project plugins worked in their
  Rspack form. They take Rspack's classes from `compiler.rspack`, with
  `compiler.webpack` as a fallback
  ([report](../../labs/rspack-pipeline/README.md#stage-1-report)).

## Open questions

- **Plugins in definition evaluation.** Definitions are evaluated without
  project plugins today. Whether a definition may import a module that only a
  plugin can resolve is not specified.
