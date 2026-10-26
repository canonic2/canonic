import { isLight, markColor } from './space.js';
import { styles } from './styles.js';
import { stylesFor } from '../shared/stylesheet.js';
const stylesheet = stylesFor(styles);

/** Decorative mark; the surrounding control supplies its accessible name. */
export class WbSpaceMark extends HTMLElement {
  #space = {};
  #icons;
  #mark;
  #content;

  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' });
    root.adoptedStyleSheets = [stylesheet(this.ownerDocument)];
    this.#mark = document.createElement('span');
    this.#mark.className = 'mark';
    this.#mark.setAttribute('aria-hidden', 'true');
    root.append(this.#mark);
  }

  connectedCallback() {
    for (const key of ['space', 'iconRenderer']) {
      if (Object.hasOwn(this, key)) {
        const value = this[key];
        delete this[key];
        Reflect.set(this, key, value);
      }
    }
    this.#render();
  }

  get space() {
    return { ...this.#space };
  }
  set space(value) {
    this.#space = { ...value };
    this.#render();
  }
  get iconRenderer() {
    return this.#icons;
  }
  set iconRenderer(value) {
    this.#icons = value;
    this.#render();
  }

  #render() {
    const space = this.#space;
    const color = markColor(space.color);
    this.#mark.style.background = color;
    this.#mark.style.color = color && isLight(color) ? '#1d1d1d' : 'white';
    this.#mark.classList.toggle('colored', !!color);
    const content = space.image
      ? ['image', space.image]
      : space.icon && this.#icons
        ? ['icon', space.icon, this.#icons]
        : ['text', space.initial || space.name?.slice(0, 1) || '?'];
    if (this.#content?.every((value, at) => value === content[at])) return;
    this.#content = content;
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
