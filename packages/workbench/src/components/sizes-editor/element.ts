/* <wb-sizes-editor>: Edit sizes…
   -------------------------------
   A modal list of the space's sizes, in order. Each row edits its name,
   width and height (a number or Fill; Fit and Resizable have none), icon,
   and whether it is a button, and can move up, down, or be removed — all but
   the last. Rows workbench.local.yaml sets are shown read-only. The draft is
   the dialog's own until Save asks with wb-sizes-save; the controller writes
   workbench.yaml, sets `pending`, and closes the dialog or sets `error`.
   Cancel, Escape, or the backdrop ask with wb-sizes-close. See
   specs/sizes.md. */

import type { ResolvedSize } from '../../sizes/browser/size.ts';
import type { IconRenderer } from '../space-mark/space.ts';
import { dialogStyles } from '../size-dialog/styles.ts';
import { draftOf, rowsProblem, rowValue } from './rows.ts';
import type { SizeRowDraft, SizeRowValue } from './rows.ts';

export interface SizesSave { sizes: SizeRowValue[] }

declare global {
  interface HTMLElementTagNameMap { 'wb-sizes-editor': WbSizesEditor }
  interface HTMLElementEventMap {
    'wb-sizes-save': CustomEvent<SizesSave>;
    'wb-sizes-close': CustomEvent<Record<string, never>>;
  }
}

const INPUTS = ['sizes', 'open', 'pending', 'error', 'iconRenderer'] as const;

const editorStyles = `
:host { --wb-dialog-width: 840px; }
.rows { display:flex; flex-direction:column; gap:4px; margin:0; padding:0; list-style:none; max-height:min(420px,calc(100vh - 260px)); overflow:auto; }
.size { display:grid; grid-template-columns:30px minmax(100px,1.3fr) minmax(140px,1fr) minmax(140px,1fr) minmax(100px,.9fr) 52px 86px;
  align-items:center; gap:8px; padding:6px; border-radius:8px; }
.size:hover { background:var(--wb-hover); }
.size.local { color:var(--wb-fg-2); }
.head { color:var(--wb-fg-3); font-size:11px; font-weight:600; letter-spacing:.06em; text-transform:uppercase; }
.head:hover { background:none; }
.none { color:var(--wb-fg-3); font-size:12px; }
.tools { display:flex; gap:2px; }
.tool { display:grid; place-items:center; width:26px; height:26px; padding:0; color:var(--wb-fg-2); background:none; border:0; border-radius:6px; }
.tool:hover:not(:disabled) { color:var(--wb-fg); background:var(--wb-raised); }
.note { grid-column:2 / -1; margin:-2px 0 0; color:var(--wb-fg-3); font-size:12px; }
`;

function input(type: string, label: string): HTMLInputElement {
  const node = document.createElement('input');
  node.type = type;
  node.setAttribute('aria-label', label);
  return node;
}

export class WbSizesEditor extends HTMLElement {
  static observedAttributes = ['pending'];
  #sizes: ResolvedSize[] = [];
  #rows: SizeRowDraft[] = [];
  #open = false;
  #error = '';
  #icons: IconRenderer | undefined;
  #connection: AbortController | undefined;
  #root: ShadowRoot;
  #dialog: HTMLDialogElement;
  #list: HTMLOListElement;
  #problem: HTMLParagraphElement;
  #save: HTMLButtonElement;
  #cancel: HTMLButtonElement;

