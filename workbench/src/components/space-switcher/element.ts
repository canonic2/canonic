import { WbSpaceMark } from '../space-mark/element.ts';
import type { IconRenderer, Space } from '../space-mark/space.ts';
import { styles } from './styles.ts';

export interface SpaceAction { id: string; }
export interface SpaceToggle { open: boolean; }

declare global {
  interface HTMLElementTagNameMap { 'wb-space-switcher': WbSpaceSwitcher; }
  interface HTMLElementEventMap {
    'wb-space-pick': CustomEvent<SpaceAction>;
    'wb-space-remove': CustomEvent<SpaceAction>;
    'wb-space-add': CustomEvent<Record<string, never>>;
    'wb-space-toggle': CustomEvent<SpaceToggle>;
  }
}

export class WbSpaceSwitcher extends HTMLElement {
  static observedAttributes = ['allow-add', 'allow-remove', 'disabled'];
  #spaces: Space[] = [];
  #current = '';
  #open = false;
  #icons: IconRenderer | undefined;
  #connection: AbortController | undefined;
  #root: ShadowRoot;
  #trigger: HTMLButtonElement;
  #mark: WbSpaceMark;
  #name: HTMLSpanElement;
  #caret: HTMLSpanElement;
  #menu: HTMLDivElement;
  #rows = new Map<string, HTMLDivElement>();
  #add: HTMLButtonElement;
  #rule: HTMLDivElement;

  constructor() {
    super();
    this.#root = this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(styles);
    this.#root.adoptedStyleSheets = [sheet];
    this.#trigger = document.createElement('button');
    this.#trigger.type = 'button';
    this.#trigger.className = 'trigger';
    this.#trigger.setAttribute('part', 'trigger');
    this.#trigger.setAttribute('aria-haspopup', 'menu');
    this.#trigger.setAttribute('aria-controls', 'menu');
    this.#mark = document.createElement('wb-space-mark') as WbSpaceMark;
    this.#name = document.createElement('span');
    this.#name.className = 'label';
    this.#name.setAttribute('part', 'name');
    this.#caret = document.createElement('span');
    this.#caret.className = 'caret';
    this.#trigger.append(this.#mark, this.#name, this.#caret);
    this.#menu = document.createElement('div');
    this.#menu.id = 'menu';
    this.#menu.className = 'menu';
    this.#menu.setAttribute('part', 'menu');
    this.#menu.setAttribute('role', 'menu');
    this.#menu.setAttribute('aria-label', 'Spaces');
    const heading = document.createElement('div');
    heading.className = 'heading';
    heading.textContent = 'Spaces';
    heading.setAttribute('role', 'presentation');
    this.#rule = document.createElement('div');
    this.#rule.className = 'rule';
    this.#rule.setAttribute('role', 'separator');
    this.#add = document.createElement('button');
    this.#add.type = 'button';
    this.#add.className = 'item add';
    this.#add.dataset.action = 'add';
    this.#add.setAttribute('role', 'menuitem');
    this.#add.tabIndex = -1;
    this.#menu.append(heading, this.#rule, this.#add);
    this.#root.append(this.#trigger, this.#menu);
    this.#render();
  }

