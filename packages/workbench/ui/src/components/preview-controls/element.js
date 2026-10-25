import { captureInputs, replayInputs } from '@canonic2/lattice/elements';
import { styles } from './styles.js';

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

  constructor() {
    super();
    this.#pending = captureInputs(this, ['model']);
    const root = this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(styles);
    root.adoptedStyleSheets = [sheet];
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
    queueMicrotask(() => {
      if (this.#pending) replayInputs(this, this.#pending);
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
        for (const row of this.#rows.values()) {
          row.value = undefined;
          row.input.setCustomValidity('');
        }
        this.#render();
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
    return this.#model;
  }
  set model(value) {
    this.#pending?.delete('model');
    if (this.#model === value) return;
    if (
      this.#model?.instance !== value?.instance ||
      this.#model?.id !== value?.id ||
      this.#model?.scenario !== value?.scenario
    )
      for (const row of this.#rows.values()) {
        row.value = undefined;
        row.input.setCustomValidity('');
      }
    this.#model = value;
    this.#render();
  }
  #emit(type, detail) {
    this.dispatchEvent(new CustomEvent(type, { detail, bubbles: true, composed: true }));
  }
  #render() {
    const controls = this.#model?.controls || {};
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
        row = { signature, label, input };
        this.#rows.set(name, row);
      }
      const at = this.#fields.children[position++];
      if (at !== row.label) this.#fields.insertBefore(row.label, at || null);
      const value = this.#model.inputs[name];
      if (spec.type === 'boolean') {
        if (row.value !== !!value) row.input.checked = !!value;
        row.value = !!value;
      } else {
        const text =
          spec.type === 'json'
            ? JSON.stringify(value, null, 2)
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
    const actions = this.#model?.actions || [];
    this.#actions.replaceChildren(
      ...actions.map((action) =>
        node('li', action.name + (action.values.length ? ': ' + action.values.join(', ') : '')),
      ),
    );
    this.#empty.hidden = !!actions.length;
    this.#docs.hidden = !this.#model?.docs;
    this.#documentation.textContent = this.#model?.docs || '';
  }
}
