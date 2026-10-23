# Shared system contracts

This folder owns architecture and engineering rules applied across capabilities.
Feature workflows and their acceptance remain in the owning capability; dated
measurements belong to [research](../research/README.md).

| Topic | Owns |
| --- | --- |
| [Modules](modules.md) | Product/source owners, dependency direction, UI/backend independence and bounded migration |
| [Rendering flow](rendering-flow.md) | One shell, content-only renderers, responsibility boundaries and isolation acceptance |
| [Server](server.md) | Rust server/host process model, protocol, routes, resident index coordination and trust boundary |
| [Access boundaries](access-boundaries.md) | Local execution/origin model, request authority, file/write boundaries, body bounds and unresolved access policy |
| [Implementation proxy](implementation-proxy.md) | Loopback proxy routing, HTML/header/cookie rewriting and implementation framing |
| [Performance](performance.md) | Agreed/proposed budgets, timed boundaries, diagnostics, resource expectations and measurement acceptance |
| [Code quality](code-quality.md) | Engineering gates, dependency/artifact policy and behavioral evidence conventions |
| [Lab methodology](labs.md) | Experiment planning, fair comparisons, fixture ownership, runner reliability and decision/confirmation lifecycle |

The server composes services; it does not own page behavior or Workbench UI.
The [preview pipeline](../preview-pipeline/README.md) owns compilation and runtime
mechanics, while [adapters](../adapters/README.md) own technology differences.
Architecture establishes dependencies; capability specs own their state,
failure/recovery rules and numeric bounds. Engineering gates and measurements
establish different kinds of evidence and cannot substitute for one another.
