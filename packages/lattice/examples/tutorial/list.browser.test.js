/** @file Exercises the guide's real list mount, capability actions and composed application. */
import { afterEach, expect, test, vi } from 'vitest';
import { createTasks } from './tasks.js';
import { mountTaskRows } from './task-list.js';
import { mountTutorial } from './app.js';
import { TaskToggle } from './task-toggle.js';
import { TaskLabel } from './task-label.js';

/** @type {HTMLElement[]} */
const nodes = [];
/** @type {AbortController[]} */
const lifetimes = [];

/**
 * Creates a connected container owned by the current test.
 * @returns {HTMLDivElement} The empty container.
 */
function container() {
  const host = document.createElement('div');
  document.body.append(host);
  nodes.push(host);
  return host;
}

/** @returns {AbortController} A view lifetime released after the test. */
function lifetime() {
  const owner = new AbortController();
  lifetimes.push(owner);
  return owner;
}

/**
 * Mounts the exact helper used by the guide.
 * @returns {{tasks: import('./tasks.js').Tasks, list: HTMLUListElement, owner: AbortController}} The live fixture.
 */
function mount() {
  const tasks = createTasks();
  const list = document.createElement('ul');
  container().append(list);
  const owner = lifetime();
  mountTaskRows(list, tasks, { signal: owner.signal });
  return { tasks, list, owner };
}

/**
 * Looks up a retained native row.
 * @param {HTMLUListElement} list The list.
 * @param {number} index Its position.
 * @returns {HTMLLIElement} The row.
 */
function row(list, index) {
  const node = list.children[index];
  if (!(node instanceof HTMLLIElement)) throw new Error('Missing row.');
  return node;
}

/**
 * Finds the row's native checkbox.
 * @param {HTMLLIElement} node The retained row.
 * @returns {HTMLInputElement} Its control.
 */
function checkbox(node) {
  const input = node.querySelector('input');
  if (!input) throw new Error('Missing checkbox.');
  return input;
}

afterEach(() => {
  vi.restoreAllMocks();
  for (const owner of lifetimes.splice(0)) owner.abort();
  for (const node of nodes.splice(0)) node.remove();
  vi.unstubAllGlobals();
});

test('mounts current values and applies item changes without replacing other rows', () => {
  const { tasks, list } = mount();
  const first = row(list, 0);
  const second = row(list, 1);
  expect([first.textContent, second.textContent]).toEqual([
    'Write the guide',
    'Review the examples',
  ]);
  tasks.rename('guide', 'Updated guide');
  tasks.setDone('review', true);
  expect(row(list, 0)).toBe(first);
  expect(row(list, 1)).toBe(second);
  expect(first.textContent).toBe('Updated guide');
  expect(checkbox(second).checked).toBe(true);
  const toggle = vi.spyOn(tasks, 'toggle');
  checkbox(first).checked = true;
  checkbox(first).dispatchEvent(new Event('change'));
  expect(toggle).toHaveBeenCalledExactlyOnceWith('guide');
  expect(tasks.items.get('guide')?.done).toBe(true);
});

test('insertion and reorder retain existing controls and connected focus', () => {
  const { tasks, list } = mount();
  const guide = row(list, 0);
  const control = checkbox(guide);
  control.focus();
  tasks.add('first', 'First', 'guide');
  expect(row(list, 1)).toBe(guide);
  expect(document.activeElement).toBe(control);
  tasks.reverse();
  expect([...list.children].map((node) => node.textContent)).toEqual([
    'Review the examples',
    'Write the guide',
    'First',
  ]);
  expect(row(list, 1)).toBe(guide);
  expect(document.activeElement).toBe(control);
});

test('replacement creates a new incarnation row and ends the old row listener', () => {
  const { tasks, list } = mount();
  const old = row(list, 0);
  const retained = row(list, 1);
  tasks.replace('guide', 'Replacement');
  expect(row(list, 0)).not.toBe(old);
  expect(row(list, 1)).toBe(retained);
  expect(row(list, 0).textContent).toBe('Replacement');
  expect(old.isConnected).toBe(false);
  checkbox(old).dispatchEvent(new Event('change'));
  expect(tasks.items.get('guide')?.done).toBe(false);
  tasks.rename('guide', 'New incarnation');
  expect(old.textContent).toBe('Write the guide');
  expect(row(list, 0).textContent).toBe('New incarnation');
});

