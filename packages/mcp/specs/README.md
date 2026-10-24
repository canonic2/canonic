# MCP library specifications

These specifications define `canonic-mcp`: what the library must do, the
guarantees it gives, the decisions behind them and how they are verified. They
describe the library, not how to use it. Where the code and a specification
disagree, one of them is wrong and is corrected.

Start with the [overview](overview.md).

| Topic | Owns |
| --- | --- |
| [Overview](overview.md) | Purpose, layers, vocabulary, responsibilities and status |
| [Protocol](protocol.md) | Framing, message classification, lifecycle, version negotiation, outstanding IDs, cancellation and error codes |
| [Tools](tools.md) | Catalog rules, argument validation, calls, results and content |
| [Scheduling](scheduling.md) | The stdio driver: lanes, keys, admission, deadlines, cancellation, progress and end of input |
| [Confirmation](confirmation.md) | Host confirmation through form elicitation and its continuation |
| [Limits](limits.md) | Every size, count and time bound |
| [Architecture](architecture.md) | Modules, dependency direction, threads and engineering conventions |
| [Verification](verification.md) | Checks, test coverage and acceptance criteria |

Each requirement has one owning topic; other topics link to it. Numeric
bounds are owned by [Limits](limits.md).
