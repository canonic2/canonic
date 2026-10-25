/** @file Fixed-height list presentation: plain inputs and observed scroll/focus intent. */
import { captureInputs, defineOnce, replayInputs } from '@canonic2/lattice/elements';
import { TaskRow, defineTaskRow } from './task-row.js';

export const ROW_HEIGHT = 32;
const OVERSCAN = 4;
/** @typedef {{ key: string, row: TaskRow, index: number }} PresentedItem A retained presentation and its full-list index. */
/** @type {CSSStyleSheet | null} */
let sheet = null;
/** Share fixed-height presentation rules. @returns {CSSStyleSheet} */
function styles() {
  if (sheet === null) {
    sheet = new CSSStyleSheet();
    sheet.replaceSync(':host{display:block}.viewport{height:480px;overflow:auto;overflow-anchor:none;border:1px solid #999}.content{position:relative}acme-task-row{position:absolute;left:0;right:0;height:32px}');
  }
  return sheet;
}

/**
 * Render supplied rows without creating bindings or calling application ports.
 * `acme-task-range` requests retention after observed scrolling/focus departure;
 * `acme-task-focus` carries a keyboard destination as `{ key }`. Both bubble and
 * are composed. Input assignments and connection callbacks dispatch no event.
 */
export class TaskList extends HTMLElement {
  /** @type {Map<string, TaskRow>} Presentation lookup; never owns subscriptions. */
  rows = new Map();
  /** @type {AbortController | null} Native connection resources. */
  connection = null;
  viewportHeight = 480;
  /** @type {readonly string[]} */
  #keys = [];
  /** @type {readonly PresentedItem[]} */
  #items = [];
  /** @type {string | null} */
  #selected = null;
  /** @type {string | null} Focus retained across a temporary detach. */
  #focus = null;
  #windowed = true;
  #disposed = false;
  /** @type {Map<'items' | 'selectedKey', unknown> | null} */
  #pending = null;