  connectedCallback() {
    for (const key of ['spaces', 'currentId', 'iconRenderer', 'open', 'allowAdd', 'allowRemove', 'disabled'] as const) {
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
  get spaces(): Space[] { return this.#spaces.map(space => ({ ...space })); }
  set spaces(value: readonly Space[]) { this.#spaces = (value || []).map(space => ({ ...space })); this.#render(); }
  get currentId() { return this.#current; }
  set currentId(value: string) { this.#current = value || ''; this.#render(); }
  get iconRenderer() { return this.#icons; }
  set iconRenderer(value: IconRenderer | undefined) { this.#icons = value; this.#render(); }
  get open() { return this.#open; }
  set open(value: boolean) { this.#setOpen(!!value); }
  get allowAdd() { return this.hasAttribute('allow-add'); }
  set allowAdd(value: boolean) { this.toggleAttribute('allow-add', !!value); }
  get allowRemove() { return this.hasAttribute('allow-remove'); }
  set allowRemove(value: boolean) { this.toggleAttribute('allow-remove', !!value); }
  get disabled() { return this.hasAttribute('disabled'); }
  set disabled(value: boolean) { this.toggleAttribute('disabled', !!value); }

  #emit<T>(name: string, detail: T) {
    this.dispatchEvent(new CustomEvent<T>(name, { detail, bubbles: true, composed: true }));
  }

  #setOpen(value: boolean, user = false, focus = false) {
    const next = value && !this.disabled;
    const changed = this.#open !== next;
    this.#open = next;
    this.#menu.hidden = !next;
    this.#trigger.setAttribute('aria-expanded', String(next));
    if (focus) {
      if (next) (this.#menu.querySelector<HTMLButtonElement>('[aria-checked=true]') || this.#items()[0])?.focus();
      else this.#trigger.focus();
    }
    if (changed && user) this.#emit<SpaceToggle>('wb-space-toggle', { open: next });
  }

  #items() { return Array.from(this.#menu.querySelectorAll<HTMLButtonElement>('button')).filter(button => !button.hidden && !button.disabled); }

  #click = (event: Event) => {
    const target = (event.target as Element).closest('button');
    if (!target || target.disabled || this.disabled) return;
    if (target === this.#trigger) {
      this.#setOpen(!this.#open, true, (event as MouseEvent).detail === 0);
      return;
    }
    const action = target.dataset.action;
    const id = target.dataset.id || '';
    this.#setOpen(false, true, true);
    if (action === 'pick' && id !== this.#current) this.#emit<SpaceAction>('wb-space-pick', { id });
    else if (action === 'remove') this.#emit<SpaceAction>('wb-space-remove', { id });
    else if (action === 'add') this.#emit('wb-space-add', {});
  };

  #key = (event: Event) => {
    const e = event as KeyboardEvent;
    if (this.disabled) return;
    if (e.key === 'Escape' && this.#open) {
      e.preventDefault(); e.stopPropagation(); this.#setOpen(false, true, true); return;
    }
    // Move the sequential focus starting point out of the soon-hidden menu;
    // the browser then performs its normal Tab/Shift+Tab traversal.
    if (e.key === 'Tab' && this.#open) { this.#setOpen(false, true, true); return; }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    const items = this.#items();
    if (this.#root.activeElement === this.#trigger) {
      if (!['ArrowDown', 'ArrowUp'].includes(e.key)) return;
      e.preventDefault(); this.#setOpen(true, true, true); return;
    }
    if (!this.#open || !items.length) return;
    e.preventDefault();
    const at = items.indexOf(this.#root.activeElement as HTMLButtonElement);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1
      : (at + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  };

  #outside = (event: Event) => { if (!event.composedPath().includes(this)) this.#setOpen(false, true); };
  #blur = () => { this.#setOpen(false, true); };

  #glyph(name: string, fallback: string): Element {
    if (this.#icons) return this.#icons(name, 14);
    const node = document.createElement('span');
    node.textContent = fallback;
    node.setAttribute('aria-hidden', 'true');
    return node;
  }

  #render() {
    const shown = this.#spaces.find(space => space.id === this.#current);
    this.#mark.hidden = !shown;
    this.#mark.iconRenderer = this.#icons;
    this.#mark.space = shown || {};
    this.#name.textContent = shown?.name || 'Workbench';
    this.#caret.replaceChildren(this.#glyph('chevrons-up-down', '↕'));
    this.#trigger.title = shown ? `${shown.name}${shown.root ? ' — ' + shown.root : ''}\nSwitch space` : 'Switch space';
    this.#trigger.disabled = this.disabled;
    const active = this.#root.activeElement as HTMLButtonElement | null;
    const scroll = this.#menu.scrollTop;
    let cursor = this.#menu.children[1] || this.#rule;
    const wanted = new Set<string>();
    for (const space of this.#spaces) {
      if (!space.id || wanted.has(space.id)) continue;
      wanted.add(space.id);
      let row = this.#rows.get(space.id);
      if (!row) {
        row = document.createElement('div');
        row.className = 'row';
        row.setAttribute('role', 'none');
        const pick = document.createElement('button');
        pick.type = 'button'; pick.className = 'item'; pick.tabIndex = -1;
        pick.dataset.action = 'pick'; pick.dataset.id = space.id;
        pick.setAttribute('role', 'menuitemradio');
        const mark = document.createElement('wb-space-mark');
        const label = document.createElement('span'); label.className = 'label';
        const check = document.createElement('span'); check.className = 'check';
        pick.append(mark, label, check); row.append(pick);
        this.#rows.set(space.id, row);
      }
      const pick = row.firstElementChild as HTMLButtonElement;
      pick.disabled = this.disabled;
      pick.setAttribute('aria-checked', String(space.id === this.#current));
      pick.title = space.root || space.name || '';
      const mark = pick.firstElementChild as WbSpaceMark;
      mark.iconRenderer = this.#icons; mark.space = space;
      pick.querySelector('.label')!.textContent = space.name || '';
      pick.querySelector('.check')!.replaceChildren(this.#glyph('check', '✓'));
      let remove = row.querySelector<HTMLButtonElement>('.remove');
      if (space.removable && this.allowRemove) {
        if (!remove) {
          remove = document.createElement('button'); remove.type = 'button'; remove.className = 'remove';
          remove.tabIndex = -1; remove.dataset.action = 'remove'; remove.dataset.id = space.id;
          remove.setAttribute('role', 'menuitem'); row.append(remove);
        }
        remove.disabled = this.disabled;
        remove.setAttribute('aria-label', `Remove ${space.name} from Workbench`);
        remove.title = 'Remove from Workbench';
        remove.replaceChildren(this.#glyph('x', '×'));
      } else remove?.remove();
      if (row !== cursor) this.#menu.insertBefore(row, cursor);
      cursor = row.nextElementSibling || this.#rule;
    }
    for (const [id, row] of this.#rows) {
      if (!wanted.has(id)) { row.remove(); this.#rows.delete(id); }
    }
    this.#rule.hidden = this.#add.hidden = !this.allowAdd;
    this.#add.disabled = this.disabled;
    this.#add.replaceChildren(this.#glyph('plus', '+'), document.createTextNode('Add a space…'));
    this.#setOpen(this.#open);
    this.#menu.scrollTop = scroll;
    if (active && this.#open) {
      if (active.isConnected && !active.hidden) active.focus({ preventScroll: true });
      else (this.#items()[0] || this.#trigger).focus({ preventScroll: true });
    }
  }
}
