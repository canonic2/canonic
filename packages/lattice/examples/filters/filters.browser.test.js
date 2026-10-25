/**
 * @file Runs the filters example in Chromium: store → capability → binding →
 * element, including teardown.
 */
import { afterEach, expect, test, vi } from 'vitest';
import { mountFilters } from './app.js';
import { bindFilterField } from './bindings.js';
import { AcmeFilterField, defineAcmeFilterField } from './filter-field.js';
import { AcmeFilterSummary } from './filter-summary.js';
import { createFilters } from './filters.js';

defineAcmeFilterField();

/** @type {HTMLElement[]} */
const containers = [];

/**
 * Mounts the example into a fresh container in the document.
 * @param {Partial<import('./app.js').MountOptions>} [options] Ports to override.
 * @returns {{ container: HTMLElement, lifetime: AbortController, filters: import('./filters.js').Filters, fields: AcmeFilterField[], summary: AcmeFilterSummary }} The mounted example.
 */
function mount(options = {}) {
  const container = document.createElement('div');
  document.body.append(container);
  containers.push(container);
  const lifetime = new AbortController();
  const filters = mountFilters(container, { report: () => {}, ...options, signal: lifetime.signal });
  const fields = [...container.querySelectorAll('acme-filter-field')].map((node) => {
    if (!(node instanceof AcmeFilterField)) throw new Error('expected a filter field');
    return node;
  });
  const summary = container.querySelector('acme-filter-summary');
  if (!(summary instanceof AcmeFilterSummary)) throw new Error('expected a summary');
  return { container, lifetime, filters, fields, summary };
}

/**
 * Types into a field's control the way a person does: sets its value and fires `input`.
 * @param {AcmeFilterField} field The field.
 * @param {string} text The full text after typing.
 * @returns {void}
 */
function type(field, text) {
  const control = field.shadowRoot?.querySelector('input');
  if (!control) throw new Error('expected a control');
  control.value = text;
  control.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
}

/**
 * Counts assignments to the elements' inputs from now on.
 * @returns {{ value: import('vitest').MockInstance, disabled: import('vitest').MockInstance, text: import('vitest').MockInstance }} The setter spies.
 */
function spyOnInputs() {
  return {
    value: vi.spyOn(AcmeFilterField.prototype, 'value', 'set'),
    disabled: vi.spyOn(AcmeFilterField.prototype, 'disabled', 'set'),
    text: vi.spyOn(AcmeFilterSummary.prototype, 'text', 'set'),
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  for (const container of containers.splice(0)) container.remove();
});

test('applies every selected value once the example is mounted', () => {
  const { fields, summary } = mount();
  expect(fields.map((field) => [field.label, field.value, field.disabled])).toEqual([
    ['Filter 1', '', false],
    ['Filter 2', '', false],
    ['Filter 3', '', false],
  ]);
  expect(summary.text).toBe('All items');
});

test('typing in one field updates every field and the summary, and saves the query', () => {
  const save = vi.fn();
  const { fields, summary } = mount({ save });
  type(/** @type {AcmeFilterField} */ (fields[0]), 'acme');
  expect(fields.map((field) => field.value)).toEqual(['acme', 'acme', 'acme']);
  expect(fields.map((field) => field.shadowRoot?.querySelector('input')?.value)).toEqual(['acme', 'acme', 'acme']);
  expect(summary.text).toBe('Matching “acme”');
  expect(save).toHaveBeenCalledExactlyOnceWith('acme');
});

test('locking disables every field and reflects the attribute', () => {
  const { filters, fields } = mount();
  filters.toggleLocked();
  expect(fields.every((field) => field.disabled && field.hasAttribute('disabled'))).toBe(true);
});

test('assigns nothing for unchanged and unrelated updates', () => {
  const { filters } = mount();
  filters.setQuery('acme');
  const inputs = spyOnInputs();
  filters.setQuery('acme');
  filters.touch();
  expect(inputs.value).not.toHaveBeenCalled();
  expect(inputs.disabled).not.toHaveBeenCalled();
  expect(inputs.text).not.toHaveBeenCalled();
});

