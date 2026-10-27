/** @file Real Chromium interaction, incarnation, anchoring and ownership coverage. */
import { afterEach, expect, test, vi } from 'vitest';
import { mountTasks } from './app.js';
import { createTasks } from './tasks.js';
import { TaskRow } from './task-row.js';
import { bindTaskRow } from './bindings.js';
import { TaskList, ROW_HEIGHT } from './task-list.js';

/** @type {{ lifetime: AbortController, host: HTMLElement }[]} */
const owned = [];
/**
 * Mount a real owned view for each case.
 * @param {number} [count] State size.
 * @returns {import('./app.js').TaskView & { lifetime: AbortController, host: HTMLElement }} Mounted view and test-owned lifetime.
 */
function mount(count = 1000) {
  const host = document.createElement('div');
  host.style.width = '640px';
  document.body.append(host);
  const lifetime = new AbortController();
  owned.push({ lifetime, host });
  return {
    ...mountTasks(host, { signal: lifetime.signal, tasks: createTasks(count), report: () => {} }),
    lifetime,
    host,
  };
}
/**
 * Get an expected retained row.
 * @param {import('./task-list.js').TaskList} list List owner.
 * @param {string} key Item.
 * @returns {TaskRow} The retained presentation for the key.
 */
function row(list, key) {
  const value = list.rows.get(key);
  if (!value) throw new Error(`Missing row ${key}`);
  return value;
}
/**
 * Get a row's native editor.
 * @param {TaskRow} item Row.
 * @returns {HTMLInputElement} The row's text editor.
 */
function editor(item) {
  const value = item.shadowRoot?.querySelector('input[type=text]');
  if (!(value instanceof HTMLInputElement)) throw new Error('Missing editor');
  return value;
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  for (const item of owned.splice(0)) {
    item.lifetime.abort();
    item.host.remove();
  }
});

let upgradeId = 0;

test('view teardown removes nodes and bindings even when list disposal fails', () => {
  const host = document.createElement('div');
  document.body.append(host);
  const lifetime = new AbortController();
  owned.push({ lifetime, host });
  const report = vi.fn();
  const { list, tasks } = mountTasks(host, {
    signal: lifetime.signal,
    tasks: createTasks(3),
    report,
  });
  const failure = new Error('dispose');
  vi.spyOn(list, 'dispose').mockImplementation(() => {
    throw failure;
  });
  lifetime.abort();
  expect(host.children).toHaveLength(0);
  expect(report).toHaveBeenCalledExactlyOnceWith(failure);
  const assign = vi.spyOn(TaskRow.prototype, 'text', 'set');
  tasks.rename('t0', 'After teardown');
  expect(assign).not.toHaveBeenCalled();
});

test('row upgrade captures safe inputs immediately and defers host decoration', async () => {
  const name = `acme-early-row-${upgradeId++}`,
    host = document.createElement('div');
  document.body.append(host);
  const early = /** @type {TaskRow} */ (document.createElement(name));
  early.text = 'Early';
  early.done = true;
  early.selected = true;
  host.append(early);
  try {
    customElements.define(name, class extends TaskRow {});
    expect(Object.hasOwn(early, 'text')).toBe(false);
    expect(early.text).toBe('Early');
    expect(editor(early).value).toBe('Early');
    expect(early.done).toBe(true);
    expect(early.selected).toBe(true);
    expect(early.hasAttribute('data-selected')).toBe(false);
    await Promise.resolve();
    expect(early.hasAttribute('data-selected')).toBe(true);
  } finally {
    host.remove();
  }
});

test('list upgrade preserves inputs and a newer assignment supersedes pending replay', async () => {
  const name = `acme-early-list-${upgradeId++}`,
    early = /** @type {TaskList} */ (document.createElement(name));
  early.windowed = false;
  early.selectedKey = 't1';
  customElements.define(name, class extends TaskList {});
  customElements.upgrade(early);
  try {
    expect(Object.hasOwn(early, 'windowed')).toBe(false);
    expect(early.windowed).toBe(false);
    early.windowed = true;
    early.selectedKey = null;
    await Promise.resolve();
    expect(early.windowed).toBe(true);
    expect(early.selectedKey).toBeNull();
  } finally {
    early.dispose();
  }
});

