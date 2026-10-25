/** @file Real Chromium interaction, incarnation, anchoring and ownership coverage. */
import { afterEach, expect, test, vi } from 'vitest';
import { mountTasks } from './app.js';
import { createTasks } from './tasks.js';
import { TaskRow } from './task-row.js';
import { bindTaskRow } from './bindings.js';
import { ROW_HEIGHT } from './task-list.js';

/** @type {{ lifetime: AbortController, host: HTMLElement }[]} */
const owned = [];
/**
 * Mount a real owned view for each case.
 * @param {number} [count] State size.
 * @returns {import('./app.js').TaskView & { lifetime: AbortController, host: HTMLElement }}
 */
function mount(count = 1000) {
  const host = document.createElement('div'); host.style.width = '640px'; document.body.append(host);
  const lifetime = new AbortController(); owned.push({ lifetime, host });
  return { ...mountTasks(host, { signal: lifetime.signal, tasks: createTasks(count), report: () => {} }), lifetime, host };
}
/** Get an expected retained row. @param {import('./task-list.js').TaskList} list List owner. @param {string} key Item. @returns {TaskRow} */
function row(list, key) { const value = list.rows.get(key); if (!value) throw new Error(`Missing row ${key}`); return value; }
/** Get a row's native editor. @param {TaskRow} item Row. @returns {HTMLInputElement} */
function editor(item) { const value = item.shadowRoot?.querySelector('input[type=text]'); if (!(value instanceof HTMLInputElement)) throw new Error('Missing editor'); return value; }

afterEach(() => { vi.restoreAllMocks(); for (const item of owned.splice(0)) { item.lifetime.abort(); item.host.remove(); } });

test('10,000 items retain a window and selection patches only the affected rows', () => {
  const { tasks, list } = mount(10000), first = row(list, 't1');
  expect(list.rows.size).toBeLessThanOrEqual(23);
  list.render(); list.render(); expect(row(list, 't1')).toBe(first);
  tasks.choose('t1'); const selected = vi.spyOn(TaskRow.prototype, 'selected', 'set');
  tasks.choose('t2'); expect(selected).toHaveBeenCalledTimes(2);
  selected.mockClear(); tasks.choose('t2'); expect(selected).not.toHaveBeenCalled();
  tasks.choose('t9999'); expect(list.rows.has('t9999')).toBe(false);
  list.focusKey('t9999'); expect(row(list, 't9999').selected).toBe(true);
});

test('focused drafts stay pinned and keyboard destinations exist before focus', () => {
  const { list } = mount(); const first = row(list, 't1'), input = editor(first);
  input.focus(); input.value = 'Uncommitted draft';
  list.viewport.scrollTop = 100 * ROW_HEIGHT; list.render();
  expect(row(list, 't1')).toBe(first); expect(first.shadowRoot?.activeElement).toBe(input);
  expect(input.value).toBe('Uncommitted draft'); expect(list.rows.size).toBeLessThanOrEqual(24);
  first.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, composed: true }));
  expect(list.shadowRoot?.activeElement).toBe(row(list, 't2'));
  expect(row(list, 't2').isConnected).toBe(true);
});

test('insertion and removal above the viewport preserve the anchor', () => {
  const { tasks, list } = mount(); list.viewport.scrollTop = 100 * ROW_HEIGHT + 7; list.render();
  const anchor = row(list, 't100'), top = anchor.getBoundingClientRect().top;
  const inserted = tasks.insert('t0'); expect(Math.abs(anchor.getBoundingClientRect().top - top)).toBeLessThan(.5);
  tasks.remove(inserted); expect(Math.abs(anchor.getBoundingClientRect().top - top)).toBeLessThan(.5);
});

test('keyboard navigation scrolls a retained overscan destination into view', () => {
  const { list } = mount(); list.focusKey('t14');
  row(list, 't14').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, composed: true }));
  expect(list.shadowRoot?.activeElement).toBe(row(list, 't15'));
  const destination = row(list, 't15').getBoundingClientRect(), viewport = list.viewport.getBoundingClientRect();
  expect(destination.bottom).toBeLessThanOrEqual(viewport.bottom);
});

test('key reuse ends old bindings, replaces the node and applies focus fallback', () => {
  const { tasks, list } = mount(); const old = row(list, 't2');
  editor(old).focus(); tasks.choose('t2'); tasks.reuse('t2');
  const next = row(list, 't2'); expect(next).not.toBe(old);
  expect(list.shadowRoot?.activeElement).toBe(next); expect(next.selected).toBe(false);
  old.dispatchEvent(new CustomEvent('acme-task-rename', { detail: { text: 'Stale' } }));
  expect(tasks.items.get('t2')?.text).toBe('Replacement t2');
  tasks.rename('t2', 'Current'); expect(next.text).toBe('Current'); expect(old.text).toBe('Task 3');
});

test('empty removal focuses the list and duplicate membership leaves the view intact', () => {
  const { tasks, list } = mount(1), first = row(list, 't0');
  expect(() => list.membership(['t0', 't0'])).toThrow(TypeError); expect(row(list, 't0')).toBe(first);
  list.focusKey('t0'); tasks.remove('t0'); expect(document.activeElement).toBe(list);
  expect(list.rows.size).toBe(0);
});

test('teardown ends bindings and partial binding setup does not abort its caller', () => {
  const { tasks, list, lifetime, host } = mount(); const first = row(list, 't0'); lifetime.abort();
  expect(host.children).toHaveLength(0);
  const setter = vi.spyOn(TaskRow.prototype, 'text', 'set'); tasks.rename('t0', 'After'); expect(setter).not.toHaveBeenCalled();
  first.dispatchEvent(new CustomEvent('acme-task-select')); expect(tasks.state.get().selectedKey).toBeNull();
  const caller = new AbortController(), failure = new Error('setup');
  tasks.select.done = { fields: ['done'], read: () => { throw failure; } };
  expect(() => bindTaskRow(new TaskRow(), tasks, 't1', { signal: caller.signal, report: () => {} })).toThrow(failure);
  expect(caller.signal.aborted).toBe(false); setter.mockClear(); tasks.rename('t1', 'After failure'); expect(setter).not.toHaveBeenCalled();
});
