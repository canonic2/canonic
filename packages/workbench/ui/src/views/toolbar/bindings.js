/** Existing shell controls; annotation and review state stay in their capabilities. */
export function bindToolbar({ document, signal, selected, command, act, say, copyReference }) {
  const $ = (id) => document.getElementById(id);
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
        await copyReference();
        say('Copied reference');
      });
    },
    { signal, capture: true },
  );
}
