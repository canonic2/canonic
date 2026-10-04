/** Preserve complete canvas reports, while retaining the single-view shape. */
export function cleanView(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== 'object') return null;
  const v = value as Record<string, unknown>;
  if (typeof v.text !== 'string' || !v.text.trim()) return null;
  if (v.text.length > 100000) throw new Error('Canvas context exceeds 100000 characters; no artboards were truncated.');
  const out: Record<string, unknown> = { text: v.text };
  for (const key of ['src','state','story','lens']) if (typeof v[key] === 'string' && v[key]) out[key] = v[key];
  if (v.version === 2) {
    if (typeof v.canvas !== 'string' || !Array.isArray(v.artboards) || v.artboards.length > 32) throw new Error('Invalid canvas context.');
    const ids = new Set<string>();
    out.artboards = v.artboards.map((entry: unknown) => {
      if (!entry || typeof entry !== 'object') throw new Error('Invalid artboard context.');
      const b = entry as Record<string, unknown>, space = b.space as Record<string, unknown>, size = b.size as Record<string, unknown>;
      if (typeof b.id !== 'string' || ids.has(b.id) || typeof b.src !== 'string' || !space || typeof space.id !== 'string' || typeof space.name !== 'string' || !size || typeof size.width !== 'number' || typeof size.height !== 'number' || ![size.width,size.height].every(Number.isFinite)) throw new Error('Invalid artboard context.');
      ids.add(b.id);
      return { id: b.id, space: { id: space.id, name: space.name }, src: b.src, state: typeof b.state === 'string' ? b.state : null,
        lens: typeof b.lens === 'string' ? b.lens : null, size: { width: size.width, height: size.height }, status: typeof b.status === 'string' ? b.status : 'loading' };
    });
    if (v.selected !== null && (typeof v.selected !== 'string' || !ids.has(v.selected))) throw new Error('Invalid selected artboard.');
    Object.assign(out, { version: 2, canvas: v.canvas, selected: v.selected });
  }
  return out;
}
