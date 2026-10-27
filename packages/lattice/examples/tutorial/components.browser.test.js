/** @file Exercises the guide's actual native elements, including upgrade and intent. */
import { afterEach, expect, test, vi } from 'vitest';
import { TaskToggle, defineTaskToggle } from './task-toggle.js';
import { TaskLabel, defineTaskLabel } from './task-label.js';

defineTaskToggle();
defineTaskLabel();
/** @type {HTMLElement[]} */
const nodes = [];
let nextTag = 0;

/**
 * Connects a node owned by this test.
 * @template {HTMLElement} T
 * @param {T} node The node to connect.
 * @returns {T} The connected node.
 */
function connect(node) {
  nodes.push(node);
  document.body.append(node);
  return node;
}

/**
 * Finds the real checkbox for observable control assertions.
 * @param {TaskToggle} toggle The tutorial element.
 * @returns {HTMLInputElement} Its retained checkbox.
 */
function checkbox(toggle) {
  const input = toggle.shadowRoot?.querySelector('input');
  if (!input) throw new Error('Missing checkbox.');
  return input;
}

/**
 * Upgrades a disconnected label whose property predates registration.
 * @param {string} text The early property value.
 * @param {string} attribute The attribute present at upgrade.
 * @returns {TaskLabel} The upgraded label before its replay microtask.
 */
function earlyLabel(text, attribute) {
  const tag = `acme-early-label-${nextTag++}`;
  const node = document.createElement(tag);
  nodes.push(node);
  node.setAttribute('text', attribute);
  Reflect.set(node, 'text', text);
  /** A unique registry constructor for this upgrade fixture. */
  class EarlyLabel extends TaskLabel {}
  customElements.define(tag, EarlyLabel);
  customElements.upgrade(node);
  if (!(node instanceof TaskLabel)) throw new Error('Expected label upgrade.');
  return node;
}

afterEach(() => {
  for (const node of nodes.splice(0)) node.remove();
  vi.restoreAllMocks();
});

test('assignments update the live control without emitting intent', () => {
  const toggle = connect(new TaskToggle());
  const intent = vi.fn();
  toggle.addEventListener('acme-task-toggle', intent);
  toggle.done = true;
  expect(checkbox(toggle).checked).toBe(true);
  checkbox(toggle).checked = false;
  toggle.done = true;
  expect(checkbox(toggle).checked).toBe(true);
  expect(intent).not.toHaveBeenCalled();
});

test('only connected user changes emit one bubbling, composed intent after reconnect', () => {
  const toggle = connect(new TaskToggle());
  const intent = vi.fn();
  toggle.addEventListener('acme-task-toggle', intent);
  checkbox(toggle).checked = true;
  checkbox(toggle).dispatchEvent(new Event('change'));
  expect(intent).toHaveBeenCalledTimes(1);
  const event = intent.mock.calls[0]?.[0];
  expect([event.detail.done, event.bubbles, event.composed]).toEqual([true, true, true]);
  expect(toggle.done).toBe(false);
  toggle.remove();
  checkbox(toggle).dispatchEvent(new Event('change'));
  expect(intent).toHaveBeenCalledTimes(1);
  document.body.append(toggle);
  checkbox(toggle).dispatchEvent(new Event('change'));
  expect(intent).toHaveBeenCalledTimes(2);
});

test('a pre-upgrade toggle property reaches the setter and remains writable', () => {
  const tag = `acme-early-toggle-${nextTag++}`;
  const node = document.createElement(tag);
  nodes.push(node);
  Reflect.set(node, 'done', true);
  /** An independently registered toggle for the upgrade fixture. */
  class EarlyToggle extends TaskToggle {}
  customElements.define(tag, EarlyToggle);
  customElements.upgrade(node);
  if (!(node instanceof TaskToggle)) throw new Error('Expected toggle upgrade.');
  expect(Object.hasOwn(node, 'done')).toBe(false);
  expect(node.done).toBe(true);
  expect(checkbox(node).checked).toBe(true);
  node.done = false;
  expect(checkbox(node).checked).toBe(false);
});

test('reflected inputs update text and attributes with silent guarded echoes', () => {
  const label = connect(new TaskLabel());
  label.text = 'Complete';
  expect(label.shadowRoot?.textContent).toBe('Complete');
  expect(label.getAttribute('text')).toBe('Complete');
  label.setAttribute('text', 'In progress');
  expect(label.text).toBe('In progress');
  expect(label.shadowRoot?.textContent).toBe('In progress');
  label.removeAttribute('text');
  expect(label.text).toBe('');
  expect(label.shadowRoot?.textContent).toBe('');
});

test('early reflected property wins over the initial attribute and is readable before replay', async () => {
  const label = earlyLabel('early', 'attribute');
  expect(label.text).toBe('early');
  await Promise.resolve();
  expect(label.text).toBe('early');
  expect(label.shadowRoot?.textContent).toBe('early');
  expect(label.getAttribute('text')).toBe('early');
});

test('a newer property, including the default, supersedes pending replay', async () => {
  const label = earlyLabel('early', 'attribute');
  label.text = '';
  await Promise.resolve();
  expect(label.text).toBe('');
  expect(label.getAttribute('text')).toBe('');
  expect(label.shadowRoot?.textContent).toBe('');
});

test('a newer attribute supersedes pending input and a captured default corrects reflection', async () => {
  const newer = earlyLabel('early', 'attribute');
  newer.setAttribute('text', 'newer');
  const empty = earlyLabel('', 'attribute');
  await Promise.resolve();
  expect(newer.text).toBe('newer');
  expect(newer.shadowRoot?.textContent).toBe('newer');
  expect(empty.text).toBe('');
  expect(empty.getAttribute('text')).toBe('');
});
