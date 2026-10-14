# Registry specifications

These specifications define `canonic-registry`: what the library must do, the
guarantees it gives, the decisions behind them, and how they are verified.
They describe the product, not how to use it. They are the reference that
the code is held to: where the two disagree, one of them is wrong and is
corrected.

Start with the [overview](overview.md). It states the problem, the workflow
the library supports from start to finish, the vocabulary, and the boundary
between the library and the products that use it.

| Topic | Owns |
| --- | --- |
| [Overview](overview.md) | Purpose, workflow, vocabulary, responsibilities and status |
| [Registry format](registry-format.md) | What a registry serves and the rules its publisher must follow |
| [Trust](trust.md) | Source identity, first-use discovery, the built-in registry, catalog authentication, checkpoints and threats to trust |
| [Security](security.md) | The complete security model: attackers, defenses, non-goals, dependency behavior relied on, and remaining gaps |
| [Acquisition](acquisition.md) | Reading from local and HTTPS sources |
| [Registrations](registrations.md) | Aliases, scopes, defaults and the global registrations file |
| [References](references.md) | Operator references, catalog lookups, installed selectors and installed matching |
| [Dependency resolution](resolution.md) | Choosing one release per reachable pack |
| [Installed state](installed-state.md) | The artifact cache and lockfile entries |
| [Limits](limits.md) | Every size, count and time bound |
| [Architecture](architecture.md) | Modules, dependency direction and engineering conventions |
| [Verification](verification.md) | Checks, test coverage and acceptance criteria |

Each requirement has one owning topic; other topics link to it. Numeric
bounds are owned by [Limits](limits.md).
