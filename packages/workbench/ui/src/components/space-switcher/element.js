import { styles } from './styles.js';

export class WbSpaceSwitcher extends HTMLElement {
  static observedAttributes = ['allow-add', 'allow-remove', 'disabled'];
  #spaces = [];
  #current = '';
  #open = false;
  #icons;
  #connection;
  #root;
  #trigger;
  #mark;
  #name;
  #caret;
  #menu;
  #rows = new Map();
  #add;
  #rule;

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
    this.#mark = document.createElement('wb-space-mark');
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
    for (const key of [
      'spaces',
      'currentId',
      'iconRenderer',
      'open',
      'allowAdd',
      'allowRemove',
      'disabled',
    ]) {
      if (Object.hasOwn(this, key)) {
        const value = this[key];
        delete this[key];
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

  attributeChangedCallback() {
    this.#render();
  }
  get spaces() {
    return this.#spaces.map((space) => ({ ...space }));
  }
  set spaces(value) {
    this.#spaces = (value || []).map((space) => ({ ...space }));
    this.#render();
  }
  get currentId() {
    return this.#current;
  }
  set currentId(value) {
    this.#current = value || '';
    this.#render();
  }
  get iconRenderer() {
    return this.#icons;
  }
  set iconRenderer(value) {
    this.#icons = value;
    this.#render();
  }
  get open() {
    return this.#open;
  }
  set open(value) {
    this.#setOpen(!!value);
  }
  get allowAdd() {
    return this.hasAttribute('allow-add');
  }
  set allowAdd(value) {
    this.toggleAttribute('allow-add', !!value);
  }
  get allowRemove() {
    return this.hasAttribute('allow-remove');
  }
  set allowRemove(value) {
    this.toggleAttribute('allow-remove', !!value);
  }
  get disabled() {
    return this.hasAttribute('disabled');
  }
  set disabled(value) {
    this.toggleAttribute('disabled', !!value);
  }

  #emit(name, detail) {
    this.dispatchEvent(new CustomEvent(name, { detail, bubbles: true, composed: true }));
  }

  #setOpen(value, user = false, focus = false) {
    const next = value && !this.disabled;
    const changed = this.#open !== next;
    this.#open = next;
    this.#menu.hidden = !next;
    this.#trigger.setAttribute('aria-expanded', String(next));
    if (focus) {
      if (next) (this.#menu.querySelector('[aria-checked=true]') || this.#items()[0])?.focus();
      else this.#trigger.focus();
    }
    if (changed && user) this.#emit('wb-space-toggle', { open: next });
  }

  #items() {
    return Array.from(this.#menu.querySelectorAll('button')).filter(
      (button) => !button.hidden && !button.disabled,
    );
  }

  #click = (event) => {
    const target = event.target.closest('button');
    if (!target || target.disabled || this.disabled) return;
    if (target === this.#trigger) {
      this.#setOpen(!this.#open, true, event.detail === 0);
      return;
    }
    const action = target.dataset.action;
    const id = target.dataset.id || '';
    this.#setOpen(false, true, true);
    if (action === 'pick' && id !== this.#current) this.#emit('wb-space-pick', { id });
    else if (action === 'remove') this.#emit('wb-space-remove', { id });
    else if (action === 'add') this.#emit('wb-space-add', {});
  };

