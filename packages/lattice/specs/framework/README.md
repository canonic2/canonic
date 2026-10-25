# Lattice framework

This folder describes the framework's authoring model: what application code
looks like and how its parts are composed. Guarantees stay in the
[contracts](../contracts/README.md); these pages show how authors meet them.

| Page | Covers | Status |
| --- | --- | --- |
| [Choosing an approach](choosing-an-approach.md) | State and list presentation choices, tradeoffs and evidence limits | Agreed guidance; windowing readiness open |
| [Elements](elements.md) | How an element is written: a native class with narrow helpers | Agreed shape; helpers implemented |
| [Capabilities](capabilities.md) | State ownership, selections, named actions and failures | Agreed shape, from the filters example |
| [Bindings](bindings.md) | Inputs, events, reporting failures and setup rollback | Agreed shape, from the filters example |
| [Composition](composition.md) | Creating the parts, lifetimes and teardown | Agreed shape, from the filters example |
| [Collection presentation](collections.md) | Guarding completed reconciliation while preserving row incarnations, focus and anchoring | Accepted application pattern; large-list readiness open |

The element, capability, binding and composition snippets come from the
runnable filters example in `examples/filters/`, whose Node and browser tests
exercise it. Collection presentation patterns state their measured scope and
link to the research that supports them.
