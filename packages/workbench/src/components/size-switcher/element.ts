/* <wb-size-switcher>: the artboard's size, in the top bar
   ------------------------------------------------------
   The space's button sizes as a row of icon buttons, then a menu button that
   lists every size — the buttons under Sizes, the rest under More sizes — and
   Custom size… and Edit sizes… when the host allows editing. It only shows
   sizes and says what was asked for; the canvas controller owns the current
   size and every change to workbench.yaml. See specs/sizes.md. */

import { describe, dimensionsText } from '../../sizes/browser/choice.ts';
import type { ResolvedSize } from '../../sizes/browser/size.ts';
import type { IconRenderer } from '../space-mark/space.ts';
import { styles } from './styles.ts';

/** A size as the switcher shows it: the space's, or a page's own. */
export type SizeOption = Pick<ResolvedSize, 'key' | 'label' | 'icon' | 'kind' | 'width' | 'height' | 'button'>;
export interface SizePick { key: string }
export interface SizeToggle { open: boolean }

declare global {
  interface HTMLElementTagNameMap { 'wb-size-switcher': WbSizeSwitcher }
  interface HTMLElementEventMap {
    'wb-size-pick': CustomEvent<SizePick>;
    'wb-size-custom': CustomEvent<Record<string, never>>;
    'wb-size-edit': CustomEvent<Record<string, never>>;
    'wb-size-toggle': CustomEvent<SizeToggle>;
  }
}

const INPUTS = ['sizes', 'supported', 'current', 'iconRenderer', 'open', 'allowEdit', 'disabled'] as const;

export class WbSizeSwitcher extends HTMLElement {
  static observedAttributes = ['allow-edit', 'disabled', 'unsupported-reason'];
  #sizes: SizeOption[] = [];
  #supported: string[] | null = null;
  #current = '';
  #open = false;
  #icons: IconRenderer | undefined;
  #connection: AbortController | undefined;
  #root: ShadowRoot;
  #group: HTMLDivElement;
  #more: HTMLButtonElement;
  #menu: HTMLDivElement;
  #buttons = new Map<string, HTMLButtonElement>();

