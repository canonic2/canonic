# Studio development

Studio uses TypeScript ESM modules and a small Electron desktop shell.
Feature modules separate services, validation, transport, and presentation.
Application wiring and platform adapters keep those responsibilities independent.

The package pins pnpm 10.33.0, TypeScript 5.9.3, and Node 24 types. tsconfig.json
uses strict checking, unused-code checks, NodeNext resolution, and erasable syntax
compatible with native Node TypeScript execution. Relative source imports include
.ts extensions; the compiler rewrites them for JavaScript output.

`pnpm run typecheck` checks production modules, desktop, renderer, scripts, and
test runners. `pnpm run build` cleans and compiles dist/ and copies shell assets.
Electron launches dist/app/main.js. The sandboxed preload is authored as .cts
and emitted as CommonJS .cjs; renderer modules emit browser JavaScript. Node
scripts/tests execute source directly. Check the separate full-stack fixture with
`pnpm --dir test/fixtures/fullstack run typecheck` after installing its workspace.

The Studio and fixture workspaces have separate pnpm-lock.yaml files. Use frozen
installs for reproducibility. Studio explicitly permits Electron's dependency
build script; the fixture recipe installs with dependency scripts disabled.
Build output, package-local stores, user data, and downloaded IDEs stay ignored.

The native source execution constraints follow
[Node's TypeScript documentation](https://nodejs.org/api/typescript.html).
Compiler extension rewriting follows the
[TypeScript module documentation](https://www.typescriptlang.org/docs/handbook/modules/theory.html).

| Area                      | Responsibility                                                                        |
| ------------------------- | ------------------------------------------------------------------------------------- |
| src/application/studio.ts | Composition and cross-module workflows; public application API                        |
| src/modules/projects      | Repository registration, project selection, open/closed tabs                          |
| src/modules/sessions      | Task names, session branches, worktree creation                                       |
| src/modules/git           | Git subprocess arguments and worktree operations                                      |
| src/modules/state         | Validated persisted state and atomic updates with rollback                            |
| src/modules/configuration | Project schema loading, validation, and defaults                                      |
| src/modules/runtime       | Per-session queues, host supervision, Compose ownership and cleanup                   |
| src/modules/ide           | Code OSS startup, profiles, private IPC sockets and lifecycle                         |
| src/platform              | Shared paths, environment, process groups, and queues                                 |
| app/desktop               | Electron windows, workspace views, commands and IPC boundary                          |
| app/renderer              | Project/session/runtime rendering and selectors                                       |
| app/studio.ts             | UI action orchestration                                                               |
| test                      | Behavioral suites, fixture repositories, verification extensions, and desktop runners |

Domain modules do not import Electron or UI code. Project/session logic does not
launch containers or IDEs. Runtime and IDE queues are scoped to a session; state
writes serialize independently. The application coordinates project closing and
whole-app shutdown. Desktop commands validate renderer input and restrict IPC to
the trusted shell frame. Embedded workspaces have no Studio IPC bridge.

Production startup opens user projects and never imports test fixtures or smoke
runners. Retain useful regression fixtures under test rather than branching
production code for tests. Tests use a distinct desktop profile and ignored state.

Run the [package checks](../README.md#validate) after behavior changes. Add tests
for contracts such as isolation, rollback, state reopening, and cleanup, rather
than tests that repeat code or documentation. Schema changes require corresponding
configuration examples and documentation.

`pnpm run check` runs formatting, strict types, typed lint, unused-code analysis,
module dependency rules, contract validation, core coverage and build checks.
`pnpm run check:full` additionally runs real desktop and Docker/Turbo acceptance
suites and the production dependency audit. Use `pnpm run format`,
`pnpm run lint:fix` and `pnpm run test:watch` during development. The
[check specification](../specs/quality-checks.md) defines coverage scope,
thresholds and external prerequisites. Neither command installs Git hooks.

Internal requirements live in specs/ with a README index; reader-facing guides
live in docs/. Use product-specs and product-docs for those changes. These guides
are repository documentation; no website publishing integration is configured.
