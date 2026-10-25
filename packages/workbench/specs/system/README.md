# Shared system contracts

This folder owns architecture and engineering rules applied across capabilities.
Feature workflows and their acceptance remain in the owning capability; dated
measurements belong to the package's [research](../../research/README.md).

| Topic | Owns |
| --- | --- |
| [Modules](modules.md) | Product/source owners, dependency direction, UI/backend independence and the current browser source layout |
| [Browser UI port](ui-port.md) | Target browser layout, capability and view map, port order, path consumers, temporary adapters and the Lattice delivery slice |
| [Rendering flow](rendering-flow.md) | One shell, the content renderer contract, renderer-specific implementation ownership, responsibility boundaries and isolation acceptance; navigation behavior is in [canvas navigation](../canvas/navigation.md) |
| [Server](server.md) | Rust server/host process model (evaluators, Storybook, their lifetimes, deadlines and runtime selection), protocol, request tracing, routes, resident index coordination and trust boundary |
| [Access boundaries](access-boundaries.md) | Local execution/origin model, request authority, file/write boundaries, body bounds and unresolved access policy |
| [Implementation proxy](implementation-proxy.md) | Loopback proxy routing, HTML/header/cookie rewriting and implementation framing |
| [Performance](performance.md) | Agreed/proposed budgets, timed boundaries, diagnostics and measurement use of request tracing, resource expectations, unmet targets and measurement acceptance |
| [Verification map](verification.md) | Automated coverage and recorded host acceptance for each capability, kept apart, with links to each topic's gaps |
| [Code quality](code-quality.md) | Engineering gates, dependency/artifact policy and behavioral evidence conventions |
| [Lab methodology](labs.md) | Experiment planning in ignored local labs, fair comparisons, shared fixtures, runner reliability, research records and decision/confirmation lifecycle |

The server composes services; it does not own page behavior or Workbench UI.
The [preview pipeline](../preview-pipeline/README.md) owns compilation and runtime
mechanics, while [adapters](../adapters/README.md) own technology differences.
Architecture establishes dependencies; capability specs own their state,
failure/recovery rules and numeric bounds. Engineering gates and measurements
establish different kinds of evidence and cannot substitute for one another.
