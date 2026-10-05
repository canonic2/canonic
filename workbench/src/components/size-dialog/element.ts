/* <wb-size-dialog>: Custom size…
   ------------------------------
   A modal form for a new size: name, width and height (each a number or
   Fill), icon, whether it is a button, and whether it is for this page only.
   Add is enabled once the fields make a size; it asks with wb-size-add, and
   the controller writes workbench.yaml, sets `pending` while it does, and
   closes the dialog or sets `error`. Closing by Cancel, Escape, or the
   backdrop asks with wb-size-dialog-close. See specs/sizes.md. */

import type { IconRenderer } from '../space-mark/space.ts';
import { EMPTY_DRAFT, readDraft } from './form.ts';
import type { SizeDraft, SizeRequest } from './form.ts';
import { dialogStyles } from './styles.ts';

declare global {
  interface HTMLElementTagNameMap { 'wb-size-dialog': WbSizeDialog }
  interface HTMLElementEventMap {
    'wb-size-add': CustomEvent<SizeRequest>;
    'wb-size-dialog-close': CustomEvent<Record<string, never>>;
  }
}

const INPUTS = ['open', 'pending', 'error', 'notice', 'iconRenderer', 'allowPageOnly'] as const;

function input(type: string, name: string): HTMLInputElement {
  const node = document.createElement('input');
  node.type = type;
  node.name = name;
  return node;
}

function labelled(text: string, control: Node, className = ''): HTMLLabelElement {
  const label = document.createElement('label');
  if (className) label.className = className;
  if (className === 'check' || className === 'inline') label.append(control, document.createTextNode(text));
  else label.append(document.createTextNode(text), control);
  return label;
}

export class WbSizeDialog extends HTMLElement {
  static observedAttributes = ['pending', 'allow-page-only'];
  #open = false;
  #error = '';
  #notice = '';
  #icons: IconRenderer | undefined;
  #connection: AbortController | undefined;
  #root: ShadowRoot;
  #dialog: HTMLDialogElement;
  #fields: Record<'name' | 'width' | 'height' | 'icon', HTMLInputElement>;
  #checks: Record<'widthFill' | 'heightFill' | 'button' | 'pageOnly', HTMLInputElement>;
  #pageOnly: HTMLLabelElement;
  #preview: HTMLSpanElement;
  #problem: HTMLParagraphElement;
  #noticeText: HTMLParagraphElement;
  #add: HTMLButtonElement;
  #cancel: HTMLButtonElement;

