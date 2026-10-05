/* <wb-actions-switch>: whether the page's links and forms work, in the top bar
   ---------------------------------------------------------------------------
   An icon and a switch. Off, the preview logs a link or a submit instead of
   following it; the canvas controller owns the setting, reloads the page with
   it, and locks the switch on through a lens the workbench doesn't serve.
   The element only shows the state and asks for a change. */

import type { IconRenderer } from '../space-mark/space.ts';
import { styles } from './styles.ts';

export interface ActionsToggle { checked: boolean }

declare global {
  interface HTMLElementTagNameMap { 'wb-actions-switch': WbActionsSwitch }
  interface HTMLElementEventMap { 'wb-actions-toggle': CustomEvent<ActionsToggle> }
}

const INPUTS = ['checked', 'disabled', 'hint', 'iconRenderer'] as const;
const HINT = 'let links navigate and forms submit';

export class WbActionsSwitch extends HTMLElement {
  static observedAttributes = ['checked', 'disabled', 'hint'];
  #icons: IconRenderer | undefined;
  #connection: AbortController | undefined;
  #button: HTMLButtonElement;
  #icon: HTMLSpanElement;

  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(styles);
    root.adoptedStyleSheets = [sheet];
    this.#button = document.createElement('button');
    this.#button.type = 'button';
    this.#button.setAttribute('role', 'switch');
    this.#button.setAttribute('aria-label', 'Actions');
    this.#button.setAttribute('part', 'button');
    this.#icon = document.createElement('span');
    this.#icon.className = 'icon';
    const track = document.createElement('span');
    track.className = 'track';
    const knob = document.createElement('span');
    knob.className = 'knob';
    track.append(knob);
    this.#button.append(this.#icon, track);
    root.append(this.#button);
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
    this.#button.addEventListener('click', this.#click, { signal: this.#connection.signal });
    this.#render();
  }

  disconnectedCallback() {
    this.#connection?.abort();
    this.#connection = undefined;
  }

  attributeChangedCallback() { this.#render(); }

  /** Whether links and forms work in the page. Setting it emits nothing. */
  get checked() { return this.hasAttribute('checked'); }
  set checked(value: boolean) { this.toggleAttribute('checked', !!value); }
  /** Locks the switch where it is, as through a lens the workbench doesn't serve. */
  get disabled() { return this.hasAttribute('disabled'); }
  set disabled(value: boolean) { this.toggleAttribute('disabled', !!value); }
  /** What the title says after "Actions — "; says what the switch does by default. */
  get hint() { return this.getAttribute('hint') || HINT; }
  set hint(value: string) { if (value) this.setAttribute('hint', value); else this.removeAttribute('hint'); }
  get iconRenderer() { return this.#icons; }
  set iconRenderer(value: IconRenderer | undefined) { this.#icons = value; this.#render(); }

  #click = () => {
    if (this.disabled) return;
    this.dispatchEvent(new CustomEvent<ActionsToggle>('wb-actions-toggle', { detail: { checked: !this.checked }, bubbles: true, composed: true }));
  };

  #render() {
    this.#button.disabled = this.disabled;
    this.#button.setAttribute('aria-checked', String(this.checked));
    this.#button.title = 'Actions — ' + this.hint;
    if (this.#icons) this.#icon.replaceChildren(this.#icons('mouse-pointer-click', 16));
    else this.#icon.replaceChildren();
  }
}
