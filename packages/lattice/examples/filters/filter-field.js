/**
 * @file `<acme-filter-field>`: a labeled search field. Inputs `label` and
 * `disabled` reflect to attributes; `value` is compared with the control's live
 * value. It dispatches `acme-filter-input` for typing, and observes its own size
 * while connected.
 */
import { captureInputs, defineOnce, replayInputs, skipUpgradeAttribute } from 'canonic-lattice/elements';

/** @type {CSSStyleSheet | null} */
let sheet = null;

/**
 * Returns the stylesheet every instance shares, created on first use.
 * @returns {CSSStyleSheet} The sheet.
 */
function styles() {
  if (sheet === null) {
    sheet = new CSSStyleSheet();
    sheet.replaceSync(
      ':host{display:block}label{display:flex;gap:8px;align-items:center}label.compact{flex-direction:column}input{flex:1;min-width:0}',
    );
  }
  return sheet;
}

/** A labeled search field. */
export class AcmeFilterField extends HTMLElement {
  static observedAttributes = ['label', 'disabled'];

  #label = '';
  #value = '';
  #disabled = false;
  /** @type {Map<string, unknown> | null} */
  #pending = null;
  /** @type {AbortController | null} */
  #connection = null;
  #text = document.createTextNode('');
  #control = document.createElement('input');
  #row = document.createElement('label');

  /** Builds the view once, applies an early value and captures early reflected inputs. */
  constructor() {
    super();
    this.#control.type = 'search';
    this.#row.append(this.#text, this.#control);
    const root = this.attachShadow({ mode: 'open' });
    root.adoptedStyleSheets = [styles()];
    root.append(this.#row);
    this.#control.addEventListener('input', this);
    const early = captureInputs(this, ['value']);
    if (early !== null) replayInputs(this, early);
    const pending = captureInputs(this, ['label', 'disabled'], { label: 'label', disabled: 'disabled' });
    if (pending !== null) {
      this.#pending = pending;
      queueMicrotask(() => {
        replayInputs(this, pending);
        this.#pending = null;
      });
    }
  }

  /** @returns {string} The label, including one not yet replayed. */
  get label() {
    return this.#pending?.has('label') ? String(this.#pending.get('label')) : this.#label;
  }

  /** @param {string} value The label text, reflected to the `label` attribute. */
  set label(value) {
    this.#pending?.delete('label');
    if (!Object.is(value, this.#label)) {
      this.#label = value;
      this.#text.data = value;
    }
    if (this.getAttribute('label') !== value) this.setAttribute('label', value);
  }

  /** @returns {string} The last value assigned. */
  get value() {
    return this.#value;
  }

  /** @param {string} value The text to show, written only when the control shows something else. */
  set value(value) {
    this.#value = value;
    if (this.#control.value !== value) this.#control.value = value;
  }

  /** @returns {boolean} Whether the field is disabled, including a value not yet replayed. */
  get disabled() {
    return this.#pending?.has('disabled') ? Boolean(this.#pending.get('disabled')) : this.#disabled;
  }

  /** @param {boolean} value Whether the field is disabled, reflected to the `disabled` attribute. */
  set disabled(value) {
    this.#pending?.delete('disabled');
    if (!Object.is(value, this.#disabled)) {
      this.#disabled = value;
      this.#control.disabled = value;
    }
    if (this.hasAttribute('disabled') !== value) this.toggleAttribute('disabled', value);
  }

  /**
   * Applies `label` and `disabled` attributes, except the upgrade's own callback
   * for an input captured as a property.
   * @param {string} name The attribute name.
   * @param {string | null} oldValue The previous value.
   * @param {string | null} newValue The new value, `null` when removed.
   */
  attributeChangedCallback(name, oldValue, newValue) {
    if (skipUpgradeAttribute(this.#pending, name)) return;
    if (name === 'label') this.label = newValue ?? '';
    else this.disabled = newValue !== null;
  }

  /** Observes the field's size for this connection. */
  connectedCallback() {
    if (!this.isConnected) return;
    this.#connection?.abort();
    const connection = new AbortController();
    this.#connection = connection;
    const observer = new ResizeObserver((entries) => {
      const width = entries[entries.length - 1]?.contentRect.width ?? 0;
      this.#row.classList.toggle('compact', width < 240);
    });
    observer.observe(this);
    connection.signal.addEventListener('abort', () => observer.disconnect(), { once: true });
  }

  /** Releases what the connection acquired. */
  disconnectedCallback() {
    this.#connection?.abort();
    this.#connection = null;
  }

  /** Keeps the connection when moved with `moveBefore`. */
  connectedMoveCallback() {}

  /** Turns typing into an `acme-filter-input` event while connected. */
  handleEvent() {
    if (!this.isConnected) return;
    this.dispatchEvent(
      new CustomEvent('acme-filter-input', {
        bubbles: true,
        composed: true,
        detail: { value: this.#control.value },
      }),
    );
  }
}

/**
 * Defines `<acme-filter-field>` in a registry.
 * @param {CustomElementRegistry} [registry] The registry to define it in.
 * @returns {void}
 */
export function defineAcmeFilterField(registry = customElements) {
  defineOnce(registry, 'acme-filter-field', AcmeFilterField);
}