test('temporary detach and atomic moves keep rows, drafts and one connection', async () => {
  const { list, host, render, tasks } = mount(),
    first = row(list, 't1'),
    input = editor(first);
  input.focus();
  input.value = 'Draft';
  const connection = list.connection;
  host.moveBefore(list, host.firstChild);
  expect(list.connection).toBe(connection);
  expect(row(list, 't1')).toBe(first);
  expect(first.shadowRoot?.activeElement).toBe(input);
  list.viewport.scrollTop = 100 * ROW_HEIGHT;
  render();
  const range = vi.fn();
  list.addEventListener('acme-task-range', range);
  for (let turn = 0; turn < 3; turn++) {
    list.remove();
    expect(list.connection).toBeNull();
    host.append(list);
    await Promise.resolve();
    expect(row(list, 't1')).toBe(first);
    expect(input.value).toBe('Draft');
    expect(list.connection?.signal.aborted).toBe(false);
  }
  expect(range).not.toHaveBeenCalled();
  expect(list.rows.size).toBeLessThanOrEqual(24);
  const choose = vi.spyOn(tasks, 'choose');
  first.dispatchEvent(
    new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, composed: true }),
  );
  expect(choose).toHaveBeenCalledTimes(1);
});

test('resize after reconnect reconciles current geometry before the viewport observer runs', async () => {
  const NativeObserver = ResizeObserver;
  /** @type {string[]} */
  const deliveries = [];
  vi.stubGlobal(
    'ResizeObserver',
    class extends NativeObserver {
      /**
       * Record native delivery order without changing its scheduling.
       * @param {ResizeObserverCallback} callback Owner callback.
       */
      constructor(callback) {
        super((entries, observer) => {
          deliveries.push(
            entries.some((entry) => entry.target instanceof TaskList) ? 'composition' : 'viewport',
          );
          callback(entries, observer);
        });
      }
    },
  );
  const { list, host, render } = mount(),
    pinned = row(list, 't1'),
    input = editor(pinned);
  input.focus();
  input.value = 'Draft';
  list.viewport.scrollTop = 100 * ROW_HEIGHT;
  render();
  await new Promise(requestAnimationFrame);
  await new Promise(requestAnimationFrame);
  list.remove();
  host.append(list);
  await new Promise(requestAnimationFrame);
  await new Promise(requestAnimationFrame);
  // Establish the same scrolled window after the browser's detach scroll reset.
  list.viewport.scrollTop = 100 * ROW_HEIGHT;
  render();
  await new Promise(requestAnimationFrame);
  await new Promise(requestAnimationFrame);
  const retained = row(list, 't100');
  deliveries.length = 0;
  list.viewport.style.height = '960px';
  await new Promise(requestAnimationFrame);
  await new Promise(requestAnimationFrame);
  expect(deliveries.slice(0, 2)).toEqual(['composition', 'viewport']);
  expect(list.viewport.scrollTop).toBe(100 * ROW_HEIGHT);
  expect(list.rows.has('t129')).toBe(true);
  expect(list.rows.size).toBe(39);
  expect(row(list, 't100')).toBe(retained);
  expect(row(list, 't1')).toBe(pinned);
  expect(input.value).toBe('Draft');
  list.viewport.style.height = '320px';
  await new Promise(requestAnimationFrame);
  await new Promise(requestAnimationFrame);
  expect(list.rows.has('t129')).toBe(false);
  expect(list.rows.size).toBe(19);
  expect(row(list, 't100')).toBe(retained);
  expect(row(list, 't1')).toBe(pinned);
  expect(input.value).toBe('Draft');
});

test('keyboard destinations use resized geometry before observer delivery', () => {
  const { list, render, focusKey } = mount();
  list.viewport.scrollTop = 100 * ROW_HEIGHT;
  render();
  list.viewport.style.height = '320px';
  focusKey('t114');
  const destination = row(list, 't114');
  expect(list.shadowRoot?.activeElement).toBe(destination);
  expect(list.viewport.scrollTop).toBe(115 * ROW_HEIGHT - 320);
  expect(destination.getBoundingClientRect().bottom).toBeLessThanOrEqual(
    list.viewport.getBoundingClientRect().bottom,
  );
});

test('an ancestor removing the list before its queued connection prevents acquisition', () => {
  mount(1);
  const name = `acme-removing-parent-${upgradeId++}`,
    list = new TaskList();
  customElements.define(
    name,
    class extends HTMLElement {
      /**
       * Remove the child before its queued connected callback runs.
       * @returns {void}
       */
      connectedCallback() {
        this.firstElementChild?.remove();
      }
    },
  );
  const parent = document.createElement(name);
  parent.append(list);
  const observe = vi.spyOn(ResizeObserver.prototype, 'observe');
  try {
    document.body.append(parent);
    expect(list.isConnected).toBe(false);
    expect(list.connection).toBeNull();
    expect(observe).not.toHaveBeenCalled();
  } finally {
    list.dispose();
    parent.remove();
  }
});

