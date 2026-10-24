/** @file Verifies the element helpers and the early-input rules in Chromium. */
import { afterEach, describe, expect, test, vi } from 'vitest';
import { captureInputs, defineOnce, replayInputs, skipUpgradeAttribute } from './elements.js';

/** @returns {string} A tag name no other test defines. */
function uniqueTag() {
  return `acme-test-${crypto.randomUUID()}`;
}

/**
 * Narrows a node to an element class, failing the test otherwise.
 * @template {HTMLElement} T
 * @param {Node | null} node The node to narrow.
 * @param {new () => T} type The class it must be an instance of.
 * @returns {T} The same node, typed as the class.
 */
function as(node, type) {
  if (!(node instanceof type)) throw new Error(`expected an instance of ${type.name}`);
  return node;
}

/** @returns {Promise<void>} Resolves after microtasks queued so far, including replays. */
const microtasks = () => Promise.resolve();

/**
 * Creates a fresh counter class whose input is replayed in a microtask, so the
 * pending-window rules can be tested on it.
 * @returns {new () => HTMLElement & { count: number, writes: number, readonly rendered: string }} A class that has not been defined.
 */
function counterClass() {
  return class Counter extends HTMLElement {
    #text = document.createTextNode('0');
    #count = 0;
    /** @type {Map<string, unknown> | null} */
    #pending = null;
    /** Number of DOM writes the element made. */
    writes = 0;

    /** Builds the view and captures a count assigned before upgrade. */
    constructor() {
      super();
      this.attachShadow({ mode: 'open' }).append(this.#text);
      const pending = captureInputs(this, ['count']);
      if (pending !== null) {
        this.#pending = pending;
        queueMicrotask(() => {
          replayInputs(this, pending);
          this.#pending = null;
        });
      }
    }

    /** @returns {number} The count, including one not yet replayed. */
    get count() {
      return this.#pending?.has('count') ? Number(this.#pending.get('count')) : this.#count;
    }

    /** @param {number} value The count to display. */
    set count(value) {
      this.#pending?.delete('count');
      if (Object.is(value, this.#count)) return;
      this.#count = value;
      this.writes++;
      this.#text.data = String(value);
    }

    /** @returns {string} The rendered text. */
    get rendered() {
      return this.#text.data;
    }
  };
}

/**
 * Creates a fresh class with one input reflected to the `label` attribute.
 * @returns {new () => HTMLElement & { label: string, readonly rendered: string }} A class that has not been defined.
 */
function labelClass() {
  return class Label extends HTMLElement {
    static observedAttributes = ['label'];
    #text = document.createTextNode('');
    #label = '';
    /** @type {Map<string, unknown> | null} */
    #pending = null;

    /** Builds the view and captures a label assigned before upgrade. */
    constructor() {
      super();
      this.attachShadow({ mode: 'open' }).append(this.#text);
      const pending = captureInputs(this, ['label'], { label: 'label' });
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

    /** @param {string} value The label to display and reflect. */
    set label(value) {
      this.#pending?.delete('label');
      if (!Object.is(value, this.#label)) {
        this.#label = value;
        this.#text.data = value;
      }
      if (this.getAttribute('label') !== value) this.setAttribute('label', value);
    }

    /**
     * Applies the `label` attribute, except during upgrade for a captured property.
     * @param {string} name The attribute name.
     * @param {string | null} oldValue The previous value, `null` when it was absent.
     * @param {string | null} newValue The new value, `null` when it was removed.
     */
    attributeChangedCallback(name, oldValue, newValue) {
      if (skipUpgradeAttribute(this.#pending, name)) return;
      this.label = newValue ?? '';
    }

    /** @returns {string} The rendered text. */
    get rendered() {
      return this.#text.data;
    }
  };
}

/**
 * Creates an element of a tag that is not defined yet, assigns properties and
 * attributes to it, then defines the class and upgrades the element.
 * @template {HTMLElement} T
 * @param {new () => T} type The class to define.
 * @param {Record<string, unknown>} properties Properties assigned before upgrade.
 * @param {Record<string, string>} [attributes] Attributes set before upgrade.
 * @returns {T} The upgraded element.
 */
function upgradeWith(type, properties, attributes = {}) {
  const tag = uniqueTag();
  const element = document.createElement(tag);
  Object.assign(element, properties);
  for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, value);
  customElements.define(tag, type);
  customElements.upgrade(element);
  return as(element, type);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('captureInputs', () => {
  test('captures only declared own properties and removes them from the host', () => {
    const element = upgradeWith(counterClass(), { count: 5, other: 1 });
    expect(Object.hasOwn(element, 'count')).toBe(false);
    expect(Object.hasOwn(element, 'other')).toBe(true);
    expect(element.count).toBe(5);
  });

  test('returns null and queues no replay when nothing was assigned before upgrade', () => {
    const Counter = counterClass();
    const tag = uniqueTag();
    customElements.define(tag, Counter);
    const queued = vi.spyOn(globalThis, 'queueMicrotask');
    const element = as(document.createElement(tag), Counter);
    expect(queued).not.toHaveBeenCalled();
    expect(captureInputs(element, ['count'])).toBeNull();
  });
});

describe('failed reflection operations', () => {
  test('captureInputs throws when an early property cannot be removed', () => {
    const Counter = counterClass();
    const tag = uniqueTag();
    customElements.define(tag, Counter);
    const element = as(document.createElement(tag), Counter);
    Object.defineProperty(element, 'count', { value: 5, writable: true, configurable: false });
    expect(() => captureInputs(element, ['count'])).toThrow(/count/);
  });

  test('replayInputs throws when a captured value cannot be assigned', () => {
    /** An element whose input has a getter and no setter. */
    class ReadOnly extends HTMLElement {
      /** @returns {number} Always zero. */
      get fixed() {
        return 0;
      }
    }
    customElements.define(uniqueTag(), ReadOnly);
    const element = new ReadOnly();
    /** @type {Map<'fixed', unknown>} */
    const pending = new Map([['fixed', 5]]);
    expect(() => replayInputs(element, pending)).toThrow(/fixed/);
  });
});

describe('replayInputs', () => {
  test('reflects a captured value equal to the default over an attribute present at upgrade', async () => {
    const Label = labelClass();
    const tag = uniqueTag();
    const element = document.createElement(tag);
    Object.assign(element, { label: '' });
    element.setAttribute('label', 'Attr');
    customElements.define(tag, Label);
    customElements.upgrade(element);
    const label = as(element, Label);
    await microtasks();
    expect(label.label).toBe('');
    expect(label.rendered).toBe('');
    expect(label.getAttribute('label')).toBe('');
  });

  test('applies a captured value once through the setter', async () => {
    const element = upgradeWith(counterClass(), { count: 5 });
    expect(element.rendered).toBe('0');
    await microtasks();
    expect(element.rendered).toBe('5');
    expect(element.count).toBe(5);
    expect(element.writes).toBe(1);
  });

  test('keeps an assignment made after upgrade and before the replay', async () => {
    const element = upgradeWith(counterClass(), { count: 5 });
    element.count = 7;
    await microtasks();
    expect(element.rendered).toBe('7');
    expect(element.count).toBe(7);
    expect(element.writes).toBe(1);
  });

  test('writes nothing when the newer assignment equals the rendered value', async () => {
    const element = upgradeWith(counterClass(), { count: 5 });
    element.count = 0;
    await microtasks();
    expect(element.rendered).toBe('0');
    expect(element.count).toBe(0);
    expect(element.writes).toBe(0);
  });

  test('applies an attribute first added after upgrade and before the replay', async () => {
    const Label = labelClass();
    const tag = uniqueTag();
    const element = document.createElement(tag);
    Object.assign(element, { label: 'Prop' });
    customElements.define(tag, Label);
    customElements.upgrade(element);
    const label = as(element, Label);
    label.setAttribute('label', 'New');
    await microtasks();
    expect(label.label).toBe('New');
    expect(label.rendered).toBe('New');
    expect(label.getAttribute('label')).toBe('New');
  });

  test('applies an attribute changed after upgrade and before the replay', async () => {
    const Label = labelClass();
    const tag = uniqueTag();
    const element = document.createElement(tag);
    Object.assign(element, { label: 'Prop' });
    element.setAttribute('label', 'Attr');
    customElements.define(tag, Label);
    customElements.upgrade(element);
    const label = as(element, Label);
    label.setAttribute('label', 'New');
    await microtasks();
    expect(label.label).toBe('New');
    expect(label.rendered).toBe('New');
    expect(label.getAttribute('label')).toBe('New');
  });

  test('checks attribute presence only for inputs captured as properties', () => {
    const Label = labelClass();
    const tag = uniqueTag();
    customElements.define(tag, Label);
    const checked = vi.spyOn(Element.prototype, 'hasAttribute');
    const container = document.createElement('div');
    container.innerHTML = `<${tag} label="Acme"></${tag}>`;
    document.createElement(tag);
    expect(checked).not.toHaveBeenCalled();
  });

  test('lets a captured property win over an attribute present at upgrade', async () => {
    const Label = labelClass();
    const tag = uniqueTag();
    const element = document.createElement(tag);
    Object.assign(element, { label: 'Prop' });
    element.setAttribute('label', 'Attr');
    const observer = new MutationObserver(() => {});
    observer.observe(element, { attributes: true });
    customElements.define(tag, Label);
    customElements.upgrade(element);
    const label = as(element, Label);
    expect(observer.takeRecords()).toHaveLength(0);
    expect(label.label).toBe('Prop');
    await microtasks();
    observer.disconnect();
    expect(label.getAttribute('label')).toBe('Prop');
    expect(label.rendered).toBe('Prop');
  });
});

describe('skipUpgradeAttribute', () => {
  test('returns false when nothing is pending or the attribute was not present at capture', () => {
    expect(skipUpgradeAttribute(null, 'label')).toBe(false);
    expect(skipUpgradeAttribute(new Map([['label', 'Prop']]), 'label')).toBe(false);
  });
});

describe('defineOnce', () => {
  test('defines a constructor once and accepts the same constructor again', () => {
    const Counter = counterClass();
    const tag = uniqueTag();
    defineOnce(customElements, tag, Counter);
    expect(() => defineOnce(customElements, tag, Counter)).not.toThrow();
    expect(customElements.get(tag)).toBe(Counter);
  });

  test('throws for a different constructor and keeps the first', () => {
    const Counter = counterClass();
    const tag = uniqueTag();
    defineOnce(customElements, tag, Counter);
    expect(() => defineOnce(customElements, tag, counterClass())).toThrow(tag);
    expect(customElements.get(tag)).toBe(Counter);
  });
});

describe('inputs without an attribute applied during construction', () => {
  /**
   * Creates a fresh class combining a reflected label, replayed in a microtask,
   * with a count applied in the constructor.
   * @returns {new () => HTMLElement & { label: string, count: number, readonly rendered: string }} A class that has not been defined.
   */
  function mixedClass() {
    return class Mixed extends HTMLElement {
      static observedAttributes = ['label'];
      #text = document.createTextNode(': 0');
      #label = '';
      #count = 0;
      /** @type {Map<string, unknown> | null} */
      #pending = null;

      /** Builds the view, applies an early count and captures an early label. */
      constructor() {
        super();
        this.attachShadow({ mode: 'open' }).append(this.#text);
        const early = captureInputs(this, ['count']);
        if (early !== null) replayInputs(this, early);
        const pending = captureInputs(this, ['label'], { label: 'label' });
        if (pending !== null) {
          this.#pending = pending;
          queueMicrotask(() => {
            replayInputs(this, pending);
            this.#pending = null;
          });
        }
      }

      /** @returns {number} The count. */
      get count() {
        return this.#count;
      }

      /** @param {number} value The count to display. */
      set count(value) {
        if (Object.is(value, this.#count)) return;
        this.#count = value;
        this.#render();
      }

      /** @returns {string} The label, including one not yet replayed. */
      get label() {
        return this.#pending?.has('label') ? String(this.#pending.get('label')) : this.#label;
      }

      /** @param {string} value The label to display and reflect. */
      set label(value) {
        this.#pending?.delete('label');
        if (!Object.is(value, this.#label)) {
          this.#label = value;
          this.#render();
        }
        if (this.getAttribute('label') !== value) this.setAttribute('label', value);
      }

      /**
       * Applies the `label` attribute, except during upgrade for a captured property.
       * @param {string} name The attribute name.
       * @param {string | null} oldValue The previous value, `null` when it was absent.
       * @param {string | null} newValue The new value, `null` when it was removed.
       */
      attributeChangedCallback(name, oldValue, newValue) {
        if (skipUpgradeAttribute(this.#pending, name)) return;
        this.label = newValue ?? '';
      }

      /** Writes the text derived from both inputs. */
      #render() {
        this.#text.data = `${this.#label}: ${this.#count}`;
      }

      /** @returns {string} The rendered text. */
      get rendered() {
        return this.#text.data;
      }
    };
  }

  test('renders and returns an early input as soon as upgrade returns', () => {
    const element = upgradeWith(mixedClass(), { count: 5 });
    expect(element.count).toBe(5);
    expect(element.rendered).toBe(': 5');
  });

  test('writes no host attribute and dispatches no event during construction', () => {
    const Mixed = mixedClass();
    const tag = uniqueTag();
    const element = document.createElement(tag);
    Object.assign(element, { count: 5 });
    const observer = new MutationObserver(() => {});
    observer.observe(element, { attributes: true });
    const listener = vi.fn();
    element.addEventListener('acme-change', listener);
    customElements.define(tag, Mixed);
    customElements.upgrade(element);
    expect(observer.takeRecords()).toHaveLength(0);
    expect(listener).not.toHaveBeenCalled();
    observer.disconnect();
  });

  test('derives output from a constructor-applied and a replayed input', async () => {
    const element = upgradeWith(mixedClass(), { count: 5, label: 'Acme' }, { label: 'Attr' });
    expect(element.rendered).toBe(': 5');
    await microtasks();
    expect(element.rendered).toBe('Acme: 5');
    expect(element.getAttribute('label')).toBe('Acme');
  });

  test('queues no replay for an element upgraded without early inputs', () => {
    const Mixed = mixedClass();
    const tag = uniqueTag();
    customElements.define(tag, Mixed);
    const queued = vi.spyOn(globalThis, 'queueMicrotask');
    as(document.createElement(tag), Mixed);
    expect(queued).not.toHaveBeenCalled();
  });

  test('renders defaults for elements created by the parser and by cloning', () => {
    const Mixed = mixedClass();
    const tag = uniqueTag();
    customElements.define(tag, Mixed);
    const container = document.createElement('div');
    container.innerHTML = `<${tag} label="Acme"></${tag}>`;
    const parsed = as(container.firstElementChild, Mixed);
    expect(parsed.rendered).toBe('Acme: 0');
    const clone = as(parsed.cloneNode(), Mixed);
    expect(clone.rendered).toBe('Acme: 0');
    expect(clone.getAttribute('label')).toBe('Acme');
  });
});