test('removal and abort end item bindings and detached control listeners', () => {
  const { tasks, list, owner } = mount();
  const removed = row(list, 0);
  const remaining = row(list, 1);
  tasks.remove('guide');
  checkbox(removed).dispatchEvent(new Event('change'));
  owner.abort();
  expect(list.children).toHaveLength(0);
  tasks.rename('review', 'After teardown');
  checkbox(remaining).dispatchEvent(new Event('change'));
  expect(remaining.textContent).toBe('Review the examples');
  expect(tasks.items.get('review')?.done).toBe(false);
  owner.abort();
});

test('an already aborted view acquires no item watchers or rows', () => {
  const tasks = createTasks();
  const watch = vi.spyOn(tasks.items, 'watchItem');
  const list = document.createElement('ul');
  const owner = lifetime();
  owner.abort();
  mountTaskRows(list, tasks, { signal: owner.signal });
  expect(watch).not.toHaveBeenCalled();
  expect(list.children).toHaveLength(0);
});

test('partial setup failure releases earlier rows without aborting the caller', () => {
  const tasks = createTasks();
  const list = document.createElement('ul');
  const owner = lifetime();
  /** @type {AbortSignal[]} */
  const acquired = [];
  const watch = tasks.items.watchItem;
  const failure = new Error('second row');
  vi.spyOn(tasks.items, 'watchItem').mockImplementation((key, selection, run, options) => {
    if (options?.signal) acquired.push(options.signal);
    if (key === 'review') throw failure;
    watch(key, selection, run, options);
  });
  expect(() => mountTaskRows(list, tasks, { signal: owner.signal })).toThrow(failure);
  expect(list.children).toHaveLength(0);
  expect(owner.signal.aborted).toBe(false);
  expect(acquired.length).toBeGreaterThan(0);
  expect(acquired.every((signal) => signal.aborted)).toBe(true);
});

test('abort attempts every row release even when one node removal fails', () => {
  const { tasks, list, owner } = mount();
  const first = row(list, 0);
  const last = row(list, 1);
  const failure = new Error('remove');
  const report = vi.fn();
  vi.stubGlobal('reportError', report);
  vi.spyOn(last, 'remove').mockImplementation(() => {
    throw failure;
  });
  owner.abort();
  expect(list.children).toHaveLength(1);
  expect(first.isConnected).toBe(false);
  expect(report).toHaveBeenCalledExactlyOnceWith(failure);
  tasks.rename('review', 'Ignored by ended view');
  checkbox(last).dispatchEvent(new Event('change'));
  expect(last.textContent).toBe('Review the examples');
  expect(tasks.items.get('review')?.done).toBe(false);
});

test('the composed page shares completion across both recipes and ends the full view', () => {
  const host = container();
  const owner = lifetime();
  const tasks = mountTutorial(host, { signal: owner.signal });
  const toggle = host.querySelector('acme-task-toggle');
  const label = host.querySelector('acme-task-label');
  const list = host.querySelector('ul');
  if (!tasks || !(toggle instanceof TaskToggle) || !(label instanceof TaskLabel) || !list) {
    throw new Error('Missing tutorial view.');
  }
  const control = toggle.shadowRoot?.querySelector('input');
  if (!control) throw new Error('Missing toggle control.');
  control.checked = true;
  control.dispatchEvent(new Event('change'));
  expect(tasks.items.get('guide')?.done).toBe(true);
  expect(checkbox(row(list, 0)).checked).toBe(true);
  expect(label.text).toBe('Complete');
  const setDone = vi.spyOn(tasks, 'setDone');
  owner.abort();
  expect(host.children).toHaveLength(0);
  tasks.setDone('guide', false);
  setDone.mockClear();
  toggle.dispatchEvent(new CustomEvent('acme-task-toggle', { detail: { done: true } }));
  expect(setDone).not.toHaveBeenCalled();
  expect(label.text).toBe('Complete');
});

test('composition removes earlier acquisitions if appending the view fails', () => {
  const host = container();
  const owner = lifetime();
  const failure = new Error('append');
  const add = vi.spyOn(EventTarget.prototype, 'addEventListener');
  vi.spyOn(host, 'append').mockImplementation(() => {
    throw failure;
  });
  expect(() => mountTutorial(host, { signal: owner.signal })).toThrow(failure);
  expect(owner.signal.aborted).toBe(false);
  expect(host.children).toHaveLength(0);
  /** @type {AbortSignal[]} */
  const acquired = [];
  for (const [name, , options] of add.mock.calls) {
    if (
      (name === 'change' || name === 'acme-task-toggle') &&
      options &&
      typeof options === 'object' &&
      options.signal
    )
      acquired.push(options.signal);
  }
  expect(acquired.length).toBeGreaterThan(0);
  expect(acquired.every((signal) => signal.aborted)).toBe(true);
});
