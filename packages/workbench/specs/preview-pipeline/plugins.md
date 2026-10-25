# Plugins

Part of the [preview pipeline](README.md). This spec covers how each native
format becomes Rspack input, and the plugin contract projects use in
`workbench.config.ts`. The rest of the config (adapters, aliases, `dedupe`,
`resolveExtensions`, `define`, `environment`) keeps its meaning, as defined in
[previews](../pages/previews.md#adapters-and-project-configuration).

**Status:** Implemented in the native compiler in `workbench-server`
([compiler/](../../server/src/compiler.rs), [plugins/](../../server/src/compiler/plugins.rs)):
Workbench's format handling, the `plugins` capabilities, project `rules` with
JavaScript loaders run in the Node evaluator
([node_loader.rs](../../server/src/compiler/node_loader.rs)), problems for
JavaScript plugin objects and invalid options, and portable output from a
production build. Not implemented: project plugins and rules for Astro's
server-side bundling ([implementation gaps](#implementation-gaps)).

## Requirements

### Workbench format handling

Workbench supplies these as Rspack loaders, rules and plugins in every
compiler, before any project plugin:

| Concern | Handling |
| --- | --- |
| `@canonic2/workbench` | Resolved to Workbench's bundled API module, whatever the project installs |
| Preview runtime | `/_workbench/preview-runtime.js` is an external read from a window global set by the preview module, so every preview in a document shares one runtime |
| TypeScript, JSX, TSX | Rspack's SWC loader, with React Refresh for project sources outside `node_modules` and generated entries |
| Vue single-file components | Workbench's Vue loaders, with the Node evaluator compiling each component ([Vue adapter](../adapters/vue.md#build)) |
| CSS | Rspack's native CSS, with hot updates. Files named `*.module.css` are CSS modules importable as a default object (`namedExports: false`); their custom properties keep their written names, as in Vite, so other stylesheets and SVG sprites can read them |
| HTML previews | Workbench's HTML loader ([HTML adapter](../adapters/html.md#build)) |
| Assets | Images, fonts and media imported by sources are emitted with content-hashed names |
| `tsconfig.json` paths | The project's root `tsconfig.json`, with its references, resolves `paths` such as `@/*` |
| Aliases | `aliases` from the config. Relative targets resolve from the project root, and keys match the whole specifier |
| `dedupe` | Each listed package resolves to the project root's copy |
| `define` | Merged with `process.env.NODE_ENV` set to `development` and Vue's feature flags |
| React Native Web | The `react-native-web` profile ([React Native Web adapter](../adapters/react-native-web.md#build)) |
| Astro | Server rendering, then the HTML loader for the client side ([Astro adapter](../adapters/astro.md#build)) |

### Project plugins

Project configuration supplies compiler plugins and module rules:

```ts
import { defineConfig, virtualModule, resolveImport, importGlob, svgSprite } from '@canonic2/workbench';

export default defineConfig({
  plugins: [
    virtualModule({ id: 'virtual:acme-build-info', source: "export const buildInfo = 'Acme build 42';" }),
    resolveImport({ request: '@acme/platform', importer: 'src/native/**', to: './src/platform/native.ts' }),
    importGlob(),
    svgSprite({ dirs: ['src/icons'], symbolId: 'icon-[name]' }),
  ],
  rules: [{ test: /\.tsx?$/, enforce: 'pre', loader: './workbench/greeting-loader.cjs' }],
  adapters: {
    'acme-canvas': { runtime: './workbench/canvas-runtime.ts', plugins: [], rules: [] },
  },
});
```

- `plugins` is a list of compiler capabilities, plain objects that name their
  `plugin`, made by the `@canonic2/workbench` helpers. They apply after
  Workbench's own handling, in every compiler of the space. An adapter's
  `plugins` apply only to the compiler that builds that adapter's previews.
- Each capability is implemented in the native compiler; the project's paths,
  identifiers and mappings are its options. The implementations know no
  consuming project.

  | Helper | Capability |
  | --- | --- |
  | `virtualModule({ id, source })` | A module made from source text, imported by its exact ID |
  | `resolveImport({ request, to, importer? })` | An exact import resolved to another file, from every importer or from importers matching globs over the project-relative path or a regular expression over the absolute path; the first match wins |
  | `importGlob({ include? })` | Vite's `import.meta.glob`, expanded at build time: literal patterns from `./`, `../` or `/`, negative patterns, `eager`, `import`, `query` and `exhaustive`; keys as written, in name order |
  | `svgSprite({ dirs, symbolId?, register?, names?, inject?, domId? })` | `vite-plugin-svg-icons`' registration and names modules: each icon a `<symbol>` with its view box, IDs prefixed by the symbol ID with their references, the first stroke `currentColor` |

- `rules` is a list of Rspack module rules, applied after Workbench's own
  rules: `test`, `include`, `exclude`, `resource`, `resourceQuery`,
  `issuer`, `enforce`, `type`, `sideEffects`, `loader` with `options`,
  `use`, `oneOf` and `rules`. A string condition is a path from the project
  root. Loader paths resolve from the project root, as Node resolves them.
- Workbench evaluates `workbench.config.ts` in the Node evaluator with
  packages external ([compiler](compiler.md#what-runs-in-the-node-evaluator))
  and passes it to the compiler as data: regular expressions are carried
  over, and a function the compiler would need is a problem.
- An object with `apply(compiler)` (a JavaScript Rspack plugin) or
  `setup(build)` (an esbuild plugin), or any other plugin shape, produces a
  problem naming the plugin while other previews remain available. It does
  not run.
- A plugin or rule that fails shows as a build error in the previews whose
  graph it affects ([compiler](compiler.md#build-errors)). An invalid plugin
  or rule option is a configuration problem for that plugin or rule.
- Each capability records the files and folders it reads, so adding, editing
  or deleting an icon or a glob-matched file rebuilds, and hot updates and the
  persistent cache invalidate correctly. `importGlob` parses source
  expressions, so comments and string literals are not rewritten, and several
  calls in one module do not collide in generated names.

Common extension points:

| Task | Form |
| --- | --- |
| Supply a virtual module | `virtualModule` |
| Transform source | A loader in `rules`, with `enforce: 'pre'` when it needs original source |
| Redirect an import by its issuer | `resolveImport` with `importer` |
| Expand a file glob at build time | `importGlob` |
| Build an icon sprite from a folder | `svgSprite` |

### Portable output

The portable build and the export's docs examples build with a production
compiler that uses the same format handling and project plugins, so
projects write one plugin form.

### Where each part runs

In the Rust Workbench server ([processes and connections](../system/server.md#processes-and-connections)),
the compiler is Rspack's Rust crates, and only project JavaScript leaves the
server, for the Node evaluator:

- **Workbench's format handling is native.** SWC with React Refresh, native
  CSS, assets, `tsconfig.json` paths, aliases, `dedupe`, `define`, the
  `@canonic2/workbench` and preview runtime resolution, and the React Native
  Web profile are options of Rspack's Rust crates. The HTML loader and part
  loader are Rust loaders ([HTML adapter](../adapters/html.md#build)). Vue
  and Astro split their work as their adapters decide
  ([Vue](../adapters/vue.md#decisions), [Astro](../adapters/astro.md#decisions)).
- **`workbench.config.ts` is evaluated in the Node evaluator.** Its
  declarative settings (`aliases`, `dedupe`, `resolveExtensions`, `define`,
  plugins, and each rule's conditions) become compiler options in the server.
- **Project `rules`:** a rule whose loader is one of Rspack's built-in loaders
  runs natively; a JavaScript loader runs in the Node evaluator, one module
  at a time, over the evaluator's private channel, and its result returns to
  the compilation.
- **Project plugin capabilities run in Rust.** The transformations, virtual
  modules and resolver hooks are native compiler plugins. Project-specific
  paths and options remain configuration; the implementation stays
  independent of a consuming project. JavaScript `apply(compiler)` objects
  are not the extension interface, and there is no Node compiler fallback for
  them.
- **Portable output** uses the same native compiler in production mode, with
  the same rules and the same answer for plugins.

## Verification

[Preview integration tests](../../server/node/preview.test.js) cover plugin
capabilities, module rules, format handling and portable builds; the
compiler's Rust unit tests cover each capability's transformation. Together
they cover SVG symbol IDs, view boxes, ID and reference prefixing,
registration and names modules; glob ordering and keys, default and named
imports, several calls without name collisions, comments and strings left as
written; importer-dependent resolution beside the default; a loader rule run
in Node with options; and JavaScript plugins reported as problems. Test gaps:

- Rebuilding after an icon or a glob-matched file is added, edited or deleted
  relies on the modules' recorded folder and file dependencies; no test
  edits an icon or a matched file in a running server.
- Live and docs-example output use the same rules and plugins as the
  portable build, but parity between them is not compared in a test.

Plugin behavior in server-side definition and Astro evaluation must be assessed
separately from browser compilation; successful browser output does not
establish that path.

## Implementation gaps

- Astro frontmatter and Astro input edits bundle with esbuild in the Node
  evaluator, without project plugins or rules.

## Decisions

Decided by the owner on 2026-10-19: plugin implementations are native Rust
capabilities, preserving the capabilities consuming projects use, rather than
a JavaScript compiler that executes their hooks. The registration interface is
the `plugins` list of capability descriptions above.

The production use cases inspected for this decision are SVG sprite virtual
modules, eager `import.meta.glob` expansion, and import resolution based on the
importing source. They need project-agnostic implementations with configured
directories, identifiers and mappings. The formats lab also exercises a
virtual build-info module, source transformation and platform-specific
resolution; its JavaScript plugins are fixtures, not a production
compatibility requirement.

## Open questions

- **Plugins in definition evaluation.** Definitions are evaluated without
  project plugins. Whether a definition may import a module that only a
  plugin can resolve is not specified.
