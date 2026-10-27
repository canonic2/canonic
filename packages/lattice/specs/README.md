# Lattice specifications

Lattice is a framework for multiple languages and native platforms. Its shared
direction and platform implementations have separate specification owners.
Read only the area needed for the task.

| Area                               | Owns                                                                                          | Status                                                            |
| ---------------------------------- | --------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| [Framework](framework.md)          | Shared purpose, native primitives, behavior definitions, generation and platform boundaries   | Agreed direction; cross-platform implementation remains to design |
| [JavaScript](javascript/README.md) | JavaScript authoring conventions, contracts, implementation architecture and workload roadmap | Existing implementation and ongoing work                          |

JavaScript is the only implemented language. Swift, Kotlin, Windows and Linux
support are agreed product scope, not available libraries. Language choices for
Windows and Linux, platform packaging and implementation order remain open.

The shared framework specification does not prescribe JavaScript APIs to other
platforms. Detailed requirements in `javascript/` apply to that implementation
unless a shared contract explicitly adopts their semantics.

Internal specifications are separate from the reader-facing
[documentation](../docs/README.md), which currently covers JavaScript.
[Research](../research/README.md) records measured workloads and their limits;
JavaScript evidence does not establish native-platform performance. The
[package README](../README.md) owns current setup and development commands.
