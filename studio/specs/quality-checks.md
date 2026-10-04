# Engineering checks

Studio keeps its engineering checks inside this package. Run commands from
packages/studio with the pinned pnpm version and Node 24+. Checks fail on errors;
missing external prerequisites are failures in the full suite.

## Standard check

`pnpm run check` runs the following gates sequentially and stops at the first
failure. It requires installed Studio dependencies and network access when the
fixture dependencies are not already cached; it does not require Docker or an IDE.

| Command           | Contract                                                                             |
| ----------------- | ------------------------------------------------------------------------------------ |
| check:lockfile    | Frozen, offline lockfile validation without dependency build scripts                 |
| fixture:install   | Frozen independent fixture workspace with dependency scripts disabled                |
| format:check      | Consistent source, configuration and documentation formatting                        |
| typecheck         | Strict types for application, desktop, renderer, scripts and tests                   |
| typecheck:fixture | Express and Vite fixture type compatibility                                          |
| lint              | Typed ESLint, promise handling, type imports and zero warnings                       |
| analyze           | Unused files, exports and dependencies through Knip                                  |
| circular          | No dependency cycles, unresolved imports or forbidden module dependencies            |
| check:contracts   | Source syntax, JSON, recipe examples, local documentation links and pinned toolchain |
| test:coverage     | Behavioral Node tests and enforced core coverage thresholds                          |
| build             | Clean TypeScript compilation and shell asset copying                                 |
| check:build       | Required assets, emitted JavaScript syntax, preload isolation and rewritten imports  |

Unit coverage includes the application, configuration, Git, projects, sessions,
state, host runtime supervision, Compose policy and platform helpers. Minimum
aggregate coverage is 95% lines, 80% branches and 90% functions. The IDE launcher
and Docker Compose subprocess adapter are excluded from the unit coverage gate:
their behavior is verified by the real integration suites below. Desktop and
renderer coverage is established through interaction assertions, not this metric.

Module rules keep Electron and desktop presentation out of src, application
orchestration out of feature modules, and runtime/IDE launchers out of project and
session modules. Renderer runtime imports cannot access Node or Electron.
Production cannot depend on fixtures. Type-only imports do not create runtime
dependencies.

## Full acceptance check

`pnpm run check:full` runs every standard gate, followed by smoke:startup, smoke,
test:fullstack, smoke:fullstack and check:security. Install the pinned IDE with
`pnpm run setup` first. A desktop environment, Git, Docker with Compose and
network access are required. Desktop suites run sequentially to avoid competing
for the test profile.

The suites exercise empty startup, real editing and extensions, multiple Git
worktrees, independent Turbo/Express/Vite/Redis runtimes, hot reload, persistence
and owned-resource cleanup. Optional screenshots do not replace required DOM,
IDE API, HTTP, Git and Docker assertions. See [validation](validation-and-recovery.md)
and the dated [verification results](mvp-verification.md).

The security gate checks production dependency advisories at high severity or
above using pnpm audit. It requires registry access and does not constitute a
source security audit or a guarantee about development dependencies or arbitrary
project scripts.

## Developer workflow

Use `pnpm run format` and `pnpm run lint:fix` for automatic fixes, and
`pnpm run test:watch` while changing core behavior. Run the standard check before
review and the full check before accepting changes to session or runtime
contracts. Both commands are suitable for CI invocation; this package does not
install repository hooks or configure a hosted CI service.

Generated builds, stores, runtime profiles, logs and reports stay ignored.
The build copies the proprietary package LICENSE into dist/; check:build
verifies that the distributed notice matches the package source.
Coverage thresholds and dependency exceptions must reflect a stated testing or
module contract; lowering a gate to accommodate a regression is not acceptance.
