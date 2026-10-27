# Guide examples

Run `node packages/lattice/examples/serve.js` from the repository root and open
[the tutorial](http://127.0.0.1:4177/examples/tutorial/) in Chromium 133+.
The server needs Node 24.12+. Other browser engines are unsupported.

The toggle and first row share task completion. `TaskLabel` reflects the status
text to its attribute. These are the source implementations shown in the
[components](../../docs/guide/components.md),
[list rendering](../../docs/guide/list-rendering.md) and
[element-helper](../../docs/api/elements.md#reflected-inputs) chapters.

`tasks.js` owns collection state and actions; `task-list.js` owns row bindings
and keyed retention; each row uses native labeled controls. The two custom
elements own their shadow markup. `app.js` composes them, and its caller owns
the view signal. Aborting releases subscriptions and listeners and removes
the nodes. A capability can outlive its mounted view.

`components.browser.test.js` exercises inputs, events and upgrade precedence.
`list.browser.test.js` exercises the actual row mount and application composition,
including replacement, retained focus, abort and setup failure. These tests
belong to the existing browser project.
This small, fully mounted tutorial has no windowing, selection policy or
performance claim; the larger [tasks example](../tasks/README.md) covers those
presentation concerns.
