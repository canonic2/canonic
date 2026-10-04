import { bounds } from './geometry.ts';
import type { CanvasState } from './model.ts';
import type { Runtime } from './runtime.ts';
export function reference(state: CanvasState): string {
  const lines = [`Workbench canvas: ${state.artboards.length} artboard(s)`, `Selected artboard: ${state.selected || 'none'}`];
  for (const b of state.artboards) {
    lines.push('', `Artboard ${b.id} — ${b.target.space.name}`, `- Size: ${b.size.width} × ${b.size.height} CSS px`,
      b.view?.reference || `- Page: ${b.target.src}\n- State: ${b.target.state || 'default'}\n- Lens: ${b.target.lens || 'Design'}`,
      `- Status: ${b.view?.problem || (b.view?.ready ? 'Ready' : 'Loading')}`);
  }
  return lines.join('\n');
}
export function report(state: CanvasState) {
  return { version: 2, canvas: state.id, selected: state.selected, text: reference(state), artboards: state.artboards.map(b => ({
    id: b.id, space: { id: b.target.space.id, name: b.target.space.name }, src: b.target.src,
    state: b.target.state, lens: b.target.lens, size: b.size, status: b.view?.problem || (b.view?.ready ? 'ready' : 'loading'),
  })) };
}
export async function capture(state: CanvasState, runtime: (id: string) => Runtime) {
  if (!state.artboards.length) throw new Error('Add an artboard first.');
  const box = bounds(state.artboards);
  const width = box.width + 64, height = box.height + 64;
  if (width > 32767 || height > 32767 || width * height > 32_000_000) throw new Error('This canvas exceeds the 32 megapixel capture limit. Resize or close artboards before capturing.');
  const canvas = document.createElement('canvas'); canvas.width = width; canvas.height = height;
  const context = canvas.getContext('2d'); if (!context) throw new Error('Canvas image composition is unavailable.');
  context.fillStyle = '#242424'; context.fillRect(0, 0, width, height);
  const artboards: Record<string, unknown>[] = [];
  try {
  // The shared compositor is deliberately queued. Each child freezes its
  // own identity and annotations before acquisition and returns an immutable image.
  for (const board of state.artboards) {
    context.fillStyle = '#fff'; context.font = '14px system-ui';
    const label = `${board.target.space.name} / ${board.view?.label || board.target.src} · ${board.size.width} × ${board.size.height}`;
    context.fillText(label, board.x + 32, board.y + 22);
    let payload: Record<string, unknown> = { src: board.target.src, label: board.view?.label || board.target.src, annotations: [], frame: { w: board.size.width, h: board.size.height } };
    let at = Date.now();
    if (board.view?.ready && !board.view.problem) {
      const before = await runtime(board.id).request('inspect') as { hash?: string; ready?: boolean } | null;
      if (!before?.ready || before.hash !== board.view.hash) throw new Error(`Artboard ${board.id} changed during capture. Try again.`);
      const acquired = await runtime(board.id).capture(); payload = acquired.payload; at = acquired.at;
      const after = await runtime(board.id).request('inspect') as { hash?: string; ready?: boolean } | null;
      if (payload.src !== board.view.src || !after?.ready || after.hash !== before.hash) throw new Error(`Artboard ${board.id} changed during capture. Try again.`);
      const image = await createImageBitmap(acquired.blob);
      try { context.drawImage(image, board.x + 32, board.y + 32, board.size.width, board.size.height); } finally { image.close(); }
    } else {
      context.fillStyle = '#333'; context.fillRect(board.x + 32, board.y + 32, board.size.width, board.size.height);
      context.fillStyle = '#fff'; context.fillText(board.view?.problem || 'Loading preview…', board.x + 48, board.y + 64);
    }
    artboards.push({ ...payload, id: board.id, space: board.target.space, status: board.view?.problem || (board.view?.ready ? 'ready' : 'loading'), capturedAt: at,
      region: { x: board.x + 32, y: board.y + 32, width: board.size.width, height: board.size.height } });
  }
  const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob(b => b ? resolve(b) : reject(new Error('Could not encode canvas image.')), 'image/jpeg', 0.9));
  return { blob, payload: { version: 2, canvas: state.id, selected: state.selected, artboards, frame: { w: width, h: height }, label: 'Workbench canvas' } };
  } finally { canvas.width = canvas.height = 0; }
}