test('connection failure after observation rolls back and detached callbacks acquire nothing', () => {
  const { list } = mount(),
    active = new Set(),
    originalObserve = ResizeObserver.prototype.observe,
    originalDisconnect = ResizeObserver.prototype.disconnect;
  vi.spyOn(ResizeObserver.prototype, 'observe').mockImplementation(
    /**
     * Track observers acquired by this fixture.
     * @this {ResizeObserver}
     * @param {Parameters<ResizeObserver['observe']>} args Native observation arguments.
     * @returns {void} Delegates acquisition to the native observer.
     */ function (...args) {
      active.add(this);
      return originalObserve.apply(this, args);
    },
  );
  vi.spyOn(ResizeObserver.prototype, 'disconnect').mockImplementation(
    /**
     * Track observers released by this fixture.
     * @this {ResizeObserver}
     * @returns {void} Delegates release to the native observer.
     */ function () {
      active.delete(this);
      return originalDisconnect.call(this);
    },
  );
  const range = vi.fn();
  list.addEventListener('acme-task-range', range);
  const failure = new Error('render failed'),
    render = vi.spyOn(list, 'render').mockImplementationOnce(() => {
      expect(active.size).toBe(1);
      throw failure;
    });
  expect(() => list.connectedCallback()).toThrow(failure);
  expect(active.size).toBe(0);
  expect(list.connection).toBeNull();
  render.mockClear();
  list.viewport.dispatchEvent(new Event('scroll'));
  expect(range).not.toHaveBeenCalled();
  list.remove();
  list.connectedCallback();
  expect(active.size).toBe(0);
  expect(list.connection).toBeNull();
});

test('upgrade replays supplied rows outside construction and honors newer default-valued inputs', async () => {
  mount(1); // Register the ordinary row constructor through the shipping entry.
  const name = `acme-supplied-list-${upgradeId++}`,
    early = /** @type {TaskList} */ (document.createElement(name));
  const item = new TaskRow(),
    items = Object.freeze([{ key: 'early', row: item, index: 0 }]);
  early.keys = Object.freeze(['early']);
  early.items = items;
  early.selectedKey = 'early';
  const selected = vi.spyOn(TaskRow.prototype, 'selected', 'set');
  customElements.define(name, class extends TaskList {});
  customElements.upgrade(early);
  try {
    expect(early.keys).toEqual(['early']);
    expect(early.items).toBe(items);
    expect(Object.hasOwn(early, 'items')).toBe(false);
    expect(selected).not.toHaveBeenCalled();
    expect(early.rows.size).toBe(0);
    early.selectedKey = null;
    await Promise.resolve();
    expect(early.rows.get('early')).toBe(item);
    expect(item.selected).toBe(false);
    expect(early.selectedKey).toBeNull();
    expect(early.items).toBe(items);
  } finally {
    early.dispose();
  }
});

test('upgrade selection honors captured defaults and attributes changed before replay', async () => {
  for (const changed of [false, true]) {
    const name = `acme-reflected-row-${upgradeId++}`,
      early = /** @type {TaskRow} */ (document.createElement(name));
    early.setAttribute('data-selected', '');
    early.selected = false;
    customElements.define(name, class extends TaskRow {});
    customElements.upgrade(early);
    try {
      expect(early.selected).toBe(false);
      if (changed) {
        early.removeAttribute('data-selected');
        early.setAttribute('data-selected', '');
      }
      await Promise.resolve();
      expect(early.selected).toBe(changed);
      expect(early.hasAttribute('data-selected')).toBe(changed);
    } finally {
      early.remove();
    }
  }
});

test('entering-row binding failure rolls back acquired rows and allows a clean retry', () => {
  const { tasks, list, render, lifetime } = mount(),
    original = tasks.select.done,
    failure = new Error('entering row');
  let reads = 0;
  tasks.select.done = {
    fields: ['done'],
    read: (state) => {
      if (++reads === 3) throw failure;
      return state.done;
    },
  };
  list.viewport.scrollTop = 100 * ROW_HEIGHT;
  expect(() => render()).toThrow(failure);
  expect(list.rows.size).toBe(0);
  expect(lifetime.signal.aborted).toBe(false);
  const setter = vi.spyOn(TaskRow.prototype, 'text', 'set');
  tasks.rename('t96', 'After failure');
  expect(setter).not.toHaveBeenCalled();
  tasks.select.done = original;
  render();
  expect(list.rows.size).toBe(23);
  setter.mockClear();
  tasks.rename('t96', 'After retry');
  expect(setter).toHaveBeenCalledTimes(1);
});

