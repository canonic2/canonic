# Lattice

Lattice is an unpublished native JavaScript package for state and Web Component
bindings. Its root ESM entry point is still empty; `canonic-lattice/elements`
exports the element helpers `captureInputs`, `replayInputs`,
`skipUpgradeAttribute` and `defineOnce`.
It has no runtime dependencies.

Source files run unchanged in modern browsers. There is no production build,
bundler, transpilation or CommonJS entry point. TypeScript checks JavaScript and
JSDoc with `noEmit`; it does not produce the shipped code.

## Development

Use Node 24.12 or newer and pnpm 10.33.0. From the repository root:

```sh
pnpm --dir packages/lattice install --frozen-lockfile
pnpm --dir packages/lattice run browser:install
```

Run categories independently:

| Category                          | Command from the repository root                     |
| --------------------------------- | ---------------------------------------------------- |
| Lint, including JSDoc             | `pnpm --dir packages/lattice run lint`               |
| Apply safe lint fixes             | `pnpm --dir packages/lattice run lint:fix`           |
| Apply formatting                  | `pnpm --dir packages/lattice run format`             |
| Check formatting                  | `pnpm --dir packages/lattice run format:check`       |
| Check JavaScript/JSDoc types      | `pnpm --dir packages/lattice run typecheck`          |
| Node unit tests                   | `pnpm --dir packages/lattice test`                   |
| Watch unit tests                  | `pnpm --dir packages/lattice run test:watch`         |
| Chromium tests through Playwright | `pnpm --dir packages/lattice run test:browser`       |
| Watch browser tests               | `pnpm --dir packages/lattice run test:browser:watch` |

Append a test path to the relevant test command for focused verification.
Browser tests use a real browser, not a DOM emulator. Vitest's development server
is test infrastructure; it is not a production build requirement.

Oxlint runs native JSDoc rules and the `require-jsdoc` rule from
`eslint-plugin-jsdoc` through its JavaScript plugin API. No ESLint CLI runs.
Oxfmt formats source, configuration and documentation. Dependencies are pinned
in `package.json` and `pnpm-lock.yaml`.

Oxlint's JavaScript plugin API is currently alpha. Tooling fixtures verify the
pinned combination against missing module/function/private-method documentation,
incorrect parameter names and a JSDoc type mismatch. TypeScript 6.0.3 is pinned
to satisfy the JSDoc plugin's current TypeScript compatibility range.

The [specifications](specs/README.md) hold the agreed
[contracts](specs/contracts/README.md), the element authoring shape in the
framework folder and draft implementation designs in the architecture folder.
[Research](research/README.md) records the measurements behind decisions.
Contributor language, JSDoc and tooling instructions are supplied directly by
the package's agent rules.