  /** Build permanent nodes and capture inputs without traversing supplied rows. */
  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' }); root.adoptedStyleSheets = [styles()];
    this.viewport = document.createElement('div'); this.viewport.className = 'viewport';
    this.content = document.createElement('div'); this.content.className = 'content';
    this.viewport.append(this.content); root.append(this.viewport);
    const early = captureInputs(this, ['keys', 'windowed']);
    if (early !== null) replayInputs(this, early);
    const pending = captureInputs(this, ['items', 'selectedKey']);
    if (pending !== null) {
      this.#pending = pending;
      queueMicrotask(() => { replayInputs(this, pending); this.#pending = null; });
    }
  }

  /** Acquire connection resources with rollback; retained inputs/nodes survive. @returns {void} */
  connectedCallback() {
    if (!this.isConnected || this.#disposed) return;
    this.disconnect(); const own = new AbortController(); this.connection = own;
    try {
      this.tabIndex = 0; this.setAttribute('role', 'list'); this.setAttribute('aria-label', 'Tasks');
      this.viewport.addEventListener('scroll', () => this.#intent('acme-task-range'), { signal: own.signal });
      this.addEventListener('keydown', (event) => {
        if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
        event.preventDefault();
        const at = this.#keys.indexOf(this.focusedKey() ?? '');
        const next = Math.max(0, Math.min(this.#keys.length - 1, at + (event.key === 'ArrowDown' ? 1 : -1)));
        const key = this.#keys[next]; if (key !== undefined) this.#intent('acme-task-focus', key);
      }, { signal: own.signal });
      this.addEventListener('focusin', () => { this.#focus = this.#activeKey(); }, { signal: own.signal });
      this.addEventListener('focusout', () => queueMicrotask(() => {
        if (own.signal.aborted) return;
        this.#focus = this.#activeKey(); this.#intent('acme-task-range');
      }), { signal: own.signal });
      const observer = new ResizeObserver((entries) => {
        if (own.signal.aborted) return;
        const height = entries[entries.length - 1]?.contentRect.height ?? this.viewportHeight;
        if (height !== this.viewportHeight) { this.viewportHeight = height; this.render(); }
      });
      own.signal.addEventListener('abort', () => observer.disconnect(), { once: true });
      observer.observe(this.viewport); this.viewportHeight = this.viewport.clientHeight;
      this.render();
    } catch (error) { this.disconnect(); throw error; }
  }
  /** End connection resources without discarding inputs, rows or drafts. @returns {void} */
  disconnect() { const own = this.connection; this.connection = null; own?.abort(); }
  /** Temporary disconnection is not final view disposal. @returns {void} */
  disconnectedCallback() { this.disconnect(); }
  /** Atomic moves preserve connection resources and focus. @returns {void} */
  connectedMoveCallback() {}

  /** @returns {readonly string[]} Current membership, including a pending upgrade input. */
  get keys() { return this.#keys; }
  /** @param {readonly string[]} value Immutable membership; preserves the viewport anchor. */
  set keys(value) {
    if (this.#disposed || value === this.#keys) return;
    const unique = new Set();
    for (let index = 0; index < value.length; index++) {
      const key = value[index]; if (typeof key !== 'string' || unique.has(key)) throw new TypeError('Invalid or duplicate row key'); unique.add(key);
    }
    const top = Math.floor(this.viewport.scrollTop / ROW_HEIGHT), anchor = this.#keys[top], offset = this.viewport.scrollTop % ROW_HEIGHT;
    this.#keys = value; this.content.style.height = `${value.length * ROW_HEIGHT}px`;
    if (anchor !== undefined) {
      const next = value.indexOf(anchor);
      this.viewport.scrollTop = Math.max(0, next < 0 ? Math.min(top, value.length - 1) : next) * ROW_HEIGHT + offset;
    }
  }
  /** @returns {readonly PresentedItem[]} Supplied presentations, borrowed without mutation. */
  get items() { return this.#pending?.has('items') ? /** @type {readonly PresentedItem[]} */ (this.#pending.get('items')) : this.#items; }
  /** @param {readonly PresentedItem[]} value New immutable retained-row projection. */
  set items(value) { this.#pending?.delete('items'); if (this.#disposed || value === this.#items) return; this.#items = value; this.render(); }
  /** @returns {boolean} Whether geometry requests a bounded window. */
  get windowed() { return this.#windowed; }
  /** @param {boolean} value Presentation mode; composition reconciles after changing it. */
  set windowed(value) { this.#windowed = value; }
  /** @returns {string | null} Selected key input. */
  get selectedKey() { return this.#pending?.has('selectedKey') ? /** @type {string | null} */ (this.#pending.get('selectedKey')) : this.#selected; }
  /** @param {string | null} key Patch only the old and new retained rows. */
  set selectedKey(key) {
    this.#pending?.delete('selectedKey');
    if (key === this.#selected || this.#disposed) return;
    const old = this.rows.get(this.#selected ?? ''); this.#selected = key;
    if (old) old.selected = false;
    const next = this.rows.get(key ?? ''); if (next) next.selected = true;
  }
  /** @returns {string | null} Active row key, without a retained pin fallback. */
  #activeKey() { const active = this.shadowRoot?.activeElement; return active instanceof TaskRow && active.parentElement === this.content ? active.dataset.key ?? null : null; }
  /** @returns {string | null} Focused row, including one preserved across detach. */
  focusedKey() { return this.#activeKey() ?? this.#focus; }
  /** @returns {string | undefined} Item at the viewport's top edge. */
  get firstVisibleKey() { return this.#keys[Math.floor(this.viewport.scrollTop / ROW_HEIGHT)]; }
  /** @returns {{ start: number, end: number, focus: string | null }} Requested range from current geometry, independent of observer delivery order. */
  range() {
    this.viewportHeight = this.viewport.clientHeight;
    const top = Math.floor(this.viewport.scrollTop / ROW_HEIGHT);
    return { start: this.#windowed ? Math.max(0, top - OVERSCAN) : 0,
      end: this.#windowed ? Math.min(this.#keys.length, top + Math.ceil(this.viewportHeight / ROW_HEIGHT) + OVERSCAN) : this.#keys.length,
      focus: this.focusedKey() };
  }
  /** Patch supplied presentation only; never creates rows or calls application code. @returns {void} */
  render() {
    if (this.#disposed) return;
    const desired = new Map(this.#items.map(item => [item.key, item.row]));
    for (const [key, row] of this.rows) if (desired.get(key) !== row) { this.rows.delete(key); row.remove(); }
    /** @type {TaskRow | null} */
    let previous = null;
    for (const { key, row, index } of this.#items) {
      if (this.rows.get(key) !== row) { row.selected = key === this.#selected; this.rows.set(key, row); }
      const before = previous === null ? this.content.firstElementChild : previous.nextElementSibling;
      if (before !== row) { if (row.isConnected && this.content.isConnected) this.content.moveBefore(row, before); else this.content.insertBefore(row, before); }
      const top = `${index * ROW_HEIGHT}px`; if (row.style.top !== top) row.style.top = top;
      previous = row;
    }
  }
  /** Make a destination visible; composition mounts it before `focusRow`. @param {string} key Destination. @returns {void} */
  scrollToKey(key) {
    const index = this.#keys.indexOf(key); if (index < 0) return;
    this.viewportHeight = this.viewport.clientHeight;
    const first = Math.floor(this.viewport.scrollTop / ROW_HEIGHT), visible = Math.max(1, Math.floor(this.viewportHeight / ROW_HEIGHT));
    if (index < first) this.viewport.scrollTop = index * ROW_HEIGHT;
    else if (index >= first + visible) this.viewport.scrollTop = (index + 1) * ROW_HEIGHT - this.viewportHeight;
  }
  /** Focus supplied presentation or the empty list, without acquiring items. @param {string | null} key Retained destination, or null. @returns {void} */
  focusRow(key) { this.#focus = key; if (key === null) this.focus({ preventScroll: true }); else this.rows.get(key)?.focus({ preventScroll: true }); }
  /** Dispatch only observed user intent while connected. @param {string} name Event name. @param {string} [key] Keyboard destination. @returns {void} */
  #intent(name, key) { if (this.isConnected && !this.#disposed) this.dispatchEvent(new CustomEvent(name, { bubbles: true, composed: true, detail: { key } })); }
  /** Final presentation disposal; composition separately ends row bindings first. @returns {void} */
  dispose() {
    if (this.#disposed) return;
    this.#disposed = true; this.#pending?.clear(); this.disconnect();
    this.#items = []; this.#keys = []; this.#focus = null; this.rows.clear(); this.content.replaceChildren(); this.content.style.height = '0px';
  }
}

/** Register presentation classes explicitly. @returns {void} */
export function defineTaskList() { defineTaskRow(); defineOnce(customElements, 'acme-task-list', TaskList); }
