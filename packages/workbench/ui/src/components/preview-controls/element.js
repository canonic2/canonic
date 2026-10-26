import { captureInputs, replayInputs } from '@canonic2/lattice/elements';
import { styles } from './styles.js';
import { stylesFor } from '../shared/stylesheet.js';
const stylesheet = stylesFor(styles);

const UNSET = Symbol('unrendered');
const EMPTY = {};
const NO_ACTIONS = [];

const node = (tag, text) => {
  const element = document.createElement(tag);
  if (text) element.textContent = text;
  return element;
};

export class WbPreviewControls extends HTMLElement {
  #model = null;
  #pending;
  #connection;
  #fields = node('div');
  #rows = new Map();
  #reset = node('button', 'Reset scenario');
  #actions = node('ol');
  #empty = node('p', 'No actions yet.');
  #docs = node('details');
  #documentation = node('p');
  #renderedControls = UNSET;
  #renderedInputs = UNSET;
  #renderedActions = UNSET;
  #renderedDocs = UNSET;
  #actionRows = new Map();

  constructor() {
    super();
    this.#pending = captureInputs(this, ['model']);
    const root = this.attachShadow({ mode: 'open' });
    root.adoptedStyleSheets = [stylesheet(this.ownerDocument)];
    this.#reset.type = 'button';
    this.#docs.append(node('summary', 'Documentation'), this.#documentation);
    root.append(
      node('h2', 'Preview controls'),
      this.#fields,
      this.#reset,
      node('h3', 'Actions'),
      this.#empty,
      this.#actions,
      this.#docs,
    );
    if (this.#pending)
      queueMicrotask(() => {
        replayInputs(this, this.#pending);
        this.#pending = null;
      });
  }
  connectedCallback() {
    this.#connection?.abort();
    this.#connection = new AbortController();
    const { signal } = this.#connection;
    this.#reset.addEventListener(
      'click',
      () => {
        this.#invalidateInputs();
        this.#render(true);
        this.#emit('wb-preview-reset');
      },
      { signal },
    );
    this.#fields.addEventListener(
      'change',
      (event) => {
        const input = event.target;
        const name = input.dataset.name,
          spec = this.#model?.controls[name];
        if (!spec) return;
        let value;
        try {
          value =
            spec.type === 'boolean'
              ? input.checked
              : spec.type === 'number'
                ? Number(input.value)
                : spec.type === 'json'
                  ? JSON.parse(input.value)
                  : spec.type === 'select'
                    ? spec.options.find((option) => String(option) === input.value)
                    : input.value;
        } catch {
          input.setCustomValidity('Enter valid JSON');
          input.reportValidity();
          return;
        }
        input.setCustomValidity('');
        this.#emit('wb-preview-input', { name, value });
      },
      { signal },
    );
    this.#render();
  }
  disconnectedCallback() {
    this.#connection?.abort();
  }
  get model() {
    return this.#pending?.has('model') ? this.#pending.get('model') : this.#model;
  }
  set model(value) {
    const previous = this.model;
    const pending = this.#pending?.has('model');
    this.#pending?.delete('model');
    if (this.#model === value && !pending) return;
    const contextChanged =
      previous?.instance !== value?.instance ||
      previous?.id !== value?.id ||
      previous?.scenario !== value?.scenario;
    if (contextChanged) this.#invalidateInputs();
    this.#model = value;
    this.#render(contextChanged);
  }
  #emit(type, detail) {
    this.dispatchEvent(new CustomEvent(type, { detail, bubbles: true, composed: true }));
  }
  #invalidateInputs() {
    for (const row of this.#rows.values()) {
      row.source = UNSET;
      row.value = UNSET;
      row.input.setCustomValidity('');
    }
  }
  #render(forceInputs = false) {
    const model = this.model;
    const controls = model?.controls || EMPTY;
    const inputs = model?.inputs || EMPTY;
    const definitionsChanged = controls !== this.#renderedControls;
    if (definitionsChanged) {
      this.#renderDefinitions(controls);
      this.#renderedControls = controls;
    }
    if (forceInputs || definitionsChanged || inputs !== this.#renderedInputs) {
      this.#renderInputs(inputs);
      this.#renderedInputs = inputs;
    }
    const actions = model?.actions || NO_ACTIONS;
    if (actions !== this.#renderedActions) {
      this.#renderActionLog(actions);
      this.#renderedActions = actions;
    }
    const docs = model?.docs || '';
    if (docs !== this.#renderedDocs) {
      this.#docs.hidden = !docs;
      this.#documentation.textContent = docs;
      this.#renderedDocs = docs;
    }
  }
  #renderDefinitions(controls) {
    for (const [name, row] of this.#rows)
      if (!Object.hasOwn(controls, name)) {
        row.label.remove();
        this.#rows.delete(name);
      }
    let position = 0;
    for (const [name, spec] of Object.entries(controls)) {
      let row = this.#rows.get(name);
      const signature = JSON.stringify(spec);
      if (row?.signature !== signature) {
        row?.label.remove();
        const label = node('label');
        const input = node(
          spec.type === 'select' ? 'select' : spec.type === 'json' ? 'textarea' : 'input',
        );
        input.dataset.name = name;
        if (spec.type === 'select')
          for (const option of spec.options || []) {
            const item = node('option', String(option));
            item.value = String(option);
            input.append(item);
          }
        else if (spec.type !== 'json')
          input.type =
            spec.type === 'boolean' ? 'checkbox' : spec.type === 'number' ? 'number' : 'text';
        for (const key of ['min', 'max', 'step'])
          if (spec[key] !== undefined) input[key] = spec[key];
        label.append(node('span', spec.label || name), input);
        this.#fields.append(label);
        row = { signature, label, input, spec, source: UNSET, value: UNSET };
        this.#rows.set(name, row);
      }
      const at = this.#fields.children[position++];
      if (at !== row.label) this.#fields.insertBefore(row.label, at || null);
    }
  }
  #renderInputs(inputs) {
    for (const [name, row] of this.#rows) {
      const value = inputs[name];
      if (Object.is(row.source, value)) continue;
      row.source = value;
      const { spec } = row;
      if (spec.type === 'boolean') {
        if (row.value !== !!value) row.input.checked = !!value;
        row.value = !!value;
      } else {
        const text =
          spec.type === 'json'
            ? (JSON.stringify(value, null, 2) ?? '')
            : value === undefined
              ? ''
              : String(value);
        if (row.value !== text) {
          row.input.value = text;
          row.input.setCustomValidity('');
        }
        row.value = text;
      }
    }
  }
  #renderActionLog(actions) {
    const retained = new Map();
    const rows = [];
    for (const action of actions) {
      const text = action.name + (action.values.length ? ': ' + action.values.join(', ') : '');
      const occurrences = retained.get(text) || [];
      const item = this.#actionRows.get(text)?.[occurrences.length] || node('li', text);
      occurrences.push(item);
      retained.set(text, occurrences);
      rows.push(item);
    }
    for (const [text, items] of this.#actionRows)
      for (const item of items) if (!retained.get(text)?.includes(item)) item.remove();
    for (const [position, item] of rows.entries()) {
      const at = this.#actions.children[position];
      if (at !== item) this.#actions.insertBefore(item, at || null);
    }
    this.#actionRows = retained;
    if (this.#empty.hidden !== !!actions.length) this.#empty.hidden = !!actions.length;
  }
}
