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
