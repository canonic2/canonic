import { WbCanvas } from '../ui/src/components/canvas/element.js';
import { add, change, createState, select } from '../ui/src/modules/canvas/model.js';
import { scrollAfterZoom } from '../ui/src/modules/docs/canvas/docs-layout.js';
if (!customElements.get('wb-canvas')) customElements.define('wb-canvas', WbCanvas);
function example(container: HTMLElement, count: number, pending = false, reading = false) {
  const canvas = document.createElement('wb-canvas') as WbCanvas;
  canvas.reading = reading;
  canvas.style.cssText = 'display:block;height:360px;width:100%';
  let state = createState('example');
  for (let i = 0; i < count; i++) {
    const id = 'example-' + i;
    const label = reading ? 'Documentation' : 'Feedback / ' + (i ? 'Success' : 'Info');
    state = add(state, id, { space: { id: 'acme', name: 'Acme', url: '' }, src: 'feedback.workbench.ts', label, state: null, lens: null }, { width: 320, height: 180 });
    const frame = document.createElement('iframe'); frame.slot = id; frame.title = label;
    if (pending) frame.style.visibility = 'hidden';
    else {
      frame.srcdoc = reading
        ? `<html><body style="margin:0;font:16px system-ui;background:white"><main style="max-width:600px;margin:auto;padding:32px"><h1>Documentation</h1><p>A reading surface fills the canvas without an artboard label or selection border.</p>${'<p>Scroll to continue reading.</p>'.repeat(12)}</main></body></html>`
        : `<html><body style="font:16px system-ui;padding:24px;background:${i ? '#eefbf2' : '#eff6ff'}"><h3>${i ? 'Import complete' : 'Scheduled maintenance'}</h3><p>${i ? 'Your workspace is ready.' : 'Sync may pause on Sunday.'}</p></body></html>`;
      state = change(state, id, { view: { src: 'feedback.workbench.ts', state: null, lens: null, label, ready: true, problem: null, hash: '', reference: '', payload: {}, states: [], lenses: [] } });
    }
    canvas.append(frame);
  }
  canvas.state = state;
  canvas.addEventListener('wb-reading-resize', event => {
    const { id, width, height } = (event as CustomEvent).detail;
    state = change(state, id, { size: { width, height } }); canvas.state = state;
  });
  canvas.addEventListener('wb-reading-scroll', event => {
    const page = canvas.querySelector('iframe')?.contentWindow;
    if (!page) return;
    const value = (event as CustomEvent).detail;
    if (Number.isFinite(value.dy)) page.scrollBy(0, value.dy);
    else page.scrollTo(page.scrollX, scrollAfterZoom(page.scrollY, value.pointY, value.from, value.to));
  });
  canvas.addEventListener('wb-artboard-select', event => { state = select(state, (event as CustomEvent).detail.id); canvas.state = state; });
  container.append(canvas);
}
export const empty = (container: HTMLElement) => example(container, 0);
export const loading = (container: HTMLElement) => example(container, 1, true);
export const single = (container: HTMLElement) => example(container, 1);
export const multiple = (container: HTMLElement) => example(container, 2);
export const reading = (container: HTMLElement) => example(container, 1, false, true);
