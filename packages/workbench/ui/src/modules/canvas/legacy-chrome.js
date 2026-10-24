/** Compatibility adapter for existing canvas controls. No navigation or retention state. */

export function bindLegacyChrome(ports) {
  const { canvas, document, signal, selected, command, act, activityChanged, say } = ports;
  const $ = (id) => document.getElementById(id);
  ports.previewControls.connect({ send: (value) => act(() => command('preview', value)) });
  function camera(key) {
    if (key === 'fit' || key === 'fit-all') canvas.fitAll();
    else if (key === 'fit-selected') canvas.fitSelected();
    else if (key === 'actual' || key === '100') canvas.zoomTo(1);
    else
      canvas.zoomTo(
        Math.pow(
          2,
          key === 'out'
            ? Math.ceil(Math.log2(canvas.scale) - 0.000001) - 1
            : Math.floor(Math.log2(canvas.scale) + 0.000001) + 1,
        ),
      );
  }
  canvas.addEventListener(
    'wb-camera-change',
    () => {
      $('zoomValue').textContent = Math.round(canvas.scale * 100) + '%';
    },
    { signal },
  );
  const oldZoom = ports.zoom.current;
  ports.zoom.current = {
    ...oldZoom,
    scale: () => canvas.scale,
    fit: ports.fit || (() => canvas.fitAll()),
    key(event) {
      const key = oldZoom.command(event);
      if (!key) return false;
      event.preventDefault();
      camera(key);
      return true;
    },
  };
  $('zoomCenter').addEventListener('click', () => canvas.recenter(), { signal });
  for (const [id, key] of [
    ['zoomOut', 'out'],
    ['zoomIn', 'in'],
  ])
    $(id).addEventListener('click', () => camera(key), { signal });
  $('zoomValue').addEventListener(
    'click',
    () => {
      const menu = $('zoomMenu');
      menu.hidden = !menu.hidden;
      $('zoomValue').setAttribute('aria-expanded', String(!menu.hidden));
      menu.replaceChildren(
        ...[
          ['Zoom to fit', 0],
          ['100%', 1],
          ['50%', 0.5],
          ['200%', 2],
        ].map(([label, scale]) => {
          const button = document.createElement('button');
          button.className = 'wb-menu-row';
          button.type = 'button';
          button.setAttribute('role', 'menuitem');
          button.textContent = String(label);
          button.addEventListener('click', () => {
            if (scale) canvas.zoomTo(Number(scale));
            else canvas.fitAll();
            menu.hidden = true;
            $('zoomValue').setAttribute('aria-expanded', 'false');
          });
          return button;
        }),
      );
    },
    { signal },
  );
  document.addEventListener(
    'keydown',
    (event) => {
      const node = event.composedPath()[0];
      if (node?.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(node?.tagName)) return;
      if (ports.zoom.current.key(event)) event.stopImmediatePropagation();
      else if (event.key === 'Escape') {
        act(() => command('tool', 'pointer'));
        $('zoomMenu').hidden = true;
      } else if ((event.metaKey || event.ctrlKey) && event.key === 'z') {
        event.preventDefault();
        act(() => command('undo'));
      }
    },
    { signal, capture: true },
  );
  document.addEventListener('pointerdown', activityChanged, { signal, capture: true });
  document.addEventListener(
    'pointerdown',
    (event) => {
      if (!event.target.closest('#viewControls')) $('zoomMenu').hidden = true;
    },
    { signal },
  );
  for (const button of document.querySelectorAll('[data-tool]'))
    button.addEventListener(
      'click',
      () =>
        act(() =>
          command(
            'tool',
            selected()?.view?.tool === button.dataset.tool ? 'pointer' : button.dataset.tool,
          ),
        ),
      { signal },
    );
  $('shapeCaret').addEventListener(
    'click',
    () => {
      $('shapeMenu').hidden = !$('shapeMenu').hidden;
    },
    { signal },
  );
  document.querySelectorAll('[data-shape]').forEach((button) =>
    button.addEventListener(
      'click',
      () => {
        const shape = button.dataset.shape;
        $('shapeTool').dataset.tool = shape;
        $('shapeTool').setAttribute('aria-label', button.getAttribute('aria-label'));
        $('shapeTool').replaceChildren(button.firstElementChild.cloneNode(true));
        $('shapeMenu').hidden = true;
        act(() => command('tool', shape));
      },
      { signal },
    ),
  );
  const initialShape = document.querySelector('[data-shape=rect]')?.firstElementChild;
  if (initialShape) $('shapeTool').replaceChildren(initialShape.cloneNode(true));
  for (const [id, name] of [
    ['undo', 'undo'],
    ['clearAnnotations', 'clear'],
    ['reload', 'reload'],
  ])
    $(id).addEventListener(
      'click',
      (event) => {
        event.stopImmediatePropagation();
        act(() => command(name));
      },
      { signal, capture: true },
    );
  $('copyReference').addEventListener(
    'click',
    (event) => {
      event.stopImmediatePropagation();
      act(async () => {
        await ports.copyReference();
        say('Copied reference');
      });
    },
    { signal, capture: true },
  );

  signal.addEventListener(
    'abort',
    () => {
      ports.zoom.current = oldZoom;
    },
    { once: true },
  );
  return { camera };
}
