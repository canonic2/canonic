# Components

A reusable component should work wherever you put it: in an application, a test
or a documentation page. Give it plain property inputs and let it emit events
for user intent.

Lattice components are native Web Components. You write a class that extends
`HTMLElement`; Lattice supplies helpers for registration and input upgrade.

## Build a task toggle

The runnable [tutorial](../../examples/tutorial/README.md) uses this
[task-toggle.js](../../examples/tutorial/task-toggle.js). Its browser tests import
this implementation directly. To follow along, save it beside your example page:

```js
/**
 * @file Tutorial checkbox element: plain done input and observed user intent.
 */
import { captureInputs, replayInputs, defineOnce } from '@canonic2/lattice/elements';

/** A labeled native checkbox with a presentation input. */
export class TaskToggle extends HTMLElement {
  #checkbox = document.createElement('input');
  #done = false;

  /** Builds the retained shadow controls and replays constructor-safe inputs. */
  constructor() {
    super();
    this.#checkbox.type = 'checkbox';
    const label = document.createElement('label');
    label.append(this.#checkbox, document.createTextNode('Complete task'));
    this.attachShadow({ mode: 'open' }).append(label);
    this.#checkbox.addEventListener('change', () => {
      if (!this.isConnected) return;
      this.dispatchEvent(
        new CustomEvent('acme-task-toggle', {
          detail: { done: this.#checkbox.checked },
          bubbles: true,
          composed: true,
        }),
      );
    });
    const early = captureInputs(this, ['done']);
    if (early !== null) replayInputs(this, early);
  }

  /** @returns {boolean} The last assigned application value. */
  get done() {
    return this.#done;
  }

  /** @param {boolean} value The desired checked state, compared with the live control. */
  set done(value) {
    this.#done = value;
    if (this.#checkbox.checked !== value) this.#checkbox.checked = value;
  }
}

/**
 * Registers the tutorial toggle explicitly; repeated identical registration is safe.
 * @param {CustomElementRegistry} [registry] The destination registry.
 * @returns {void}
 */
export function defineTaskToggle(registry = customElements) {
  defineOnce(registry, 'acme-task-toggle', TaskToggle);
}
```

The constructor builds the shadow tree once. The setter changes only the
checkbox, and the event listener belongs to that retained checkbox.

The checkbox has a native label and keyboard behavior. A custom element does
not need to replace those with a hand-built interaction.

## Register and use it

For the `examples/hello-lattice/` page from the quick start, extend its import map:

```html
<script type="importmap">
  {
    "imports": {
      "@canonic2/lattice": "../../src/index.js",
      "@canonic2/lattice/elements": "../../src/modules/elements/elements.js"
    }
  }
</script>
```

Your own project's map uses the URLs its server serves, as described in the
[quick start](quick-start.md#use-lattice-in-your-own-project).

Registration is an explicit call:

```js
import { TaskToggle, defineTaskToggle } from './task-toggle.js';

defineTaskToggle();

const toggle = new TaskToggle();
toggle.done = true;
document.body.append(toggle);
```

Importing the module defines no tag. `defineOnce` permits repeated registration
of the same constructor, but reports a conflicting constructor under the same
name.

You can also use `<acme-task-toggle></acme-task-toggle>` in markup once its
definition is loaded.

## Inputs and intent

Assigning `toggle.done` changes presentation. It does not dispatch the toggle
event. Events describe what a person did, so assigning application state
cannot accidentally call an action again.

The component knows neither where its value came from nor how it will be saved.
The next chapter connects it to a task's state.

The setter compares against the control's live `checked` property. That matters
because a person can change the checkbox independently of the last assigned
input. Assigning the same application value can still be an intentional reset.

## Inputs assigned before upgrade

A caller may assign `element.done` before the browser knows its custom-element
class. That creates an own property which would hide the class setter.

`captureInputs` moves declared early values off the instance. `replayInputs`
applies them through the setters. This component can replay in its constructor
because its setter writes only its own fields and shadow control.

Inputs that reflect host attributes or place caller-supplied children need
deferred replay. The [element-helper reference](../api/elements.md#reflected-inputs)
shows that pattern and its precedence rules.

## Local interaction state

An element can own values used only for its interaction: an open disclosure,
a draft before it is submitted, or a drag in progress. Those values need no
application store until another part of the application needs to observe or
decide them.

Avoid keeping a second independently writable copy of application state.
A component's `done` input is presentation; the task capability remains the
owner of whether the task is complete.

**Next:** [Bindings and actions](bindings.md)
