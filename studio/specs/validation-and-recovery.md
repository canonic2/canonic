# Validation and recovery

## Required evidence

Isolation needs real worktrees, processes, browser views, and Docker resources.
Mocks or screenshots alone cannot prove two sessions are independent.

| Layer                     | Command from packages/studio | Evidence                                                                                             |
| ------------------------- | ---------------------------- | ---------------------------------------------------------------------------------------------------- |
| Engineering gates         | pnpm run check               | Frozen dependencies, formatting, types, lint, unused code, boundaries, contracts, coverage and build |
| Full acceptance           | pnpm run check:full          | All engineering gates, desktop and Docker/Turbo suites, production dependency audit                  |
| Application/configuration | pnpm test                    | Worktrees, state/rollback, parallel runtime ownership, strict recipes                                |
| Empty desktop             | pnpm run smoke:startup       | No fixtures/processes and disabled empty controls                                                    |
| IDE desktop               | pnpm run smoke               | Extension, theme, editing, terminal, switching, previews, close/reopen                               |
| Full-stack runtime        | pnpm run test:fullstack      | Two branches/Turbo stacks, six ports, two Redis stacks, teardown                                     |
| Full-stack desktop        | pnpm run smoke:fullstack     | Real views, UI navigation, independent writes and hot reload                                         |

Docker suites need a daemon/Compose and dependency installation. Desktop suites need
the installed IDE. Compositor capture failures are recorded separately; DOM, API,
Git, process, and Docker assertions remain mandatory. Reports use ignored artifacts
and tests use separate state profiles.

## Full-stack acceptance scenario

Create a temporary repository from the pinned Express/Vite/Turbo/Redis fixture and
two managed branch sessions. Start concurrently. Assert six distinct live ports,
with each frontend reaching its own backend and Redis.

Inspect container labels, networks, and volumes. Write different counters through
both frontends and verify independently, including direct container reads. Edit
only A and observe its Vite HMR and Express watch restart; B's source, response,
and process stay unchanged.

Edit A's recipe/YAML while running, then Stop using the saved plan. All A listeners
and containers stop while B stays healthy. Restart A with retained data. Fail host
startup after Compose has started and verify cleanup without disturbing B.

Finally shut down both and remove only owned fixture volumes/networks, asserting
absence. Passing tests cannot leave stacks running or use global prune for teardown.

## Failure contracts

| Failure                                            | Current behavior                                                           |
| -------------------------------------------------- | -------------------------------------------------------------------------- |
| Invalid recipe                                     | Runtime fails before provision; IDE can open to repair                     |
| Invalid Git base                                   | No published session record                                                |
| Persistence failure after Git creation             | Registered rollback; cleanup failures surfaced                             |
| Provision/Compose/readiness failure                | Attempt cleanup; retain failed cleanup ownership for retry                 |
| Provision wrapper exits or times out with children | Stop the provision process group, including remaining descendants          |
| One cleanup fails                                  | Attempt the others and aggregate failures                                  |
| Project close cleanup fails                        | Project remains open                                                       |
| Unexpected host exit                               | Host status stopped; explicit lifecycle operations clean remaining Compose |
| Port conflict                                      | Failure, without automatic reallocation                                    |
| Malformed state                                    | Error, without silent reset                                                |
| Hard crash                                         | No automatic reconciliation/recovery                                       |

Shutdown aggregates failures after attempting runtime and IDE cleanup. Atomic state
replacement is not a transaction with Git/Docker/process creation. Runtime ownership
is in memory; there is no fsync-backed journal or automatic restart adoption.

## Proposed next work

Persist ownership before side effects and record progress in a recovery journal.
Reconcile it with Git registrations, Docker labels, checkout paths, and process
identity at startup. A PID alone is insufficient because it may be reused.

Offer scoped reopen, cleanup retry, adoption, or removal for verified resources.
Preserve dirty files and volumes by default. Names/prefixes alone cannot authorize
deletion; ambiguous ownership needs diagnostics rather than automatic cleanup.

Add crash injection between creation, persistence, Compose up, host launch, and
shutdown before claiming recovery. Add daemon-loss and port-race tests alongside
their future behavior. Each supported platform needs equivalent descendant cleanup
and IDE profile/socket validation.

## Source

[Application tests](../test/application.test.ts),
[configuration tests](../test/configuration.test.ts),
[IDE smoke](../test/integration/ide-smoke.ts),
[full-stack runner](../test/integration/fullstack.ts), and
[browser assertions](../test/integration/fullstack-browser.ts) own the checks.
The [MVP overview](studio.md) defines release scope; topic specs define contracts.
