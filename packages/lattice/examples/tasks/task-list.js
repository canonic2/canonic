/** @file Hand-written fixed-height presentation, retained row lifetimes and focus ownership. */
import { defineOnce } from '@canonic2/lattice/elements';
import { TaskRow, defineTaskRow } from './task-row.js';

export const ROW_HEIGHT = 32;
const OVERSCAN = 4;
/** @type {CSSStyleSheet | null} */
let sheet = null;
/** Share list geometry rules. @returns {CSSStyleSheet} */
function styles() {
  if (sheet === null) {
    sheet = new CSSStyleSheet();
    sheet.replaceSync(':host{display:block}.viewport{height:480px;overflow:auto;overflow-anchor:none;border:1px solid #999}.content{position:relative}acme-task-row{position:absolute;left:0;right:0;height:32px}');
  }
  return sheet;
}

/**
 * An application element with plain membership and selection inputs.
 * Set `bindItem` before applying membership. State lives outside this element.
 */
export class TaskList extends HTMLElement {
  /** @type {Map<string, TaskRow>} Retained nodes; presentation-owned. */
  rows = new Map();
  /** @type {Map<string, AbortController>} One lifetime per retained incarnation. */
  lifetimes = new Map();
  /** @type {readonly string[]} Current validated membership. */
  keys = [];
  /** @type {((key: string, row: TaskRow, signal: AbortSignal) => void) | null} Stable binding port. */
  bindItem = null;
  /** @type {string | null} Projection of the selectedKey input. */
  selected = null;
  revision = 0;
  /** @type {{ revision: number, start: number, end: number, focus: string | null } | null} Completed reconciliation identity. */
  rendered = null;
  /** @type {AbortController | null} Native event connection lifetime. */
  connection = null;
  viewportHeight = 480;
  #windowed = true;
  /** Construct only the element's permanent presentation. */
  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' }); root.adoptedStyleSheets = [styles()];
    this.viewport = document.createElement('div'); this.viewport.className = 'viewport';
    this.content = document.createElement('div'); this.content.className = 'content';
    this.viewport.append(this.content); root.append(this.viewport);
  }
  /** Acquire native listeners and render current inputs when connected. @returns {void} */
  connectedCallback() {
    this.connection?.abort(); const own = new AbortController(); this.connection = own;
    this.tabIndex = 0; this.setAttribute('role', 'list'); this.setAttribute('aria-label', 'Tasks');
    this.viewport.addEventListener('scroll', () => this.render(), { signal: own.signal });
    this.viewportHeight = this.viewport.clientHeight;
    this.addEventListener('keydown', (event) => {
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
      event.preventDefault();
      const at = this.keys.indexOf(this.focusedKey() ?? '');
      const next = Math.max(0, Math.min(this.keys.length - 1, at + (event.key === 'ArrowDown' ? 1 : -1)));
      const key = this.keys[next]; if (key !== undefined) this.focusKey(key);
    }, { signal: own.signal });
    this.addEventListener('focusout', () => queueMicrotask(() => { if (this.isConnected) this.render(); }), { signal: own.signal });
    const observer = new ResizeObserver((entries) => {
      const height = entries[entries.length - 1]?.contentRect.height ?? this.viewportHeight;
      if (height !== this.viewportHeight) { this.viewportHeight = height; this.rendered = null; this.render(); }
    });
    observer.observe(this.viewport);
    own.signal.addEventListener('abort', () => observer.disconnect(), { once: true });
    this.rendered = null; this.render();
  }
  /** Release rows and connection resources when removed. @returns {void} */
  disconnectedCallback() { this.dispose(); }
  /** @returns {boolean} Whether only the retained window is mounted. */
  get windowed() { return this.#windowed; }
  /** @param {boolean} value Mount a window or all rows, preserving keyed identity. */
  set windowed(value) { if (value !== this.#windowed) { this.#windowed = value; this.rendered = null; this.render(); } }
  /** @param {string | null} key Patch only the old and new retained rows. */
  set selectedKey(key) {
    if (key === this.selected) return;
    const old = this.rows.get(this.selected ?? ''); this.selected = key;
    if (old) old.selected = false;
    const next = this.rows.get(key ?? ''); if (next) next.selected = true;
  }
  /** @returns {string | null} Focus inside a retained row's host or shadow tree. */
  focusedKey() {
    const active = this.shadowRoot?.activeElement;
    return active instanceof TaskRow && active.parentElement === this.content ? active.dataset.key ?? null : null;
  }
  /** @returns {string | undefined} Item anchoring the viewport's top edge. */
  get firstVisibleKey() { return this.keys[Math.floor(this.viewport.scrollTop / ROW_HEIGHT)]; }
  /**
   * Apply committed membership, including ended incarnations with reused keys.
   * @param {readonly string[]} keys Unique keys in order.
   * @param {import('@canonic2/lattice/collection').KeysChange} [change] Commit incarnations.
   * @returns {void}
   */
  membership(keys, change) {
    /** @type {Set<string>} */
    const unique = new Set();
    for (let index = 0; index < keys.length; index++) {
      const key = /** @type {string} */ (keys[index]); if (unique.has(key)) throw new TypeError('Duplicate row key'); unique.add(key);
    }
    const focused = this.focusedKey(), oldIndex = this.keys.indexOf(focused ?? '');
    const top = Math.floor(this.viewport.scrollTop / ROW_HEIGHT), anchor = this.keys[top];
    const offset = this.viewport.scrollTop % ROW_HEIGHT;
    if (change) for (const key of change.removed) {
      const row = this.rows.get(key); if (row) this.endRow(key, row);
    }
    this.keys = keys; this.revision++;
    this.content.style.height = `${keys.length * ROW_HEIGHT}px`;
    if (anchor !== undefined) {
      const next = keys.indexOf(anchor);
      this.viewport.scrollTop = Math.max(0, next < 0 ? Math.min(top, keys.length - 1) : next) * ROW_HEIGHT + offset;
    }
    this.render();
    if (focused !== null && (change?.removed.includes(focused) || !keys.includes(focused))) {
      const fallback = keys[Math.min(oldIndex, keys.length - 1)];
      if (fallback === undefined) this.focus({ preventScroll: true }); else this.focusKey(fallback);
    }
  }
  /** Reconcile only a changed range, retaining at most one focused extra row. @returns {void} */
  render() {
    if (!this.isConnected) return;
    const top = Math.floor(this.viewport.scrollTop / ROW_HEIGHT);
    const start = this.#windowed ? Math.max(0, top - OVERSCAN) : 0;
    const end = this.#windowed ? Math.min(this.keys.length, top + Math.ceil(this.viewportHeight / ROW_HEIGHT) + OVERSCAN) : this.keys.length;
    const focus = this.focusedKey(), old = this.rendered;
    if (old && old.revision === this.revision && old.start === start && old.end === end && old.focus === focus) return;
    const desired = this.keys.slice(start, end), positions = new Map(desired.map((key, index) => [key, start + index]));
    if (focus !== null && !positions.has(focus)) {
      const index = this.keys.indexOf(focus);
      if (index >= 0) { if (index < start) desired.unshift(focus); else desired.push(focus); positions.set(focus, index); }
    }
    const retained = new Set(desired);
    for (const [key, row] of this.rows) if (!retained.has(key)) this.endRow(key, row);
    /** @type {TaskRow | null} */
    let previous = null;
    for (let index = 0; index < desired.length; index++) {
      const key = /** @type {string} */ (desired[index]);
      let row = this.rows.get(key);
      if (row === undefined) {
        const own = new AbortController();
        try { row = this.createRow(key, own.signal); }
        catch (error) { own.abort(); throw error; }
        this.rows.set(key, row); this.lifetimes.set(key, own);
      }
      this.placeRow(key, row, /** @type {number} */ (positions.get(key)), previous);
      previous = row;
    }
    this.rendered = { revision: this.revision, start, end, focus };
  }
  /**
   * Construct presentation and acquire an incarnation's bindings before insertion.
   * @param {string} key Entering item.
   * @param {AbortSignal} signal The retained incarnation's lifetime.
   * @returns {TaskRow} New presentation; never recycled.
   */
  createRow(key, signal) {
    const row = new TaskRow();
    row.dataset.key = key; row.tabIndex = -1; row.setAttribute('role', 'listitem'); row.selected = key === this.selected;
    this.bindItem?.(key, row, signal); return row;
  }
  /**
   * Position one retained row without disconnecting a moved row.
   * @param {string} _key Item identity.
   * @param {TaskRow} row Retained presentation.
   * @param {number} index Full collection index.
   * @param {TaskRow | null} previous Previous retained row, or null for the first.
   * @returns {void}
   */
  placeRow(_key, row, index, previous) {
    const before = previous === null ? this.content.firstElementChild : previous.nextElementSibling;
    if (before !== row) {
      if (row.isConnected) this.content.moveBefore(row, before); else this.content.insertBefore(row, before);
    }
    const top = `${index * ROW_HEIGHT}px`; if (row.style.top !== top) row.style.top = top;
  }
  /**
   * Remove presentation after its lifetime has ended.
   * @param {string} _key Leaving incarnation.
   * @param {TaskRow} row Leaving presentation.
   * @returns {void}
   */
  releaseRow(_key, row) { row.remove(); }
  /**
   * End one retained incarnation before invoking presentation cleanup.
   * @param {string} key Leaving item.
   * @param {TaskRow} row Retained presentation.
   * @returns {void}
   */
  endRow(key, row) {
    this.rows.delete(key);
    const own = this.lifetimes.get(key); this.lifetimes.delete(key);
    own?.abort(); this.releaseRow(key, row);
  }
  /**
   * Ensure a keyboard destination exists before focusing it.
   * @param {string} key Existing item.
   * @returns {void}
   */
  focusKey(key) {
    const index = this.keys.indexOf(key); if (index < 0) return;
    const first = Math.floor(this.viewport.scrollTop / ROW_HEIGHT);
    const visible = Math.max(1, Math.floor(this.viewportHeight / ROW_HEIGHT));
    if (index < first) this.viewport.scrollTop = index * ROW_HEIGHT;
    else if (index >= first + visible) this.viewport.scrollTop = (index + 1) * ROW_HEIGHT - this.viewportHeight;
    this.render();
    this.rows.get(key)?.focus({ preventScroll: true }); this.render();
  }
  /** End all owned resources; the outer binding lifetime is owned by composition. @returns {void} */
  dispose() {
    this.connection?.abort(); this.connection = null;
    for (const [key, row] of this.rows) this.endRow(key, row);
    this.rows.clear(); this.lifetimes.clear(); this.rendered = null;
  }
}

/** Register the example's presentation elements. @returns {void} */
export function defineTaskList() { defineTaskRow(); defineOnce(customElements, 'acme-task-list', TaskList); }
