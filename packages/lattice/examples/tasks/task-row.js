/** @file A native task row: plain inputs, application intent events and live controls. */
import { captureInputs, defineOnce, replayInputs, skipUpgradeAttribute } from '@canonic2/lattice/elements';
/** @type {CSSStyleSheet | null} */
let sheet = null;
/** Share static presentation across retained rows. @returns {CSSStyleSheet} */
function styles() {
  if (sheet === null) {
    sheet = new CSSStyleSheet();
    sheet.replaceSync(':host{display:flex;align-items:center;gap:8px;box-sizing:border-box;padding:2px 8px}:host([data-selected]){background:#dceaff}:host(:focus){outline:2px solid #246}input[type=text]{flex:1;min-width:0}button{font:inherit}');
  }
  return sheet;
}

/** A fixed-height row; draft text is committed only on change. */
export class TaskRow extends HTMLElement {
  static observedAttributes = ['data-selected'];
  #text = '';
  #done = false;
  #selected = false;
  /** @type {Map<'selected', unknown> | null} Host-writing input awaiting safe replay. */
  #pending = null;
  /** @type {AbortController | null} Host keyboard lifetime. */
  #connection = null;
  #editor = document.createElement('input');
  #check = document.createElement('input');
  #select = document.createElement('button');
  /** Build the row once. Its own listeners live exactly as long as its nodes. */
  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' });
    root.adoptedStyleSheets = [styles()];
    this.#select.textContent = 'Select'; this.#select.type = 'button'; this.#select.setAttribute('aria-pressed', 'false');
    this.#editor.type = 'text'; this.#editor.setAttribute('aria-label', 'Task text');
    this.#check.type = 'checkbox'; this.#check.setAttribute('aria-label', 'Completed');
    root.append(this.#select, this.#check, this.#editor);
    this.#select.addEventListener('click', () => this.#intent('acme-task-select'));
    this.#check.addEventListener('change', () => this.#intent('acme-task-toggle'));
    this.#editor.addEventListener('change', () => this.#intent('acme-task-rename', this.#editor.value));
    const early = captureInputs(this, ['text', 'done']);
    if (early !== null) replayInputs(this, early);
    const pending = captureInputs(this, ['selected'], { selected: 'data-selected' });
    if (pending !== null) {
      this.#pending = pending;
      queueMicrotask(() => { replayInputs(this, pending); this.#pending = null; });
    }
  }
  /** Acquire host keyboard handling only for this connection. @returns {void} */
  connectedCallback() {
    if (!this.isConnected) return;
    this.#connection?.abort(); const own = new AbortController(); this.#connection = own;
    try {
      this.addEventListener('keydown', (event) => {
        if (event.composedPath()[0] === this && (event.key === 'Enter' || event.key === ' ')) {
          event.preventDefault(); this.#intent('acme-task-select');
        }
      }, { signal: own.signal });
    } catch (error) { own.abort(); this.#connection = null; throw error; }
  }
  /** Release host resources while retaining the row and its draft. @returns {void} */
  disconnectedCallback() { this.#connection?.abort(); this.#connection = null; }
  /** Preserve focus and the connection on atomic placement moves. @returns {void} */
  connectedMoveCallback() {}
  /** Apply selection decoration attributes after upgrade precedence. @param {string} name Attribute. @param {string | null} _oldValue Prior value. @param {string | null} value New value. @returns {void} */
  attributeChangedCallback(name, _oldValue, value) {
    if (skipUpgradeAttribute(this.#pending, name)) return;
    if ((value !== null) === this.#selected && !this.#pending?.has('selected')) return;
    this.selected = value !== null;
  }
  /** Current selected text input value from state. @returns {string} The assigned text. */
  get text() { return this.#text; }
  /** @param {string} value Text to show; compare against the live draft for explicit resets. */
  set text(value) { this.#text = value; if (this.#editor.value !== value) this.#editor.value = value; }
  /** @returns {boolean} Completion input. */
  get done() { return this.#done; }
  /** @param {boolean} value Completion input. */
  set done(value) { this.#done = value; if (this.#check.checked !== value) this.#check.checked = value; }
  /** @returns {boolean} Selection decoration. */
  get selected() { return this.#pending?.has('selected') ? /** @type {boolean} */ (this.#pending.get('selected')) : this.#selected; }
  /** @param {boolean} value Selection input, independent of product selection policy. */
  set selected(value) {
    this.#pending?.delete('selected');
    if (this.#selected !== value) { this.#selected = value; this.#select.setAttribute('aria-pressed', String(value)); }
    if (this.hasAttribute('data-selected') !== value) this.toggleAttribute('data-selected', value);
  }
  /**
   * Emit synchronous intent without owning actions or stores.
   * @param {string} name Documented intent event.
   * @param {string} [text] Edited text.
   * @returns {void}
   */
  #intent(name, text) {
    if (this.isConnected) this.dispatchEvent(new CustomEvent(name, { bubbles: true, composed: true, detail: { key: this.dataset.key, text } }));
  }
}

/** Register the example's visual component. @returns {void} */
export function defineTaskRow() { defineOnce(customElements, 'acme-task-row', TaskRow); }