  constructor() {
    super();
    this.#root = this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(styles);
    this.#root.adoptedStyleSheets = [sheet];
    this.#group = document.createElement('div');
    this.#group.className = 'group';
    this.#group.setAttribute('role', 'group');
    this.#group.setAttribute('aria-label', 'Size');
    this.#group.setAttribute('part', 'group');
    this.#more = document.createElement('button');
    this.#more.type = 'button';
    this.#more.className = 'more';
    this.#more.dataset.action = 'menu';
    this.#more.setAttribute('aria-haspopup', 'menu');
    this.#more.setAttribute('aria-controls', 'menu');
    this.#more.setAttribute('part', 'more');
    this.#group.append(this.#more);
    this.#menu = document.createElement('div');
    this.#menu.id = 'menu';
    this.#menu.className = 'menu';
    this.#menu.setAttribute('role', 'menu');
    this.#menu.setAttribute('aria-label', 'Sizes');
    this.#menu.setAttribute('part', 'menu');
    this.#root.append(this.#group, this.#menu);
    this.#render();
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
    this.#root.addEventListener('click', this.#click, options);
    this.#root.addEventListener('keydown', this.#key, options);
    this.ownerDocument.addEventListener('pointerdown', this.#outside, options);
    this.ownerDocument.defaultView?.addEventListener('blur', this.#blur, options);
    this.#render();
  }

  disconnectedCallback() {
    this.#connection?.abort();
    this.#connection = undefined;
    this.#setOpen(false);
  }

  attributeChangedCallback() { this.#render(); }

  /** Every size the menu lists, in order: the space's, then the page's own. Copied on assignment. */
  get sizes(): SizeOption[] { return this.#sizes.map(size => ({ ...size })); }
  set sizes(value: readonly SizeOption[]) { this.#sizes = (value || []).filter(size => size && size.key).map(size => ({ ...size })); this.#render(); }
  /** The keys the page supports; null when every size is. An empty list disables every size, as on a docs page. */
  get supported(): string[] | null { return this.#supported ? [...this.#supported] : null; }
  set supported(value: readonly string[] | null) { this.#supported = value ? [...value] : null; this.#render(); }
  /** The key of the size the artboard is at; empty when none is pressed. */
  get current() { return this.#current; }
  set current(value: string) { this.#current = value || ''; this.#render(); }
  get iconRenderer() { return this.#icons; }
  set iconRenderer(value: IconRenderer | undefined) { this.#icons = value; this.#render(); }
  get open() { return this.#open; }
  set open(value: boolean) { this.#setOpen(!!value); }
  get allowEdit() { return this.hasAttribute('allow-edit'); }
  set allowEdit(value: boolean) { this.toggleAttribute('allow-edit', !!value); }
  get disabled() { return this.hasAttribute('disabled'); }
  set disabled(value: boolean) { this.toggleAttribute('disabled', !!value); }

  #enabled(key: string) { return !this.disabled && (!this.#supported || this.#supported.includes(key)); }

  #emit<T>(name: string, detail: T) {
    this.dispatchEvent(new CustomEvent<T>(name, { detail, bubbles: true, composed: true }));
  }

  #setOpen(value: boolean, user = false, focus = false) {
    const next = value && !this.disabled;
    const changed = this.#open !== next;
    this.#open = next;
    this.#menu.hidden = !next;
    this.#more.setAttribute('aria-expanded', String(next));
    if (focus) {
      if (next) (this.#menu.querySelector<HTMLButtonElement>('[aria-checked=true]:not(:disabled)') || this.#items()[0])?.focus();
      else this.#more.focus();
    }
    if (changed && user) this.#emit<SizeToggle>('wb-size-toggle', { open: next });
  }

  #items() { return Array.from(this.#menu.querySelectorAll<HTMLButtonElement>('button')).filter(button => !button.disabled); }

  #pick(key: string) {
    if (key && key !== this.#current && this.#enabled(key)) this.#emit<SizePick>('wb-size-pick', { key });
  }

  #click = (event: Event) => {
    const target = (event.target as Element).closest('button');
    if (!target || target.disabled || this.disabled) return;
    const action = target.dataset.action;
    if (action === 'menu') { this.#setOpen(!this.#open, true, (event as MouseEvent).detail === 0); return; }
    if (action === 'size') { this.#pick(target.dataset.key || ''); return; }
    this.#setOpen(false, true, true);
    if (action === 'pick') this.#pick(target.dataset.key || '');
    else if (action === 'custom') this.#emit('wb-size-custom', {});
    else if (action === 'edit') this.#emit('wb-size-edit', {});
  };

  #key = (event: Event) => {
    const e = event as KeyboardEvent;
    if (this.disabled) return;
    if (e.key === 'Escape' && this.#open) { e.preventDefault(); e.stopPropagation(); this.#setOpen(false, true, true); return; }
    if (e.key === 'Tab' && this.#open) { this.#setOpen(false, true, true); return; }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    if (this.#root.activeElement === this.#more) {
      if (!['ArrowDown', 'ArrowUp'].includes(e.key)) return;
      e.preventDefault(); this.#setOpen(true, true, true); return;
    }
    const items = this.#items();
    if (!this.#open || !items.length || !this.#menu.contains(this.#root.activeElement)) return;
    e.preventDefault();
    const at = items.indexOf(this.#root.activeElement as HTMLButtonElement);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1
      : (at + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  };

  #outside = (event: Event) => { if (this.#open && !event.composedPath().includes(this)) this.#setOpen(false, true); };
  #blur = () => { if (this.#open) this.#setOpen(false, true); };

  #glyph(name: string, fallback: string, size = 16): Element {
    if (this.#icons) return this.#icons(name, size);
    const node = document.createElement('span');
    node.textContent = fallback;
    node.setAttribute('aria-hidden', 'true');
    return node;
  }

  #reason() { return this.getAttribute('unsupported-reason') || 'not supported by this page'; }

  #renderButtons() {
    const wanted = new Set<string>();
    for (const size of this.#sizes.filter(candidate => candidate.button)) {
      wanted.add(size.key);
      let button = this.#buttons.get(size.key);
      if (!button) {
        button = document.createElement('button');
        button.type = 'button';
        button.className = 'size';
        button.dataset.action = 'size';
        button.dataset.key = size.key;
        this.#buttons.set(size.key, button);
      }
      const enabled = this.#enabled(size.key);
      const name = describe(size as ResolvedSize);
      button.disabled = !enabled;
      button.setAttribute('aria-pressed', String(size.key === this.#current));
      button.setAttribute('aria-label', name);
      button.title = enabled ? name : name + ' · ' + this.#reason();
      button.replaceChildren(this.#glyph(size.icon, size.label.charAt(0)));
      this.#group.insertBefore(button, this.#more);
    }
    for (const [key, button] of this.#buttons) {
      if (!wanted.has(key)) { button.remove(); this.#buttons.delete(key); }
    }
  }

  #renderMore() {
    const shown = this.#sizes.find(size => size.key === this.#current && !size.button);
    this.#more.disabled = this.disabled;
    this.#more.setAttribute('aria-pressed', String(!!shown));
    const name = shown ? describe(shown as ResolvedSize) : 'More sizes';
    this.#more.setAttribute('aria-label', name);
    this.#more.title = name;
    this.#more.replaceChildren(shown ? this.#glyph(shown.icon, shown.label.charAt(0)) : this.#glyph('chevron-down', '▾', 14));
  }

  #item(size: SizeOption): HTMLButtonElement {
    const item = document.createElement('button');
    item.type = 'button';
    item.className = 'item';
    item.tabIndex = -1;
    item.dataset.action = 'pick';
    item.dataset.key = size.key;
    item.setAttribute('role', 'menuitemradio');
    const enabled = this.#enabled(size.key);
    item.disabled = !enabled;
    item.setAttribute('aria-checked', String(size.key === this.#current));
    const check = document.createElement('span'); check.className = 'check';
    check.append(this.#glyph('check', '✓', 14));
    const icon = document.createElement('span'); icon.className = 'icon';
    icon.append(this.#glyph(size.icon, '', 16));
    const label = document.createElement('span'); label.className = 'label'; label.textContent = size.label;
    const dims = document.createElement('span'); dims.className = 'dims'; dims.textContent = dimensionsText(size as ResolvedSize);
    item.append(check, icon, label, dims);
    item.title = enabled ? describe(size as ResolvedSize) : describe(size as ResolvedSize) + ' · ' + this.#reason();
    return item;
  }

  #section(title: string, sizes: SizeOption[]): Node[] {
    if (!sizes.length) return [];
    const heading = document.createElement('div');
    heading.className = 'heading';
    heading.setAttribute('role', 'presentation');
    heading.textContent = title;
    return [heading, ...sizes.map(size => this.#item(size))];
  }

  #rule(): HTMLDivElement {
    const rule = document.createElement('div');
    rule.className = 'rule';
    rule.setAttribute('role', 'separator');
    return rule;
  }

  #action(action: string, text: string): HTMLButtonElement {
    const item = document.createElement('button');
    item.type = 'button';
    item.className = 'item action';
    item.tabIndex = -1;
    item.dataset.action = action;
    item.setAttribute('role', 'menuitem');
    const label = document.createElement('span'); label.className = 'label'; label.textContent = text;
    item.append(label);
    return item;
  }

  /* The menu is rebuilt from its inputs; focus moves to the same entry after. */
  #renderMenu() {
    const focused = this.#root.activeElement as HTMLButtonElement | null;
    const focusKey = focused && this.#menu.contains(focused) ? (focused.dataset.key || focused.dataset.action) : null;
    const scroll = this.#menu.scrollTop;
    const buttons = this.#sizes.filter(size => size.button);
    const more = this.#sizes.filter(size => !size.button);
    const parts: Node[] = [...this.#section('Sizes', buttons)];
    const rest = this.#section('More sizes', more);
    if (rest.length) parts.push(...(parts.length ? [this.#rule()] : []), ...rest);
    if (this.allowEdit) parts.push(...(parts.length ? [this.#rule()] : []), this.#action('custom', 'Custom size…'), this.#action('edit', 'Edit sizes…'));
    this.#menu.replaceChildren(...parts);
    this.#menu.scrollTop = scroll;
    if (focusKey && this.#open) {
      const again = Array.from(this.#menu.querySelectorAll<HTMLButtonElement>('button'))
        .find(item => (item.dataset.key || item.dataset.action) === focusKey && !item.disabled);
      (again || this.#items()[0] || this.#more).focus({ preventScroll: true });
    }
  }

  #render() {
    this.#renderButtons();
    this.#renderMore();
    this.#renderMenu();
    this.#setOpen(this.#open);
  }
}
