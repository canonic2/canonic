/** @file Tutorial reflected text input, including pre-upgrade assignment precedence. */
import {
  captureInputs,
  replayInputs,
  skipUpgradeAttribute,
  defineOnce,
} from '@canonic2/lattice/elements';

/** A reflected text element whose pending input can be read before replay. */
export class TaskLabel extends HTMLElement {
  static observedAttributes = ['text'];
  #text = '';
  #node = document.createTextNode('');
  /** @type {Map<'text', unknown> | null} */
  #pending = null;

  /** Builds the shadow text and defers any input that would reflect attributes. */
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).append(this.#node);
    const pending = captureInputs(this, ['text'], { text: 'text' });
    if (pending !== null) {
      this.#pending = pending;
      queueMicrotask(() => {
        replayInputs(this, pending);
        this.#pending = null;
      });
    }
  }

  /** @returns {string} The current text, including captured input awaiting replay. */
  get text() {
    return this.#pending?.has('text') ? String(this.#pending.get('text')) : this.#text;
  }

  /** @param {string} value The text to render and reflect, superseding pending input. */
  set text(value) {
    this.#pending?.delete('text');
    if (value !== this.#text) {
      this.#text = value;
      this.#node.data = value;
    }
    if (this.getAttribute('text') !== value) this.setAttribute('text', value);
  }

  /**
   * Applies attributes, skipping the upgrade callback superseded by early input.
   * @param {string} name The changed attribute.
   * @param {string | null} oldValue The preceding attribute value.
   * @param {string | null} value The next value, or null for removal.
   * @returns {void}
   */
  attributeChangedCallback(name, oldValue, value) {
    if (!skipUpgradeAttribute(this.#pending, name)) this.text = value ?? '';
  }
}

/**
 * Registers the tutorial label without import-time registration.
 * @param {CustomElementRegistry} [registry] The destination registry.
 * @returns {void}
 */
export function defineTaskLabel(registry = customElements) {
  defineOnce(registry, 'acme-task-label', TaskLabel);
}
