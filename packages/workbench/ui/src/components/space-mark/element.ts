import { isLight, markColor, type IconRenderer, type Space } from './space.ts';
import { styles } from './styles.ts';

declare global {
  interface HTMLElementTagNameMap { 'wb-space-mark': WbSpaceMark; }
}

/** Decorative mark; the surrounding control supplies its accessible name. */
export class WbSpaceMark extends HTMLElement {
  #space: Space = {};
  #icons: IconRenderer | undefined;
  #mark: HTMLSpanElement;

  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(styles);
    root.adoptedStyleSheets = [sheet];
    this.#mark = document.createElement('span');
    this.#mark.className = 'mark';
    this.#mark.setAttribute('aria-hidden', 'true');
    root.append(this.#mark);
  }

  connectedCallback() {
    for (const key of ['space', 'iconRenderer'] as const) {
      if (Object.hasOwn(this, key)) {
        const value = this[key];
        delete (this as unknown as Record<string, unknown>)[key];
        Reflect.set(this, key, value);
      }
    }
    this.#render();
  }

  get space(): Space { return { ...this.#space }; }
  set space(value: Space) { this.#space = { ...value }; this.#render(); }
  get iconRenderer() { return this.#icons; }
  set iconRenderer(value: IconRenderer | undefined) { this.#icons = value; this.#render(); }

  #render() {
    const space = this.#space;
    const color = markColor(space.color);
    this.#mark.style.background = color;
    this.#mark.style.color = color && isLight(color) ? '#1d1d1d' : 'white';
    this.#mark.classList.toggle('colored', !!color);
    if (space.image) {
      const image = document.createElement('img');
      image.src = space.image;
      image.alt = '';
      this.#mark.replaceChildren(image);
    } else if (space.icon && this.#icons) {
      this.#mark.replaceChildren(this.#icons(space.icon, 14));
    } else {
      this.#mark.textContent = space.initial || space.name?.slice(0, 1) || '?';
    }
  }
}
