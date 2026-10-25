# Element-local state

Keep a fact inside an element when it belongs to that element's interaction:
an open disclosure, a hovered row or a drag in progress. It needs no store or
subscription if no application capability needs to decide or observe it.

## Choose it for an owned interaction

A disclosure can retain its nodes and change only its own presentation:

```js
/** @file An application-owned disclosure element. */
import { defineOnce } from '@canonic2/lattice/elements';

/** A disclosure whose open state belongs to this instance. */
class AcmeDisclosure extends HTMLElement {
  #button = document.createElement('button');
  #panel = document.createElement('div');
  #open = false;

  /** Builds the shadow tree and its instance-owned interaction once. */
  constructor() {
    super();
    this.#button.type = 'button';
    this.#button.textContent = 'Details';
    this.#button.setAttribute('aria-expanded', 'false');
    this.#button.setAttribute('aria-controls', 'details');
    this.#panel.id = 'details';
    this.#panel.hidden = true;
    this.#panel.append(document.createElement('slot'));
    this.attachShadow({ mode: 'open' }).append(this.#button, this.#panel);
    this.#button.addEventListener('click', () => this.#toggle());
  }

  /**
   * Toggles the owned panel and its accessible expanded state.
   * @returns {void}
   */
  #toggle() {
    this.#open = !this.#open;
    this.#panel.hidden = !this.#open;
    this.#button.setAttribute('aria-expanded', String(this.#open));
  }
}

/**
 * Registers the disclosure when the application explicitly requests it.
 * @param {CustomElementRegistry} [registry] The document's registry.
 * @returns {void}
 */
export function defineAcmeDisclosure(registry = customElements) {
  defineOnce(registry, 'acme-disclosure', AcmeDisclosure);
}
```

The listener belongs to a retained shadow button and is created once, so
reconnection does not install another listener. An element that listens on
`window` or `document`, or owns an observer or timer, must separately acquire
and release those connection resources. Local state does not make external
resources exempt from cleanup.

## Move application decisions to a capability

Use a [store](stores.md) when other parts of the application need the value,
when actions must validate it, or when it belongs in persistence. The element
then receives a public input and emits intent; it does not import the store.
An input setter should update presentation without emitting an intent event.

Do not store the same application fact independently in both places. For an
editable control, compare an incoming value with the control's live value
before writing it, so updates preserve typing and selection when possible.
Public inputs must also handle assignments made before custom-element upgrade;
see the input implementation in the
[filter-field example](../examples/filters/filter-field.js).

Local state avoids store and subscription setup, but still owns DOM work and
lifecycle obligations. Do not move a shared application decision into local
state just to save a subscription.