  #key = (event) => {
    const e = event;
    if (this.disabled) return;
    if (e.key === 'Escape' && this.#open) {
      e.preventDefault();
      e.stopPropagation();
      this.#setOpen(false, true, true);
      return;
    }
    // Move the sequential focus starting point out of the soon-hidden menu;
    // the browser then performs its normal Tab/Shift+Tab traversal.
    if (e.key === 'Tab' && this.#open) {
      this.#setOpen(false, true, true);
      return;
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    const items = this.#items();
    if (this.#root.activeElement === this.#trigger) {
      if (!['ArrowDown', 'ArrowUp'].includes(e.key)) return;
      e.preventDefault();
      this.#setOpen(true, true, true);
      return;
    }
    if (!this.#open || !items.length) return;
    e.preventDefault();
    const at = items.indexOf(this.#root.activeElement);
    const next =
      e.key === 'Home'
        ? 0
        : e.key === 'End'
          ? items.length - 1
          : (at + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  };

  #outside = (event) => {
    if (!event.composedPath().includes(this)) this.#setOpen(false, true);
  };
  #blur = () => {
    this.#setOpen(false, true);
  };

  #glyph(name, fallback) {
    if (this.#icons) return this.#icons(name, 14);
    const node = document.createElement('span');
    node.textContent = fallback;
    node.setAttribute('aria-hidden', 'true');
    return node;
  }

  #render() {
    // Only an open, connected menu has a scroll position to preserve. Reading
    // after DOM writes forces layout even when the menu is closed.
    const scroll = this.#open && this.isConnected ? this.#menu.scrollTop : null;
    const shown = this.#spaces.find((space) => space.id === this.#current);
    this.#mark.hidden = !shown;
    this.#mark.iconRenderer = this.#icons;
    this.#mark.space = shown || {};
    this.#name.textContent = shown?.name || 'Workbench';
    this.#caret.replaceChildren(this.#glyph('chevrons-up-down', '↕'));
    this.#trigger.title = shown
      ? `${shown.name}${shown.root ? ' — ' + shown.root : ''}\nSwitch space`
      : 'Switch space';
    this.#trigger.disabled = this.disabled;
    const active = this.#root.activeElement;
    let cursor = this.#menu.children[1] || this.#rule;
    const wanted = new Set();
    for (const space of this.#spaces) {
      if (!space.id || wanted.has(space.id)) continue;
      wanted.add(space.id);
      let row = this.#rows.get(space.id);
      if (!row) {
        row = document.createElement('div');
        row.className = 'row';
        row.setAttribute('role', 'none');
        const pick = document.createElement('button');
        pick.type = 'button';
        pick.className = 'item';
        pick.tabIndex = -1;
        pick.dataset.action = 'pick';
        pick.dataset.id = space.id;
        pick.setAttribute('role', 'menuitemradio');
        const mark = document.createElement('wb-space-mark');
        const label = document.createElement('span');
        label.className = 'label';
        const check = document.createElement('span');
        check.className = 'check';
        pick.append(mark, label, check);
        row.append(pick);
        this.#rows.set(space.id, row);
      }
      const pick = row.firstElementChild;
      pick.disabled = this.disabled;
      pick.setAttribute('aria-checked', String(space.id === this.#current));
      pick.title = space.root || space.name || '';
      const mark = pick.firstElementChild;
      mark.iconRenderer = this.#icons;
      mark.space = space;
      pick.querySelector('.label').textContent = space.name || '';
      pick.querySelector('.check').replaceChildren(this.#glyph('check', '✓'));
      let remove = row.querySelector('.remove');
      if (space.removable && this.allowRemove) {
        if (!remove) {
          remove = document.createElement('button');
          remove.type = 'button';
          remove.className = 'remove';
          remove.tabIndex = -1;
          remove.dataset.action = 'remove';
          remove.dataset.id = space.id;
          remove.setAttribute('role', 'menuitem');
          row.append(remove);
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
      if (!wanted.has(id)) {
        row.remove();
        this.#rows.delete(id);
      }
    }
    this.#rule.hidden = this.#add.hidden = !this.allowAdd;
    this.#add.disabled = this.disabled;
    this.#add.replaceChildren(this.#glyph('plus', '+'), document.createTextNode('Add a space…'));
    this.#setOpen(this.#open);
    if (scroll !== null) this.#menu.scrollTop = scroll;
    if (active && this.#open) {
      if (active.isConnected && !active.hidden) active.focus({ preventScroll: true });
      else (this.#items()[0] || this.#trigger).focus({ preventScroll: true });
    }
  }
}
