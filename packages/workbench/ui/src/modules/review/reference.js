// Printed like Copy reference: the label, else the id, then the id.
function iterationLine(target) {
  if (!target.iteration) return '';
  return `\n- Iteration: ${target.iterationLabel || target.iteration} — \`${target.iteration}\``;
}
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
        `- Page: ${b.target.label ? b.target.label + ' — ' : ''}${b.target.src}\n- Scenario: ${b.target.scenario || 'default'}${iterationLine(b.target)}\n- Lens: ${b.target.lens || 'Design'}`,
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
      scenario: b.target.scenario,
      lens: b.target.lens,
      size: b.size,
      status: b.view?.problem || (b.view?.ready ? 'ready' : 'loading'),
      ...(b.target.artboard ? { artboard: b.target.artboard } : {}),
      ...(b.target.iteration ? { iteration: b.target.iteration } : {}),
    })),
  };
}
