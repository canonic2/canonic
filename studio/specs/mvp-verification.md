# MVP verification

## Audit scope

Verified on 2026-10-04 against the current [MVP contract](studio.md) and topic
specs. This audit covers implemented behavior; proposed next work remains outside
the release. No new product features were added.

Environment: macOS x64, Node 24.12.0, Git 2.50.1, Docker 29.7.2, Compose 5.5.0,
with the package's installed Electron and code-server runtimes. These results
establish behavior on this host, not equivalent verification on Linux or other
architectures.

The verified toolchain uses pnpm 10.33.0, TypeScript 5.9.3, and Node 24 types.
Node tests/scripts run .ts source; Electron and renderer code run the clean
compiled build. The full-stack fixture has its own frozen pnpm workspace and
TypeScript check.

## Results

| Check                                | Result                         | Contract evidence                                                                                                                                                               |
| ------------------------------------ | ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| pnpm install --frozen-lockfile       | Passed                         | Pinned package dependency graph                                                                                                                                                 |
| pnpm run typecheck                   | Passed                         | Strict source, desktop, renderer, and test contracts; unused-code checks                                                                                                        |
| pnpm run build                       | Passed                         | Clean ESM JavaScript output and sandboxed CommonJS preload                                                                                                                      |
| Fixture frozen install and typecheck | Passed                         | Separate pnpm workspace; Express/Vite TypeScript compatibility                                                                                                                  |
| pnpm run check                       | Passed                         | Frozen lockfiles, formatting, types, lint, unused-code analysis, module boundaries, configuration/docs contracts, coverage and build checks                                     |
| pnpm run check:full                  | Passed                         | Every standard gate plus all desktop and Docker/Turbo suites and production dependency audit                                                                                    |
| pnpm run doctor                      | Passed                         | Required local tools and IDE present; Docker available                                                                                                                          |
| pnpm test                            | 17 passed, none failed/skipped | Real worktrees, state persistence/rollback, project identity, immutable snapshots, independent runtimes, descendant cleanup, configuration, Compose policy and desktop payloads |
| pnpm run test:coverage               | Passed                         | Core coverage: 96.00% lines, 83.15% branches, 93.46% functions; adapter scope documented in engineering checks                                                                  |
| pnpm run check:security              | Passed                         | Production dependency audit reported no known vulnerabilities at verification time                                                                                              |
| pnpm run smoke:startup               | Passed                         | Empty production-style desktop without fixture projects, IDEs, or runtimes                                                                                                      |
| pnpm run smoke                       | Passed                         | Real extension/theme, edit/save, terminal context, retained views, project/session switching, preview/logs, close/reopen, malformed recipe repair                               |
| pnpm run test:fullstack              | Passed                         | Two Git branches and Turbo/Express/Vite/Redis stacks, six distinct ports, separate data/resources, restart and failed-start cleanup                                             |
| pnpm run smoke:fullstack             | Passed                         | Full-stack contract through real IDE/Preview views, frontend writes, Vite HMR, Express watch restart, and visible session controls                                              |

The desktop full-stack suite was run with the pnpm/TypeScript toolchain. The
final behavioral suite includes its timeout, successful-exit, and failed-exit
regressions. Both full-stack reports assert removal of their owned containers,
networks, volumes, and host process groups.

The [engineering checks](quality-checks.md) are pinned package-local tools and
fail-fast orchestration. Typed lint identified promise handling and external
payload typing that were tightened before acceptance. Unsafe Compose settings,
malformed persisted ownership and invalid desktop payloads have dedicated
regression checks. Formatting, lint, unused-code and dependency analysis all pass
without warnings. Verification ran the complete check:full command on this host.

## Cleanup correction

Provisioning owns a detached process group and must leave no descendants when
it completes, fails, or times out. A regression test demonstrated that killing
only the provision command could leave its child HTTP server listening. The
runtime now keeps that group in its ownership record, waits for successful
completion, and stops remaining descendants before launching the application.
Failure cleanup uses the same owned record, retaining it if cleanup fails.

The regression checks verify closed descendant listeners and a healthy neighboring
session, including a wrapper that exits while its child is still running.

## Evidence and limits

Ignored runtime evidence lives under artifacts: smoke.json, fullstack.json, and
fullstack-desktop.json, plus per-stack logs. These reports are local outputs,
not release files. The checked-in test runners reproduce the acceptance checks.

Host compositor errors prevented some optional screenshots. Required DOM, IDE
API, HTTP, Git, process, and Docker assertions passed independently of captures.

No failing acceptance check remains from this audit. Hard-crash recovery, port
reservation/retry, session deletion, continuous health monitoring, per-service
controls, global unsaved-file handling, and Workbench/Shield integration remain
the documented release limits. Laravel and arbitrary VS Code extensions have
not acquired compatibility guarantees through the Express/Vite fixture.
