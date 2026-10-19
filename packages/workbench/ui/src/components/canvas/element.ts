import type { CanvasState } from '../../canvas/model.ts';
import { fit, zoom } from '../../canvas/geometry.ts';
import type { Camera } from '../../canvas/geometry.ts';

/** Keyed artboard presentation. The application owns descriptors and renderers;
 * this element owns only its camera and connection-scoped input handlers. */
export class WbCanvas extends HTMLElement {
  #root = this.attachShadow({ mode: 'open' });
  #world = document.createElement('div');
  #empty = document.createElement('p');
  #panLayer = document.createElement('div');
  #nodes = new Map<string, { shell: HTMLElement; label: HTMLButtonElement; slot: HTMLSlotElement }>();
  #state: CanvasState = { id: '', revision: 0, selected: null, artboards: [] };
  #camera: Camera = { x: 32, y: 32, scale: 1 };
  #fitted = true;
  #connection?: AbortController;
  #observer?: ResizeObserver;
  #pan?: { x: number; y: number; camera: Camera };
  #space = false;
  #resize?: { id: string; edge: string; x: number; y: number; width: number; height: number };
  constructor() {
    super();
    const style = document.createElement('style');
    style.textContent = `
      :host{display:block;position:relative;overflow:hidden;min-width:0;min-height:0;background:transparent;color:var(--wb-fg);outline:none;touch-action:none}
      .world{position:absolute;inset:0 auto auto 0;transform-origin:0 0}
      section{position:absolute;box-shadow:0 0 0 1px var(--wb-line);background:var(--wb-raised)}
      section[data-content-fitted=true]{background:transparent;box-shadow:none}
      section[data-selected=true]{box-shadow:0 0 0 2px var(--wb-accent)}
      button{position:absolute;bottom:100%;left:0;width:100%;height:calc(28px / var(--camera-scale,1));padding:0;border:0;background:transparent;color:var(--wb-fg-2);font:calc(12px / var(--camera-scale,1)) var(--wb-ui-font);text-align:left;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;cursor:pointer}
      button:focus-visible{outline:2px solid var(--wb-focus)}
      section[data-selected=true] button{color:var(--wb-fg)}
      ::slotted(iframe){display:block;width:100%;height:100%;border:0}
      p{position:absolute;inset:40% 0 auto;text-align:center;color:var(--wb-fg-2);font:14px var(--wb-ui-font)}
      .pan{position:absolute;inset:0;z-index:10;cursor:grabbing}
      .pan[hidden]{display:none}
      .rail{position:absolute;z-index:2;display:none}
      section[data-selected=true][data-resizable=true] .rail{display:block}
      .rail[data-edge=left]{left:-5px;top:0;bottom:0;width:5px;cursor:ew-resize}
      .rail[data-edge=right]{right:-5px;top:0;bottom:0;width:5px;cursor:ew-resize}
      .rail[data-edge=bottom]{left:0;right:0;bottom:-5px;height:5px;cursor:ns-resize}
      .rail[data-edge=bottom-left]{left:-5px;bottom:-5px;width:10px;height:10px;cursor:nesw-resize}
      .rail[data-edge=bottom-right]{right:-5px;bottom:-5px;width:10px;height:10px;cursor:nwse-resize}
    `;
    this.#world.className = 'world';
    this.#empty.textContent = 'Pick a page or a component to see it here.';
    this.#panLayer.className = 'pan'; this.#panLayer.hidden = true;
    this.#root.append(style, this.#empty, this.#world, this.#panLayer);
  }
  connectedCallback() {
    if (Object.hasOwn(this, 'state')) { const value = this.state; delete (this as unknown as Record<string, unknown>).state; this.state = value; }
    this.#connection?.abort();
    const signal = (this.#connection = new AbortController()).signal;
    this.tabIndex = 0;
    this.setAttribute('aria-label', 'Artboards. Space and drag to pan. Shift 1 to fit all.');
    this.addEventListener('wheel', event => {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        const r = this.getBoundingClientRect();
        this.#camera = zoom(this.#camera, this.#camera.scale * Math.exp(-event.deltaY * .01), { x: event.clientX - r.left, y: event.clientY - r.top });
      } else { this.#camera.x -= event.deltaX; this.#camera.y -= event.deltaY; }
      this.#fitted = false; this.#paint();
    }, { passive: false, signal });
    this.addEventListener('keydown', event => {
      if (event.target !== this) return;
      if (event.code === 'Space') { event.preventDefault(); this.panReady(true); }
      if (event.shiftKey && event.key === '1') { event.preventDefault(); this.fitAll(); }
      if (event.shiftKey && event.key === '2') { event.preventDefault(); this.fitSelected(); }
      if ((event.metaKey || event.ctrlKey) && ['0', '=', '+', '-'].includes(event.key)) {
        event.preventDefault();
        this.zoomTo(event.key === '0' ? 1 : this.#camera.scale * (event.key === '-' ? .8 : 1.25));
      }
    }, { signal });
    window.addEventListener('keyup', event => { if (event.code === 'Space') this.panReady(false); }, { signal });
    window.addEventListener('blur', () => { this.panReady(false); this.#pan = undefined; }, { signal });
    this.addEventListener('pointerdown', event => {
      if (event.button !== 1 && !this.#space && event.composedPath()[0] !== this && event.composedPath()[0] !== this.#world) return;
      event.preventDefault(); this.focus(); this.setPointerCapture(event.pointerId);
      this.dispatchEvent(new CustomEvent('wb-artboard-deselect', { bubbles: true, composed: true }));
      this.#pan = { x: event.clientX, y: event.clientY, camera: { ...this.#camera } };
    }, { signal });
    this.addEventListener('pointermove', event => {
      if (this.#resize) {
        const drag = this.#resize, dx = (event.clientX - drag.x) / this.#camera.scale, dy = (event.clientY - drag.y) / this.#camera.scale;
        const width = drag.edge === 'bottom' ? drag.width : Math.min(8192, Math.max(1, Math.round(drag.width + (drag.edge.includes('left') ? -dx : dx))));
        const height = drag.edge.includes('bottom') ? Math.min(8192, Math.max(1, Math.round(drag.height + dy))) : drag.height;
        this.dispatchEvent(new CustomEvent('wb-artboard-resize', { detail: { id: drag.id, width, height }, bubbles: true, composed: true }));
        return;
      }
      if (!this.#pan) return;
      this.#camera = { ...this.#pan.camera, x: this.#pan.camera.x + event.clientX - this.#pan.x, y: this.#pan.camera.y + event.clientY - this.#pan.y };
      this.#fitted = false; this.#paint();
    }, { signal });
    this.addEventListener('lostpointercapture', () => { this.#pan = undefined; this.#resize = undefined; }, { signal });
    this.addEventListener('pointerup', event => { this.#pan = undefined; this.#resize = undefined; if (this.hasPointerCapture(event.pointerId)) this.releasePointerCapture(event.pointerId); }, { signal });
    this.#observer = new ResizeObserver(() => { if (this.#fitted) this.fitAll(); });
    this.#observer.observe(this);
  }
  disconnectedCallback() { this.#connection?.abort(); this.#observer?.disconnect(); this.#pan = undefined; this.#resize = undefined; this.#space = false; }
  set state(value: CanvasState) {
    this.#state = value;
    const ids = new Set(value.artboards.map(b => b.id));
    for (const [id, node] of this.#nodes) if (!ids.has(id)) { node.shell.remove(); this.#nodes.delete(id); }
    for (const board of value.artboards) {
      let node = this.#nodes.get(board.id);
      if (!node) {
        const shell = document.createElement('section'), label = document.createElement('button'), slot = document.createElement('slot');
        shell.setAttribute('part', 'artboard'); label.type = 'button'; slot.name = board.id;
        label.addEventListener('click', () => this.dispatchEvent(new CustomEvent('wb-artboard-select', { detail: { id: board.id }, bubbles: true, composed: true })));
        label.addEventListener('dblclick', () => this.fitSelected());
        shell.append(label, slot); this.#world.append(shell);
        for (const edge of ['left', 'right', 'bottom', 'bottom-left', 'bottom-right']) {
          const rail = document.createElement('span'); rail.className = 'rail'; rail.dataset.edge = edge;
          rail.addEventListener('pointerdown', event => {
            event.preventDefault(); event.stopPropagation();
            const current = this.#state.artboards.find(b => b.id === board.id); if (!current) return;
            this.#fitted = false;
            this.#resize = { id: board.id, edge, x: event.clientX, y: event.clientY, width: current.size.width, height: current.size.height };
            this.setPointerCapture(event.pointerId);
          });
          shell.append(rail);
        }
        node = { shell, label, slot }; this.#nodes.set(board.id, node);
      }
      node.shell.style.cssText = `left:${board.x}px;top:${board.y}px;width:${board.size.width}px;height:${board.size.height}px`;
      node.shell.dataset.selected = String(board.id === value.selected);
      node.shell.dataset.contentFitted = String(!!board.view?.contentSize);
      node.shell.dataset.resizable = String(board.view?.sizes?.some(s => s.kind === 'resizable' || s.kind === 'fit'));
      node.label.setAttribute('aria-pressed', String(board.id === value.selected));
      node.label.textContent = `${board.view?.label || board.target.src} · ${board.size.width} × ${board.size.height}${board.view?.problem ? ' · ' + board.view.problem : !board.view?.ready ? ' · Loading…' : ''}`;
    }
    this.#empty.hidden = !!ids.size;
    if (this.#fitted) this.fitAll();
  }
  get state() { return this.#state; }
  panReady(value: boolean) { this.#space = value; this.#panLayer.hidden = !value; if (value) this.focus(); }
  wheelZoom(delta: number, point: { x: number; y: number }) { this.#fitted = false; this.#camera = zoom(this.#camera, this.#camera.scale * Math.exp(-delta * .01), point); this.#paint(); }
  fitAll() { this.#fitted = true; this.#camera = fit(this.#state.artboards, { width: this.clientWidth, height: this.clientHeight }); this.#paint(); }
  recenter() {
    const boards = this.#state.artboards;
    if (!boards.length) return;
    const left = Math.min(...boards.map(b => b.x)), right = Math.max(...boards.map(b => b.x + b.size.width));
    const top = Math.min(...boards.map(b => b.y - 32)), bottom = Math.max(...boards.map(b => b.y + b.size.height));
    this.#fitted = false;
    this.#camera.x = this.clientWidth / 2 - (left + right) / 2 * this.#camera.scale;
    this.#camera.y = this.clientHeight / 2 - (top + bottom) / 2 * this.#camera.scale;
    this.#paint();
  }
  fitSelected() { this.#fitted = false; this.#camera = fit(this.#state.artboards.filter(b => b.id === this.#state.selected), { width: this.clientWidth, height: this.clientHeight }); this.#paint(); }
  zoomTo(scale: number) { this.#fitted = false; this.#camera = zoom(this.#camera, scale, { x: this.clientWidth / 2, y: this.clientHeight / 2 }); this.#paint(); }
  get scale() { return this.#camera.scale; }
  #paint() {
    this.#world.style.setProperty('--camera-scale', String(this.#camera.scale));
    this.#world.style.transform = `translate(${this.#camera.x}px,${this.#camera.y}px) scale(${this.#camera.scale})`;
    this.dispatchEvent(new CustomEvent('wb-camera-change', { detail: { scale: this.#camera.scale }, bubbles: true, composed: true }));
  }
}
