import { WbCanvas } from '../ui/src/components/canvas/element.ts';
import { add, createState, select } from '../ui/src/canvas/model.ts';
if (!customElements.get('wb-canvas')) customElements.define('wb-canvas', WbCanvas);
function example(container: HTMLElement, count: number) {
  const canvas = document.createElement('wb-canvas') as WbCanvas;
  canvas.style.cssText = 'display:block;height:360px;width:100%';
  let state = createState('example');
  for (let i = 0; i < count; i++) {
    const id = 'example-' + i;
    state = add(state, id, { space: { id: 'acme', name: 'Acme', url: '' }, src: i ? 'Success' : 'Info', state: null, lens: null }, { width: 320, height: 180 });
    const frame = document.createElement('iframe'); frame.slot = id; frame.title = i ? 'Success' : 'Info';
    frame.srcdoc = `<html><body style="font:16px system-ui;padding:24px;background:${i ? '#eefbf2' : '#eff6ff'}"><h3>${i ? 'Import complete' : 'Scheduled maintenance'}</h3><p>${i ? 'Your workspace is ready.' : 'Sync may pause on Sunday.'}</p></body></html>`;
    canvas.append(frame);
  }
  canvas.state = state;
  canvas.addEventListener('wb-artboard-select', event => { state = select(state, (event as CustomEvent).detail.id); canvas.state = state; });
  container.append(canvas);
}
export const empty = (container: HTMLElement) => example(container, 0);
export const single = (container: HTMLElement) => example(container, 1);
export const multiple = (container: HTMLElement) => example(container, 2);