  constructor() {
    super();
    this.#root = this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(dialogStyles);
    this.#root.adoptedStyleSheets = [sheet];
    this.#dialog = document.createElement('dialog');
    this.#dialog.setAttribute('aria-labelledby', 'title');
    this.#dialog.setAttribute('part', 'dialog');
    const form = document.createElement('form');
    form.method = 'dialog';
    form.noValidate = true;
    const title = document.createElement('h2');
    title.id = 'title';
    title.textContent = 'Custom size';
    this.#fields = { name: input('text', 'name'), width: input('number', 'width'), height: input('number', 'height'), icon: input('text', 'icon') };
    this.#checks = { widthFill: input('checkbox', 'widthFill'), heightFill: input('checkbox', 'heightFill'), button: input('checkbox', 'button'), pageOnly: input('checkbox', 'pageOnly') };
    this.#fields.name.autocomplete = 'off';
    this.#fields.name.placeholder = 'Sidebar';
    this.#fields.icon.placeholder = 'frame';
    this.#fields.icon.autocomplete = 'off';
    for (const field of [this.#fields.width, this.#fields.height]) { field.min = '1'; field.max = '8192'; field.step = '1'; field.inputMode = 'numeric'; }
    const length = (text: string, field: HTMLInputElement, fill: HTMLInputElement) => {
      const box = document.createElement('div'); box.className = 'length';
      box.append(field, labelled('Fill', fill, 'inline'));
      field.setAttribute('aria-label', text);
      const wrap = document.createElement('div'); wrap.className = 'field';
      const caption = document.createElement('span'); caption.textContent = text; caption.setAttribute('aria-hidden', 'true');
      wrap.append(caption, box);
      return wrap;
    };
    const dims = document.createElement('div'); dims.className = 'row';
    dims.append(length('Width', this.#fields.width, this.#checks.widthFill), length('Height', this.#fields.height, this.#checks.heightFill));
    this.#preview = document.createElement('span'); this.#preview.className = 'preview'; this.#preview.setAttribute('aria-hidden', 'true');
    const iconBox = document.createElement('div'); iconBox.className = 'icon-field';
    iconBox.append(this.#preview, this.#fields.icon);
    const iconLabel = labelled('Icon', iconBox);
    this.#pageOnly = labelled('Only for this page', this.#checks.pageOnly, 'check');
    this.#noticeText = document.createElement('p'); this.#noticeText.className = 'notice';
    this.#problem = document.createElement('p'); this.#problem.className = 'problem'; this.#problem.setAttribute('role', 'status');
    this.#cancel = document.createElement('button'); this.#cancel.type = 'button'; this.#cancel.className = 'secondary'; this.#cancel.textContent = 'Cancel';
    this.#add = document.createElement('button'); this.#add.type = 'submit'; this.#add.className = 'primary'; this.#add.textContent = 'Add';
    const actions = document.createElement('div'); actions.className = 'actions';
    actions.append(this.#cancel, this.#add);
    form.append(title, labelled('Name', this.#fields.name), dims, iconLabel,
      labelled('Show as a button', this.#checks.button, 'check'), this.#pageOnly, this.#noticeText, this.#problem, actions);
    this.#dialog.append(form);
    this.#root.append(this.#dialog);
    this.#reset();
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
    this.#root.addEventListener('input', this.#changed, options);
    this.#root.addEventListener('change', this.#changed, options);
    this.#dialog.addEventListener('submit', this.#submit, options);
    this.#dialog.addEventListener('cancel', this.#dismiss, options);
    this.#dialog.addEventListener('click', this.#backdrop, options);
    this.#cancel.addEventListener('click', this.#dismiss, options);
    this.#sync();
  }

  disconnectedCallback() {
    this.#connection?.abort();
    this.#connection = undefined;
    if (this.#dialog.open) this.#dialog.close();
  }

  attributeChangedCallback() { this.#sync(); }

  /** Shows the dialog, with empty fields each time it opens. */
  get open() { return this.#open; }
  set open(value: boolean) {
    const next = !!value;
    if (next && !this.#open) { this.#reset(); this.#error = ''; }
    this.#open = next;
    this.#sync();
  }
  /** While the controller writes the size: fields and Add are disabled. */
  get pending() { return this.hasAttribute('pending'); }
  set pending(value: boolean) { this.toggleAttribute('pending', !!value); }
  /** Why the controller couldn't add the size; cleared when a field changes. */
  get error() { return this.#error; }
  set error(value: string) { this.#error = value || ''; this.#sync(); }
  /** A note the controller shows above the actions, such as a preview definition's sizes. */
  get notice() { return this.#notice; }
  set notice(value: string) { this.#notice = value || ''; this.#sync(); }
  get iconRenderer() { return this.#icons; }
  set iconRenderer(value: IconRenderer | undefined) { this.#icons = value; this.#sync(); }
  /** Whether Only for this page is offered: a page listed in workbench.yaml. */
  get allowPageOnly() { return this.hasAttribute('allow-page-only'); }
  set allowPageOnly(value: boolean) { this.toggleAttribute('allow-page-only', !!value); }

  /** The fields as they are typed. */
  get draft(): SizeDraft {
    return {
      name: this.#fields.name.value, width: this.#fields.width.value, widthFill: this.#checks.widthFill.checked,
      height: this.#fields.height.value, heightFill: this.#checks.heightFill.checked, icon: this.#fields.icon.value,
      button: this.#checks.button.checked, pageOnly: this.#checks.pageOnly.checked && this.allowPageOnly,
    };
  }

  #reset() {
    for (const [key, field] of Object.entries(this.#fields)) field.value = String(EMPTY_DRAFT[key as keyof SizeDraft]);
    for (const [key, check] of Object.entries(this.#checks)) check.checked = !!EMPTY_DRAFT[key as keyof SizeDraft];
  }

  #changed = () => { this.#error = ''; this.#sync(); };

  #dismiss = (event: Event) => {
    event.preventDefault();
    if (!this.pending) this.dispatchEvent(new CustomEvent('wb-size-dialog-close', { detail: {}, bubbles: true, composed: true }));
  };

  #backdrop = (event: Event) => { if (event.target === this.#dialog) this.#dismiss(event); };

  #submit = (event: Event) => {
    event.preventDefault();
    const read = readDraft(this.draft);
    if (this.pending || !('size' in read)) return;
    this.dispatchEvent(new CustomEvent<SizeRequest>('wb-size-add', { detail: read.size, bubbles: true, composed: true }));
  };

  #sync() {
    const draft = this.draft;
    const read = readDraft(draft);
    const pending = this.pending;
    this.#fields.width.disabled = pending || draft.widthFill;
    this.#fields.height.disabled = pending || draft.heightFill;
    for (const field of [this.#fields.name, this.#fields.icon]) field.disabled = pending;
    for (const check of [this.#checks.widthFill, this.#checks.heightFill, this.#checks.pageOnly]) check.disabled = pending;
    this.#pageOnly.hidden = !this.allowPageOnly;
    this.#checks.button.disabled = pending || draft.pageOnly;
    this.#cancel.disabled = pending;
    this.#add.disabled = pending || !('size' in read);
    this.#add.textContent = pending ? 'Adding…' : 'Add';
    this.#dialog.setAttribute('aria-busy', String(pending));
    const typed = draft.name || draft.width || draft.height || draft.widthFill || draft.heightFill || draft.icon;
    this.#problem.textContent = this.#error || (typed && 'problem' in read ? read.problem : '');
    this.#noticeText.textContent = this.#notice;
    const icon = draft.icon.trim() || 'frame';
    this.#preview.replaceChildren(this.#icons && /^[a-z0-9-]+$/.test(icon) ? this.#icons(icon, 16) : document.createTextNode(''));
    if (this.#open && !this.#dialog.open && this.isConnected) { this.#dialog.showModal(); this.#fields.name.focus(); }
    if (!this.#open && this.#dialog.open) this.#dialog.close();
  }
}
