/**
 * @file `<acme-filter-summary>`: one line of text describing the current
 * filter. Its `text` input has no attribute, so an early value is applied in the
 * constructor.
 */
import { captureInputs, defineOnce, replayInputs } from 'canonic-lattice/elements';

/** One line of summary text. */
export class AcmeFilterSummary extends HTMLElement {
  #text = document.createTextNode('');
  #value = '';

  /** Builds the view once and applies early text. */
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).append(this.#text);
    const early = captureInputs(this, ['text']);
    if (early !== null) replayInputs(this, early);
  }

  /** @returns {string} The text shown. */
  get text() {
    return this.#value;
  }

  /** @param {string} value The text to show. */
  set text(value) {
    if (Object.is(value, this.#value)) return;
    this.#value = value;
    this.#text.data = value;
  }
}

/**
 * Defines `<acme-filter-summary>` in a registry.
 * @param {CustomElementRegistry} [registry] The registry to define it in.
 * @returns {void}
 */
export function defineAcmeFilterSummary(registry = customElements) {
  defineOnce(registry, 'acme-filter-summary', AcmeFilterSummary);
}
