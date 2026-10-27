# Quick start

In this chapter you will build a task control that updates a status message
when you complete the task. You need **Chromium 133+**, **Node 24.12+** to run
the example host, and this repository's Lattice checkout. Other browser engines
are unsupported. Embedded browser hosts must also use Chromium 133+; Lattice
ships no polyfills for missing APIs.

Lattice is unpublished. The example below loads the local source directly;
there is no package-registry installation step.

## Run the existing examples

From the repository root, start the example host:

```sh
node packages/lattice/examples/serve.js
```

Open [the filters example](http://127.0.0.1:4177/examples/filters/). Type in one
field: the other fields and summary update with it. The Lock button disables
the fields. Tear down removes the view and ends its bindings.

The same host serves Lattice's source files, the task-list example and
[the guide tutorial](http://127.0.0.1:4177/examples/tutorial/). The tutorial runs
the component and keyed-row implementations introduced in later chapters;
their sources and browser coverage live in
[examples/tutorial](../../examples/tutorial/README.md).

## Build your first page

Create `packages/lattice/examples/hello-lattice/index.html` and paste this page:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>My first Lattice task</title>
    <script type="importmap">
      {
        "imports": {
          "@canonic2/lattice": "../../src/index.js"
        }
      }
    </script>
  </head>
  <body>
    <h1>Write the guide</h1>
    <label>
      <input id="done" type="checkbox" />
      Complete task
    </label>
    <p id="status" role="status"></p>

    <script type="module">
      import { createStore } from '@canonic2/lattice';

      const task = createStore({ done: false });
      const lifetime = new AbortController();
      const checkbox = document.getElementById('done');
      const output = document.getElementById('status');

      const done = { fields: ['done'], read: (state) => state.done };
      const status = {
        fields: ['done'],
        read: (state) => (state.done ? 'Complete' : 'In progress'),
      };

      task.watch(
        done,
        (value) => {
          checkbox.checked = value;
        },
        { signal: lifetime.signal },
      );

      task.watch(
        status,
        (value) => {
          output.textContent = value;
        },
        { signal: lifetime.signal },
      );

      checkbox.addEventListener(
        'change',
        () => {
          task.update({ done: checkbox.checked });
        },
        { signal: lifetime.signal },
      );

      window.addEventListener('pagehide', () => lifetime.abort(), { once: true });
    </script>
  </body>
</html>
```

With the example host still running, open
[hello-lattice](http://127.0.0.1:4177/examples/hello-lattice/).
You should see “In progress”. Check the box and the message becomes “Complete”.

The checkbox and status message each watch the value they need. The initial
callbacks run while the page is set up. Later, the change event updates state,
which delivers the new values synchronously.

The controller gives the listeners and watchers one lifetime. When the page
leaves, aborting it ends those subscriptions.

## Use Lattice in your own project

A local consumer can use a package-manager path dependency, such as
`"@canonic2/lattice": "link:../lattice"`, with a path relative to its own
`package.json`.

For native browser modules, serve the source and provide an import map. A
package-manager link alone does not tell a browser how to resolve a package name:

```html
<script type="importmap">
  {
    "imports": {
      "@canonic2/lattice": "/vendor/lattice/src/index.js",
      "@canonic2/lattice/collection": "/vendor/lattice/src/modules/collection/collection.js",
      "@canonic2/lattice/elements": "/vendor/lattice/src/modules/elements/elements.js"
    }
  }
</script>
```

These URLs are examples: your server must actually serve those files. Put the
map before modules that use it. Each document, including an iframe, needs its
own import map. A bundler can instead resolve the package's exports.

You can now continue with the store behind the page.

**Next:** [State](state.md)