test('assigns only the inputs a write changes', () => {
  const { filters } = mount();
  const inputs = spyOnInputs();
  const controlDisabled = vi.spyOn(HTMLInputElement.prototype, 'disabled', 'set');
  filters.toggleLocked();
  // Each binding assignment is followed by the attribute callback's echo into
  // the same setter, which writes nothing because the value is unchanged.
  expect(inputs.disabled).toHaveBeenCalledTimes(6);
  expect(controlDisabled).toHaveBeenCalledTimes(3);
  expect(inputs.value).not.toHaveBeenCalled();
  expect(inputs.text).not.toHaveBeenCalled();
});

test('teardown removes the elements, releases observers, subscriptions and listeners', () => {
  const disconnect = vi.spyOn(ResizeObserver.prototype, 'disconnect');
  const save = vi.fn();
  const { lifetime, filters, fields, summary, container } = mount({ save });
  lifetime.abort();
  expect(container.children).toHaveLength(0);
  expect(disconnect).toHaveBeenCalledTimes(3);

  const inputs = spyOnInputs();
  filters.setQuery('after');
  filters.toggleLocked();
  expect(inputs.value).not.toHaveBeenCalled();
  expect(inputs.disabled).not.toHaveBeenCalled();
  expect(inputs.text).not.toHaveBeenCalled();
  expect(summary.text).toBe('All items');

  save.mockClear();
  fields[0]?.dispatchEvent(new CustomEvent('acme-filter-input', { detail: { value: 'ignored' } }));
  expect(save).not.toHaveBeenCalled();
  lifetime.abort();
});

test('presents a rejected save through report once, keeping the query set', async () => {
  const failure = new Error('save failed');
  const report = vi.fn();
  const { fields, filters } = mount({ save: () => Promise.reject(failure), report });
  type(/** @type {AcmeFilterField} */ (fields[0]), 'acme');
  await vi.waitFor(() => expect(report).toHaveBeenCalledExactlyOnceWith(failure));
  expect(filters.state.get().query).toBe('acme');
});

test('hands a failure to the platform when report itself throws', async () => {
  const platform = vi.fn();
  vi.stubGlobal('reportError', platform);
  const reportFailure = new Error('report');
  const { fields } = mount({
    save: () => Promise.reject(new Error('save')),
    report: () => {
      throw reportFailure;
    },
  });
  type(/** @type {AcmeFilterField} */ (fields[0]), 'acme');
  await vi.waitFor(() => expect(platform).toHaveBeenCalledExactlyOnceWith(reportFailure));
});

test('rolls back a binding whose setup fails, without aborting the caller’s lifetime', () => {
  const save = vi.fn();
  const filters = createFilters({ save });
  const failure = new Error('read');
  filters.select.locked = {
    fields: ['locked'],
    read: () => {
      throw failure;
    },
  };
  const field = new AcmeFilterField();
  const lifetime = new AbortController();
  expect(() => bindFilterField(field, filters, { signal: lifetime.signal, report: () => {} })).toThrow(failure);
  expect(lifetime.signal.aborted).toBe(false);
  const inputs = spyOnInputs();
  filters.setQuery('after');
  expect(inputs.value).not.toHaveBeenCalled();
  field.dispatchEvent(new CustomEvent('acme-filter-input', { detail: { value: 'ignored' } }));
  expect(save).toHaveBeenCalledExactlyOnceWith('after');
});

test('hands a save that fails after teardown to the platform instead of report', async () => {
  const failure = new Error('late');
  const report = vi.fn();
  const platform = vi.fn();
  vi.stubGlobal('reportError', platform);
  /** @type {(error: unknown) => void} */
  let reject = () => {};
  const { fields, lifetime } = mount({
    save: () =>
      new Promise((_, rejectSave) => {
        reject = rejectSave;
      }),
    report,
  });
  type(/** @type {AcmeFilterField} */ (fields[0]), 'acme');
  lifetime.abort();
  reject(failure);
  await vi.waitFor(() => expect(platform).toHaveBeenCalledExactlyOnceWith(failure));
  expect(report).not.toHaveBeenCalled();
});
