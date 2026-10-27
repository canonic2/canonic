# Lattice JavaScript authoring model

This folder describes the JavaScript authoring model: what application code
looks like and how its parts are composed. Guarantees stay in the
[contracts](../contracts/README.md); these pages show how authors meet them.

The [shared framework direction](../../framework.md) owns the multi-language
scope. These Web Component and binding patterns do not prescribe native APIs
for other platforms.

| Page                                                     | Covers                                                                                   | Status                                                  |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| [Choosing an approach](choosing-an-approach.md)          | State and list presentation choices, tradeoffs and evidence limits                       | Agreed guidance; windowing readiness open               |
| [Elements](elements.md)                                  | How an element is written: a native class with narrow helpers                            | Agreed shape; helpers implemented                       |
| [Capabilities](capabilities.md)                          | State ownership, selections, named actions and failures                                  | Agreed shape, from the filters example                  |
| [Bindings](bindings.md)                                  | Inputs, events, reporting failures and setup rollback                                    | Agreed shape, from the filters example                  |
| [Composition](composition.md)                            | Creating the parts, lifetimes and teardown                                               | Agreed shape, from the filters example                  |
| [Collection presentation](collections.md)                | Guarding completed reconciliation while preserving row incarnations, focus and anchoring | Accepted application pattern; large-list readiness open |
| [Retained collection mechanics](collection-mechanics.md) | Candidate callback-based ownership and range reconciliation                              | Proposal; no exported helper or adoption                |

The element, capability, binding and composition patterns have runnable filters
and tasks examples in `examples/`, with Node and browser coverage. Snippets
omit some types and surrounding code. The element helper fixtures also live in
`src/modules/elements/elements.browser.test.js`. Collection presentation pages
separate measured baselines from the current example and state the limits
of their evidence.
