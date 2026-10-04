export interface CanvasReview { file: string; selected: string | null; artboards: Record<string, unknown>[] }
export function canvasPrompt(value: unknown, singlePrompt: (payload: Record<string, unknown>) => string): string {
  if (!value || typeof value !== 'object') throw new Error('Invalid canvas review.');
  const review = value as CanvasReview;
  if (typeof review.file !== 'string' || !review.file || !Array.isArray(review.artboards) || !review.artboards.length || review.artboards.length > 32) throw new Error('A canvas handoff needs its image and artboards.');
  const lines = ['Here is a Workbench canvas review.', '', `- Screenshot: \`${review.file}\``, `- Selected artboard: ${review.selected || 'none'}`,
    'Each labelled image region is an artboard. Annotation coordinates below are local to that artboard, in CSS pixels.'];
  const ids = new Set<string>();
  for (const board of review.artboards) {
    const space = board.space as { name?: unknown; id?: unknown };
    const region = board.region as { x?: unknown; y?: unknown; width?: unknown; height?: unknown };
    if (typeof board.id !== 'string' || !board.id || ids.has(board.id) || typeof board.src !== 'string' || !space || typeof space.id !== 'string' || typeof space.name !== 'string' || !region || ![region.x,region.y,region.width,region.height].every(n => typeof n === 'number' && Number.isFinite(n) && n >= 0) || region.width === 0 || region.height === 0) throw new Error('Invalid artboard in canvas review.');
    ids.add(board.id);
    lines.push('', `Artboard ${board.id} — Space: ${space.name} (${space.id})`, `- Image region: (${region.x}, ${region.y}), ${region.width} × ${region.height}`,
      `- Status: ${board.status || 'ready'}`, `- Captured at: ${board.capturedAt}`, singlePrompt({ ...board, file: review.file, width: `${region.width} × ${region.height}` }));
  }
  if (review.selected !== null && !ids.has(review.selected)) throw new Error('Invalid selected artboard in canvas review.');
  return lines.join('\n');
}
