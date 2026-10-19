export function reference(state) {
  const lines = [
    `Workbench canvas: ${state.artboards.length} artboard(s)`,
    `Selected artboard: ${state.selected || 'none'}`,
  ];
  for (const b of state.artboards) {
    lines.push(
      '',
      `Artboard ${b.id} — ${b.target.space.name}`,
      `- Size: ${b.size.width} × ${b.size.height} CSS px`,
      b.view?.reference ||
        `- Page: ${b.target.label ? b.target.label + ' — ' : ''}${b.target.src}\n- State: ${b.target.state || 'default'}\n- Lens: ${b.target.lens || 'Design'}`,
      `- Status: ${b.view?.problem || (b.view?.ready ? 'Ready' : 'Loading')}`,
    );
  }
  return lines.join('\n');
}
export function report(state) {
  return {
    version: 2,
    canvas: state.id,
    selected: state.selected,
    text: reference(state),
    artboards: state.artboards.map((b) => ({
      id: b.id,
      space: { id: b.target.space.id, name: b.target.space.name },
      src: b.target.src,
      state: b.target.state,
      lens: b.target.lens,
      size: b.size,
      status: b.view?.problem || (b.view?.ready ? 'ready' : 'loading'),
      ...(b.target.artboard ? { artboard: b.target.artboard } : {}),
    })),
  };
}
