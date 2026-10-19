import { bounds } from '../canvas/presentation.js';

import { artboardLabel } from '../canvas/presentation.js';

export async function capture(state, runtime) {
  if (!state.artboards.length) throw new Error('Add an artboard first.');
  const reading =
    state.artboards.length === 1 &&
    !state.artboards[0].target.artboard &&
    !!state.artboards[0].view?.payload.docs;
  const box = bounds(state.artboards);
  const width = reading ? state.artboards[0].size.width : box.width + 64,
    height = reading ? state.artboards[0].size.height : box.height + 64;
  if (width > 32767 || height > 32767 || width * height > 32_000_000)
    throw new Error(
      'This canvas exceeds the 32 megapixel capture limit. Resize or close artboards before capturing.',
    );
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('Canvas image composition is unavailable.');
  context.fillStyle = reading ? '#fff' : '#242424';
  context.fillRect(0, 0, width, height);
  const artboards = [];
  try {
    // The shared compositor is deliberately queued. Each child freezes its
    // own identity and annotations before acquisition and returns an immutable image.
    for (const board of state.artboards) {
      const x = reading ? 0 : board.x + 32,
        y = reading ? 0 : board.y + 32;
      context.fillStyle = '#fff';
      context.font = '14px system-ui';
      const label = `${board.target.space.name} / ${artboardLabel(board)} · ${board.size.width} × ${board.size.height}`;
      if (!reading) context.fillText(label, board.x + 32, board.y + 22);
      let payload = {
        src: board.target.src,
        label: artboardLabel(board),
        annotations: [],
        frame: { w: board.size.width, h: board.size.height },
      };
      let at = Date.now();
      if (board.view?.ready && !board.view.problem) {
        const before = await runtime(board.id).request('inspect');

        if (!before?.ready || before.hash !== board.view.hash)
          throw new Error(`Artboard ${board.id} changed during capture. Try again.`);
        const acquired = await runtime(board.id).capture();
        payload = acquired.payload;
        at = acquired.at;
        const after = await runtime(board.id).request('inspect');

        if (payload.src !== board.view.src || !after?.ready || after.hash !== before.hash)
          throw new Error(`Artboard ${board.id} changed during capture. Try again.`);
        const image = await createImageBitmap(acquired.blob);
        try {
          context.drawImage(image, x, y, board.size.width, board.size.height);
        } finally {
          image.close();
        }
      } else {
        context.fillStyle = '#333';
        context.fillRect(x, y, board.size.width, board.size.height);
        context.fillStyle = '#fff';
        context.fillText(board.view?.problem || 'Loading preview…', x + 16, y + 32);
      }
      artboards.push({
        ...payload,
        id: board.id,
        space: board.target.space,
        status: board.view?.problem || (board.view?.ready ? 'ready' : 'loading'),
        capturedAt: at,
        region: {
          x,
          y,
          width: board.size.width,
          height: board.size.height,
        },
      });
    }
    const blob = await new Promise((resolve, reject) =>
      canvas.toBlob(
        (b) => (b ? resolve(b) : reject(new Error('Could not encode canvas image.'))),
        'image/jpeg',
        0.9,
      ),
    );
    return {
      blob,
      payload: {
        version: 2,
        canvas: state.id,
        selected: state.selected,
        artboards,
        frame: { w: width, h: height },
        label: 'Workbench canvas',
      },
    };
  } finally {
    canvas.width = canvas.height = 0;
  }
}
