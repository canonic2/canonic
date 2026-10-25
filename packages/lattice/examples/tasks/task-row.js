/** @file A native task row: plain inputs, application intent events and live controls. */
import { defineOnce } from '@canonic2/lattice/elements';
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
  #text = '';
  #done = false;
  #selected = false;
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
    this.addEventListener('keydown', (event) => {
      if (event.composedPath()[0] === this && (event.key === 'Enter' || event.key === ' ')) {
        event.preventDefault(); this.#intent('acme-task-select');
      }
    });
    this.#check.addEventListener('change', () => this.#intent('acme-task-toggle'));
    this.#editor.addEventListener('change', () => this.#intent('acme-task-rename', this.#editor.value));
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
  get selected() { return this.#selected; }
  /** @param {boolean} value Selection input, independent of product selection policy. */
  set selected(value) {
    if (this.#selected === value) return;
    this.#selected = value; this.toggleAttribute('data-selected', value); this.#select.setAttribute('aria-pressed', String(value));
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