test('10,000 items retain a window and selection patches only the affected rows', () => {
  const { tasks, list, render, focusKey } = mount(10000),
    first = row(list, 't1');
  expect(list.rows.size).toBeLessThanOrEqual(23);
  render();
  render();
  expect(row(list, 't1')).toBe(first);
  tasks.choose('t1');
  const selected = vi.spyOn(TaskRow.prototype, 'selected', 'set');
  tasks.choose('t2');
  expect(selected).toHaveBeenCalledTimes(2);
  selected.mockClear();
  tasks.choose('t2');
  expect(selected).not.toHaveBeenCalled();
  tasks.choose('t9999');
  expect(list.rows.has('t9999')).toBe(false);
  focusKey('t9999');
  expect(row(list, 't9999').selected).toBe(true);
});

test('focused drafts stay pinned and keyboard destinations exist before focus', () => {
  const { list, render } = mount();
  const first = row(list, 't1'),
    input = editor(first);
  input.focus();
  input.value = 'Uncommitted draft';
  list.viewport.scrollTop = 100 * ROW_HEIGHT;
  render();
  expect(row(list, 't1')).toBe(first);
  expect(first.shadowRoot?.activeElement).toBe(input);
  expect(input.value).toBe('Uncommitted draft');
  expect(list.rows.size).toBeLessThanOrEqual(24);
  first.dispatchEvent(
    new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, composed: true }),
  );
  expect(list.shadowRoot?.activeElement).toBe(row(list, 't2'));
  expect(row(list, 't2').isConnected).toBe(true);
});

test('insertion and removal above the viewport preserve the anchor', () => {
  const { tasks, list, render } = mount();
  list.viewport.scrollTop = 100 * ROW_HEIGHT + 7;
  render();
  const anchor = row(list, 't100'),
    top = anchor.getBoundingClientRect().top;
  const inserted = tasks.insert('t0');
  expect(Math.abs(anchor.getBoundingClientRect().top - top)).toBeLessThan(0.5);
  tasks.remove(inserted);
  expect(Math.abs(anchor.getBoundingClientRect().top - top)).toBeLessThan(0.5);
});

test('keyboard navigation scrolls a retained overscan destination into view', () => {
  const { list, focusKey } = mount();
  focusKey('t14');
  row(list, 't14').dispatchEvent(
    new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, composed: true }),
  );
  expect(list.shadowRoot?.activeElement).toBe(row(list, 't15'));
  const destination = row(list, 't15').getBoundingClientRect(),
    viewport = list.viewport.getBoundingClientRect();
  expect(destination.bottom).toBeLessThanOrEqual(viewport.bottom);
});

test('key reuse ends old bindings, replaces the node and applies focus fallback', () => {
  const { tasks, list } = mount();
  const old = row(list, 't2');
  editor(old).focus();
  tasks.choose('t2');
  tasks.reuse('t2');
  const next = row(list, 't2');
  expect(next).not.toBe(old);
  expect(list.shadowRoot?.activeElement).toBe(next);
  expect(next.selected).toBe(false);
  old.dispatchEvent(new CustomEvent('acme-task-rename', { detail: { text: 'Stale' } }));
  expect(tasks.items.get('t2')?.text).toBe('Replacement t2');
  tasks.rename('t2', 'Current');
  expect(next.text).toBe('Current');
  expect(old.text).toBe('Task 3');
});

test('empty removal focuses the list and duplicate membership leaves the view intact', () => {
  const { tasks, list } = mount(1),
    first = row(list, 't0');
  expect(() => {
    list.keys = ['t0', 't0'];
  }).toThrow(TypeError);
  expect(row(list, 't0')).toBe(first);
  list.focusRow('t0');
  tasks.remove('t0');
  expect(document.activeElement).toBe(list);
  expect(list.rows.size).toBe(0);
});

test('teardown ends bindings and partial binding setup does not abort its caller', () => {
  const { tasks, list, lifetime, host } = mount();
  const first = row(list, 't0');
  lifetime.abort();
  expect(host.children).toHaveLength(0);
  const setter = vi.spyOn(TaskRow.prototype, 'text', 'set');
  tasks.rename('t0', 'After');
  expect(setter).not.toHaveBeenCalled();
  first.dispatchEvent(new CustomEvent('acme-task-select'));
  expect(tasks.state.get().selectedKey).toBeNull();
  const caller = new AbortController(),
    failure = new Error('setup');
  tasks.select.done = {
    fields: ['done'],
    read: () => {
      throw failure;
    },
  };
  expect(() =>
    bindTaskRow(new TaskRow(), tasks, 't1', { signal: caller.signal, report: () => {} }),
  ).toThrow(failure);
  expect(caller.signal.aborted).toBe(false);
  setter.mockClear();
  tasks.rename('t1', 'After failure');
  expect(setter).not.toHaveBeenCalled();
});
