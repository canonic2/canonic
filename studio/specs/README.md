# Studio developer specifications

Start with the MVP overview, then read the ownership and lifecycle topics before
changing a module. These are internal engineering contracts; reader-facing
instructions live in [docs](../docs/README.md).

| Specification                                         | Covers                                                                                                 |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| [Studio MVP](studio.md)                               | Project/session hierarchy, module boundaries, configuration, runtime ownership, validation, and limits |
| [Projects and sessions](projects-and-sessions.md)     | Identity, resource ownership, selection, persistence, close/reopen                                     |
| [Git workspaces](git-workspaces.md)                   | Concurrent branches, worktree creation, base commits, and rollback                                     |
| [Development runtimes](development-runtimes.md)       | Custom scripts, Turbo, ports, provisioning, readiness, and shutdown                                    |
| [Docker Compose](docker-compose.md)                   | Concurrent stacks, containers, networks, volumes, and cleanup                                          |
| [Configuration and setup](configuration-and-setup.md) | Installation, versioned recipes, environment, and defaults                                             |
| [Codebase and IDE](codebase-and-ide.md)               | Module dependencies, Electron transport, IDE profiles, and extensibility                               |
| [Validation and recovery](validation-and-recovery.md) | Test layers, failure cases, crash limitations, and proposed recovery                                   |
| [MVP verification](mvp-verification.md)               | Dated acceptance results, cleanup correction, evidence, and verification limits                        |
| [Engineering checks](quality-checks.md)               | Formatting, types, lint, unused code, module boundaries, coverage, build and integration gates         |

Unqualified behavior describes the current implementation and its expected
contract. Sections titled **Proposed next work** describe design directions that
are not implemented or committed APIs. Source links identify contract owners;
tests provide behavioral evidence. Update affected specs, configuration examples,
and regression coverage when changing a contract.
