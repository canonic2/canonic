import { styles } from './styles.js';
import { stylesFor } from '../../../../components/shared/stylesheet.js';
const stylesheet = stylesFor(styles);
import {
  artboardLabel,
  centeredX,
  docsLayout,
  fit,
  zoom,
} from '../../../../modules/canvas/presentation.js';

/** Keyed artboard presentation. The application owns descriptors and renderers;
 * this element owns only its camera and connection-scoped input handlers. */
export class WbCanvas extends HTMLElement {
  static observedAttributes = ['reading'];
  #root = this.attachShadow({ mode: 'open' });
  #world = document.createElement('div');
  #empty = document.createElement('p');
  #panLayer = document.createElement('div');
  #staging = document.createElement('div');
  #staged = new Map();
  #stagedBoards = [];
  #nodes = new Map();
  #state = { id: '', revision: 0, selected: null, artboards: [] };
  #camera = { x: 32, y: 32, scale: 1 };
  #fitted = true;
  #connection;
  #observer;
  #pan;
  #space = false;
  #resize;
  constructor() {
    super();
    this.#root.adoptedStyleSheets = [stylesheet(this.ownerDocument)];
    this.#world.className = 'world';
    this.#empty.textContent = 'Pick a page or a component to see it here.';
    this.#panLayer.className = 'pan';
    this.#panLayer.hidden = true;
    this.#staging.className = 'staging';
    this.#staging.inert = true;
    this.#staging.setAttribute('aria-hidden', 'true');
    this.#root.append(this.#empty, this.#world, this.#staging, this.#panLayer);
  }
  connectedCallback() {
    for (const key of ['state', 'reading', 'staged']) {
      if (Object.hasOwn(this, key)) {
        const value = this[key];
        delete this[key];
        this[key] = value;
      }
    }
    this.#connection?.abort();
    const signal = (this.#connection = new AbortController()).signal;
    this.tabIndex = 0;
    this.#describe();
    this.addEventListener(
      'wheel',
      (event) => {
        event.preventDefault();
        if (event.ctrlKey || event.metaKey) {
          const r = this.getBoundingClientRect();
          this.#zoom(this.#camera.scale * Math.exp(-event.deltaY * 0.01), {
            x: event.clientX - r.left,
            y: event.clientY - r.top,
          });
          return;
        } else {
          this.#camera.x -= event.deltaX;
          if (this.#reading()) this.#scroll({ dy: event.deltaY / this.#camera.scale });
          else this.#camera.y -= event.deltaY;
        }
        this.#fitted = false;
        this.#paint();
      },
      { passive: false, signal },
    );
    this.addEventListener(
      'keydown',
      (event) => {
        if (event.target !== this) return;
        if (event.code === 'Space') {
          event.preventDefault();
          this.panReady(true);
        }
        if (event.shiftKey && event.key === '1') {
          event.preventDefault();
          this.fitAll();
        }
        if (event.shiftKey && event.key === '2') {
          event.preventDefault();
          this.fitSelected();
        }
        if ((event.metaKey || event.ctrlKey) && ['0', '=', '+', '-'].includes(event.key)) {
          event.preventDefault();
          this.zoomTo(
            event.key === '0' ? 1 : this.#camera.scale * (event.key === '-' ? 0.8 : 1.25),
          );
        }
      },
      { signal },
    );
    window.addEventListener(
      'keyup',
      (event) => {
        if (event.code === 'Space') this.panReady(false);
      },
      { signal },
    );
    window.addEventListener(
      'blur',
      () => {
        this.panReady(false);
        this.#pan = undefined;
      },
      { signal },
    );
    this.addEventListener(
      'pointerdown',
      (event) => {
        if (
          event.button !== 1 &&
          !this.#space &&
          event.composedPath()[0] !== this &&
          event.composedPath()[0] !== this.#world
        )
          return;
        event.preventDefault();
        this.focus();
        this.setPointerCapture(event.pointerId);
        this.dispatchEvent(
          new CustomEvent('wb-artboard-deselect', { bubbles: true, composed: true }),
        );
        this.#pan = { x: event.clientX, y: event.clientY, camera: { ...this.#camera } };
      },
      { signal },
    );
    this.addEventListener(
      'pointermove',
      (event) => {
        if (this.#resize) {
          const drag = this.#resize,
            dx = (event.clientX - drag.x) / this.#camera.scale,
            dy = (event.clientY - drag.y) / this.#camera.scale;
          const width =
            drag.edge === 'bottom'
              ? drag.width
              : Math.min(
                  8192,
                  Math.max(1, Math.round(drag.width + (drag.edge.includes('left') ? -dx : dx))),
                );
          const height = drag.edge.includes('bottom')
            ? Math.min(8192, Math.max(1, Math.round(drag.height + dy)))
            : drag.height;
          this.dispatchEvent(
            new CustomEvent('wb-artboard-resize', {
              detail: { id: drag.id, width, height },
              bubbles: true,
              composed: true,
            }),
          );
          return;
        }
        if (!this.#pan) return;
        if (this.#reading()) {
          this.#scroll({ dy: (this.#pan.y - event.clientY) / this.#camera.scale });
          this.#pan.y = event.clientY;
        }
        this.#camera = {
          ...this.#pan.camera,
          x: this.#pan.camera.x + event.clientX - this.#pan.x,
          y: this.#reading() ? 0 : this.#pan.camera.y + event.clientY - this.#pan.y,
        };
        this.#fitted = false;
        this.#paint();
      },
      { signal },
    );
    this.addEventListener(
      'lostpointercapture',
      () => {
        this.#pan = undefined;
        this.#resize = undefined;
      },
      { signal },
    );
    this.addEventListener(
      'pointerup',
      (event) => {
        this.#pan = undefined;
        this.#resize = undefined;
        if (this.hasPointerCapture(event.pointerId)) this.releasePointerCapture(event.pointerId);
      },
      { signal },
    );
    this.#observer = new ResizeObserver(() => {
      if (this.#reading()) this.#paint();
      else if (this.#fitted) this.fitAll();
    });
    this.#observer.observe(this);
  }
  disconnectedCallback() {
    this.#connection?.abort();
    this.#observer?.disconnect();
    this.#pan = undefined;
    this.#resize = undefined;
    this.#space = false;
  }
  attributeChangedCallback() {
    this.#describe();
    this.fitAll();
  }
  get reading() {
    return this.hasAttribute('reading');
  }
  set reading(value) {
    if (this.reading !== !!value) this.toggleAttribute('reading', !!value);
  }
  #reading() {
    return this.reading && this.#state.artboards.length === 1;
  }
  #describe() {
    this.setAttribute(
      'aria-label',
      this.reading
        ? 'Documentation. Scroll to read. Command or Control and wheel to zoom.'
        : 'Artboards. Space and drag to pan. Shift 1 to fit all.',
    );
  }
  set state(value) {
    this.#state = value;
    const ids = new Set(value.artboards.map((b) => b.id));
    for (const [id, node] of this.#nodes)
      if (!ids.has(id)) {
        node.shell.remove();
        this.#nodes.delete(id);
      }
    for (const board of value.artboards) {
      let node = this.#nodes.get(board.id);
      if (!node) {
        const shell = document.createElement('section'),
          label = document.createElement('button'),
          slot = document.createElement('slot');
        shell.setAttribute('part', 'artboard');
        label.type = 'button';
        slot.name = board.id;
        label.addEventListener('click', () =>
          this.dispatchEvent(
            new CustomEvent('wb-artboard-select', {
              detail: { id: board.id },
              bubbles: true,
              composed: true,
            }),
          ),
        );
        label.addEventListener('dblclick', () => this.fitSelected());
        shell.append(label, slot);
        this.#world.append(shell);
        for (const edge of ['left', 'right', 'bottom', 'bottom-left', 'bottom-right']) {
          const rail = document.createElement('span');
          rail.className = 'rail';
          rail.dataset.edge = edge;
          rail.addEventListener('pointerdown', (event) => {
            event.preventDefault();
            event.stopPropagation();
            const current = this.#state.artboards.find((b) => b.id === board.id);
            if (!current) return;
            this.#fitted = false;
            this.#resize = {
              id: board.id,
              edge,
              x: event.clientX,
              y: event.clientY,
              width: current.size.width,
              height: current.size.height,
            };
            this.setPointerCapture(event.pointerId);
          });
          shell.append(rail);
        }
        node = { shell, label, slot };
        this.#nodes.set(board.id, node);
      }
      node.shell.style.cssText = `left:${board.x}px;top:${board.y}px;width:${board.size.width}px;height:${board.size.height}px`;
      node.shell.dataset.selected = String(board.id === value.selected);
      node.shell.dataset.contentFitted = String(!!board.view?.contentSize);
      node.shell.dataset.resizable = String(
        board.view?.sizes?.some((s) => s.kind === 'resizable' || s.kind === 'fit'),
      );
      node.label.setAttribute('aria-pressed', String(board.id === value.selected));
      node.label.textContent = `${artboardLabel(board)} · ${board.size.width} × ${board.size.height}${board.view?.problem ? ' · ' + board.view.problem : !board.view?.ready ? ' · Loading…' : ''}`;
    }
    this.#empty.hidden = !!ids.size;
    this.#prepare();
    if (this.#reading()) this.#paint();
    else if (this.#fitted) this.fitAll();
  }
  get state() {
    return this.#state;
  }
  set staged(boards) {
    this.#stagedBoards = boards;
    this.#prepare();
  }
  get staged() {
    return this.#stagedBoards;
  }
  #prepare() {
    const wanted = new Set();
    for (const board of this.#stagedBoards) {
      if (this.#nodes.has(board.id)) continue;
      wanted.add(board.id);
      let surface = this.#staged.get(board.id);
      if (!surface) {
        surface = document.createElement('div');
        const slot = document.createElement('slot');
        slot.name = board.id;
        surface.append(slot);
        this.#staging.append(surface);
        this.#staged.set(board.id, surface);
      }
      surface.style.cssText = `width:${board.size.width}px;height:${board.size.height}px`;
    }
    for (const [id, surface] of this.#staged) {
      if (wanted.has(id)) continue;
      surface.remove();
      this.#staged.delete(id);
    }
  }
  panReady(value) {
    this.#space = value;
    this.#panLayer.hidden = !value;
    if (value) this.focus();
  }
  wheelZoom(delta, point) {
    this.#fitted = false;
    this.#zoom(this.#camera.scale * Math.exp(-delta * 0.01), point);
  }
  fitAll() {
    this.#fitted = true;
    this.#camera = this.#reading()
      ? { x: 0, y: 0, scale: 1 }
      : fit(this.#state.artboards, {
          width: this.clientWidth,
          height: this.clientHeight,
        });
    this.#paint();
  }
  recenter() {
    if (this.#reading()) {
      this.#camera.x = centeredX({ width: this.clientWidth }, this.#camera.scale);
      this.#paint();
      return;
    }
    const boards = this.#state.artboards;
    if (!boards.length) return;
    const left = Math.min(...boards.map((b) => b.x)),
      right = Math.max(...boards.map((b) => b.x + b.size.width));
    const top = Math.min(...boards.map((b) => b.y - 32)),
      bottom = Math.max(...boards.map((b) => b.y + b.size.height));
    this.#fitted = false;
    this.#camera.x = this.clientWidth / 2 - ((left + right) / 2) * this.#camera.scale;
    this.#camera.y = this.clientHeight / 2 - ((top + bottom) / 2) * this.#camera.scale;
    this.#paint();
  }
  fitSelected() {
    if (this.#reading()) return this.fitAll();
    this.#fitted = false;
    this.#camera = fit(
      this.#state.artboards.filter((b) => b.id === this.#state.selected),
      { width: this.clientWidth, height: this.clientHeight },
    );
    this.#paint();
  }
  zoomTo(scale) {
    this.#fitted = false;
    this.#zoom(scale, { x: this.clientWidth / 2, y: this.clientHeight / 2 });
  }
  #zoom(scale, point) {
    this.#fitted = false;
    const from = this.#camera.scale;
    this.#camera = zoom(
      this.#camera,
      this.#reading() ? Math.max(0.1, this.clientHeight / 8192, scale) : scale,
      point,
    );
    this.#paint();
    if (this.#reading()) this.#scroll({ from, to: this.#camera.scale, pointY: point.y });
  }
  #scroll(detail) {
    this.dispatchEvent(
      new CustomEvent('wb-reading-scroll', { detail, bubbles: true, composed: true }),
    );
  }
  get scale() {
    return this.#camera.scale;
  }
  #paint() {
    if (this.#reading()) {
      const layout = docsLayout(
        { width: this.clientWidth, height: this.clientHeight },
        this.#camera.scale,
        this.#camera.x,
      );
      this.#camera.x = layout.x;
      this.#camera.y = 0;
      const board = this.#state.artboards[0];
      this.#nodes.get(board.id).shell.style.cssText =
        `left:0;top:0;width:${layout.width}px;height:${layout.height}px`;
      if (board.size.width !== layout.width || board.size.height !== layout.height)
        this.dispatchEvent(
          new CustomEvent('wb-reading-resize', {
            detail: { id: board.id, width: layout.width, height: layout.height },
            bubbles: true,
            composed: true,
          }),
        );
    }
    this.#world.style.setProperty('--camera-scale', String(this.#camera.scale));
    this.#world.style.transform = `translate(${this.#camera.x}px,${this.#camera.y}px) scale(${this.#camera.scale})`;
    this.dispatchEvent(
      new CustomEvent('wb-camera-change', {
        detail: { scale: this.#camera.scale },
        bubbles: true,
        composed: true,
      }),
    );
  }
}
