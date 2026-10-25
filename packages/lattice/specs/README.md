# Lattice specifications

The specifications are separated by purpose. Read only the area needed for the task.

| Area | Question | Contents |
| --- | --- | --- |
| [Framework](framework/README.md) | What does application code look like? | The element authoring shape; capabilities, bindings and composition reserved |
| [Architecture](architecture/README.md) | How is Lattice organized internally? | Store and package implementation design, as drafts for review |
| [Contracts](contracts/README.md) | What must an implementation guarantee? | Detailed behavior, edge cases, performance, delivery and acceptance evidence |

Contracts are agreed. The store core, the element helpers and the selection
declaration check are implemented; bindings are written by hand on the store,
and the other capabilities are not implemented. The framework folder
defines the element authoring shape, decided by measurement in the package's
[research](../research/README.md); the architecture pages describe the
implemented code.

Each detailed requirement has one owning contract. Contributor instructions
live in the package's agent rules; the
[package README](../README.md) owns setup and commands.
