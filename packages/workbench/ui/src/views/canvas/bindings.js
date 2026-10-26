/** Connect supplied canvas controls to their owners; the document owns creation. */
export function bindCanvas({ canvas, room, shoot, handoff }, actions, { signal }) {
  canvas.addEventListener('wb-artboard-select', (event) => actions.select(event.detail.id), {
    signal,
  });
  canvas.addEventListener('wb-artboard-resize', (event) => actions.resize(event.detail), {
    signal,
  });
  canvas.addEventListener('wb-artboard-deselect', actions.deselect, { signal });
  canvas.addEventListener('focusin', actions.focus, { signal });
  canvas.addEventListener('wb-reading-resize', (event) => actions.readingResize(event.detail), {
    signal,
  });
  canvas.addEventListener('wb-reading-scroll', (event) => actions.readingScroll(event.detail), {
    signal,
  });
  shoot.addEventListener('click', actions.shoot, { signal });
  handoff.addEventListener('click', actions.handoff, { signal });
  if (signal.aborted) return;
  const observer = new ResizeObserver(actions.refill);
  observer.observe(room);
  signal.addEventListener('abort', () => observer.disconnect(), { once: true });
}