  constructor() {
    super();
    this.#root = this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(dialogStyles + editorStyles);
    this.#root.adoptedStyleSheets = [sheet];
    this.#dialog = document.createElement('dialog');
    this.#dialog.setAttribute('aria-labelledby', 'title');
    this.#dialog.setAttribute('part', 'dialog');
    const form = document.createElement('form');
    form.method = 'dialog';
    form.noValidate = true;
    const title = document.createElement('h2');
    title.id = 'title';
    title.textContent = 'Edit sizes';
    const hint = document.createElement('p');
    hint.className = 'hint';
    hint.textContent = 'The order is the size switcher’s. Removing a size also removes it from every page that lists it.';
    const head = document.createElement('div');
    head.className = 'size head';
    head.setAttribute('aria-hidden', 'true');
    for (const text of ['', 'Name', 'Width', 'Height', 'Icon', 'Button', '']) head.append(Object.assign(document.createElement('span'), { textContent: text }));
    this.#list = document.createElement('ol');
    this.#list.className = 'rows';
    this.#list.setAttribute('aria-label', 'Sizes');
    this.#problem = document.createElement('p'); this.#problem.className = 'problem'; this.#problem.setAttribute('role', 'status');
    this.#cancel = document.createElement('button'); this.#cancel.type = 'button'; this.#cancel.className = 'secondary'; this.#cancel.textContent = 'Cancel';
    this.#save = document.createElement('button'); this.#save.type = 'submit'; this.#save.className = 'primary'; this.#save.textContent = 'Save';
    const actions = document.createElement('div'); actions.className = 'actions';
    actions.append(this.#cancel, this.#save);
    form.append(title, hint, head, this.#list, this.#problem, actions);
    this.#dialog.append(form);
    this.#root.append(this.#dialog);
  }

  connectedCallback() {
    for (const key of INPUTS) {
      if (Object.hasOwn(this, key)) {
        const value = this[key];
        delete (this as unknown as Record<string, unknown>)[key];
        Reflect.set(this, key, value);
      }
    }
    this.#connection?.abort();
    this.#connection = new AbortController();
    const options = { signal: this.#connection.signal };
    this.#root.addEventListener('input', this.#typed, options);
    this.#root.addEventListener('change', this.#typed, options);
    this.#list.addEventListener('click', this.#tool, options);
    this.#dialog.addEventListener('submit', this.#submit, options);
    this.#dialog.addEventListener('cancel', this.#dismiss, options);
    this.#dialog.addEventListener('click', this.#backdrop, options);
    this.#cancel.addEventListener('click', this.#dismiss, options);
    this.#renderRows();
    this.#sync();
  }

  disconnectedCallback() {
    this.#connection?.abort();
    this.#connection = undefined;
    if (this.#dialog.open) this.#dialog.close();
  }

  attributeChangedCallback() { this.#sync(); }

  /** The space's sizes, in order, as the server resolves them. Copied on assignment; read again when the dialog opens. */
  get sizes(): ResolvedSize[] { return this.#sizes.map(size => ({ ...size })); }
  set sizes(value: readonly ResolvedSize[]) {
    this.#sizes = (value || []).map(size => ({ ...size }));
    if (!this.#open) { this.#rows = this.#sizes.map(draftOf); this.#renderRows(); }
  }
  /** Shows the dialog, starting from `sizes` each time it opens. */
  get open() { return this.#open; }
  set open(value: boolean) {
    const next = !!value;
    if (next && !this.#open) { this.#rows = this.#sizes.map(draftOf); this.#error = ''; this.#renderRows(); }
    this.#open = next;
    this.#sync();
  }
  get pending() { return this.hasAttribute('pending'); }
  set pending(value: boolean) { this.toggleAttribute('pending', !!value); }
  /** Why the controller couldn't save; cleared when a row changes. */
  get error() { return this.#error; }
  set error(value: string) { this.#error = value || ''; this.#sync(); }
  get iconRenderer() { return this.#icons; }
  set iconRenderer(value: IconRenderer | undefined) { this.#icons = value; this.#renderRows(); }
  /** The rows as they stand. */
  get draft(): SizeRowDraft[] { return this.#rows.map(row => ({ ...row })); }

  #glyph(name: string, fallback: string, size = 14): Node {
    if (this.#icons && /^[a-z0-9-]+$/.test(name)) return this.#icons(name, size);
    return document.createTextNode(fallback);
  }

  #toolButton(action: string, icon: string, fallback: string, label: string, disabled: boolean): HTMLButtonElement {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'tool';
    button.dataset.action = action;
    button.setAttribute('aria-label', label);
    button.title = label;
    button.disabled = disabled || this.pending;
    button.append(this.#glyph(icon, fallback));
    return button;
  }

  #length(row: SizeRowDraft, axis: 'width' | 'height'): HTMLElement {
    const box = document.createElement('div');
    if (row.kind !== 'fixed') {
      box.className = 'none';
      box.textContent = row.kind === 'fit' ? 'Fills the canvas' : 'Dragged to any size';
      return box;
    }
    box.className = 'length';
    const fill = axis === 'width' ? row.widthFill : row.heightFill;
    const field = input('number', `${axis === 'width' ? 'Width' : 'Height'} of ${row.name || row.key}`);
    field.dataset.field = axis; field.min = '1'; field.max = '8192'; field.step = '1';
    field.value = axis === 'width' ? row.width : row.height;
    field.disabled = row.local || fill;
    const check = input('checkbox', `${axis === 'width' ? 'Width' : 'Height'} of ${row.name || row.key} fills the canvas`);
    check.dataset.field = axis + 'Fill';
    check.checked = fill;
    check.disabled = row.local;
    const label = document.createElement('label'); label.className = 'inline';
    label.append(check, document.createTextNode('Fill'));
    box.append(field, label);
    return box;
  }

  /* Rows are rebuilt when they move or go; typing only updates the draft. */
  #renderRows(focus?: { key: string; action: string }) {
    const last = this.#rows.length - 1;
    const items = this.#rows.map((row, at) => {
      const item = document.createElement('li');
      item.className = 'size' + (row.local ? ' local' : '');
      item.dataset.key = row.key;
      const preview = document.createElement('span'); preview.className = 'preview'; preview.setAttribute('aria-hidden', 'true');
      preview.append(this.#glyph(row.icon.trim() || 'frame', '', 16));
      const name = input('text', `Name of ${row.key}`);
      name.dataset.field = 'name'; name.value = row.name; name.disabled = row.local;
      const icon = input('text', `Icon of ${row.name || row.key}`);
      icon.dataset.field = 'icon'; icon.value = row.icon; icon.disabled = row.local; icon.placeholder = 'frame';
      const button = input('checkbox', `Show ${row.name || row.key} as a button`);
      button.dataset.field = 'button'; button.checked = row.button; button.disabled = row.local;
      const tools = document.createElement('div'); tools.className = 'tools';
      const label = row.name || row.key;
      tools.append(
        this.#toolButton('up', 'arrow-up', '↑', `Move ${label} up`, at === 0),
        this.#toolButton('down', 'arrow-down', '↓', `Move ${label} down`, at === last),
        this.#toolButton('remove', 'trash-2', '×', `Remove ${label}`, row.local || this.#rows.length === 1),
      );
      item.append(preview, name, this.#length(row, 'width'), this.#length(row, 'height'), icon, button, tools);
      if (row.local) {
        const note = document.createElement('p'); note.className = 'note';
        note.textContent = 'Set in workbench.local.yaml; change it there.';
        item.append(note);
      }
      return item;
    });
    this.#list.replaceChildren(...items);
    if (focus) {
      const row = this.#list.querySelector<HTMLElement>(`[data-key="${CSS.escape(focus.key)}"]`);
      const target = row?.querySelector<HTMLButtonElement>(`[data-action="${focus.action}"]:not(:disabled)`)
        || row?.querySelector<HTMLButtonElement>('[data-action]:not(:disabled)') || this.#list.querySelector<HTMLElement>('input:not(:disabled)');
      target?.focus();
    }
  }

  #rowOf(node: Element): SizeRowDraft | undefined {
    const key = node.closest<HTMLElement>('.size')?.dataset.key;
    return this.#rows.find(row => row.key === key);
  }

  #typed = (event: Event) => {
    const target = event.target as HTMLInputElement;
    const field = target.dataset?.field;
    const row = field ? this.#rowOf(target) : undefined;
    if (!row || row.local) return;
    if (field === 'name' || field === 'icon' || field === 'width' || field === 'height') row[field] = target.value;
    else if (field === 'widthFill' || field === 'heightFill' || field === 'button') row[field] = target.checked;
    if (field === 'icon') target.closest('.size')?.querySelector('.preview')?.replaceChildren(this.#glyph(row.icon.trim() || 'frame', '', 16));
    if (field === 'widthFill' || field === 'heightFill') {
      const axis = field === 'widthFill' ? 'width' : 'height';
      const box = target.closest('.size')?.querySelector<HTMLInputElement>(`input[data-field="${axis}"]`);
      if (box) box.disabled = target.checked;
    }
    this.#error = '';
    this.#sync();
  };

  #tool = (event: Event) => {
    const button = (event.target as Element).closest<HTMLButtonElement>('button[data-action]');
    if (!button || button.disabled || this.pending) return;
    const row = this.#rowOf(button);
    if (!row) return;
    const at = this.#rows.indexOf(row);
    const action = button.dataset.action!;
    if (action === 'up' && at > 0) this.#rows.splice(at - 1, 0, ...this.#rows.splice(at, 1));
    else if (action === 'down' && at < this.#rows.length - 1) this.#rows.splice(at + 1, 0, ...this.#rows.splice(at, 1));
    else if (action === 'remove' && !row.local && this.#rows.length > 1) {
      this.#rows.splice(at, 1);
      const neighbour = this.#rows[Math.min(at, this.#rows.length - 1)];
      this.#error = '';
      this.#renderRows(neighbour ? { key: neighbour.key, action: 'remove' } : undefined);
      this.#sync();
      return;
    }
    this.#error = '';
    this.#renderRows({ key: row.key, action });
    this.#sync();
  };

  #dismiss = (event: Event) => {
    event.preventDefault();
    if (!this.pending) this.dispatchEvent(new CustomEvent('wb-sizes-close', { detail: {}, bubbles: true, composed: true }));
  };

  #backdrop = (event: Event) => { if (event.target === this.#dialog) this.#dismiss(event); };

  #submit = (event: Event) => {
    event.preventDefault();
    if (this.pending || rowsProblem(this.#rows)) return;
    this.dispatchEvent(new CustomEvent<SizesSave>('wb-sizes-save', { detail: { sizes: this.#rows.map(rowValue) }, bubbles: true, composed: true }));
  };

  #sync() {
    const pending = this.pending;
    const problem = rowsProblem(this.#rows);
    this.#save.disabled = pending || !!problem;
    this.#save.textContent = pending ? 'Saving…' : 'Save';
    this.#cancel.disabled = pending;
    this.#list.toggleAttribute('inert', pending);
    this.#dialog.setAttribute('aria-busy', String(pending));
    this.#problem.textContent = this.#error || problem || '';
    if (this.#open && !this.#dialog.open && this.isConnected) {
      this.#dialog.showModal();
      this.#list.querySelector<HTMLInputElement>('input:not(:disabled)')?.focus();
    }
    if (!this.#open && this.#dialog.open) this.#dialog.close();
  }
}
