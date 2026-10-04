/** Serializable canvas state. Renderer handles and transport never enter it. */
export interface Space { id: string; name: string; url: string; root?: string }
export interface Target { space: Space; src: string; state: string | null; lens: string | null; example?: string | null }
export interface Size { width: number; height: number }
export interface ViewInfo {
  src: string; state: string | null; lens: string | null; label: string;
  reference: string; hash: string; ready: boolean; problem: string | null;
  payload: Record<string, unknown>; states: { id: string; label: string; current?: boolean }[];
  lenses: { id: string; label: string }[];
  tool?: string;
  sizes?: string[];
}
export interface Artboard { id: string; target: Target; size: Size; x: number; y: number; generation: number; view: ViewInfo | null }
export interface CanvasState { id: string; revision: number; selected: string | null; artboards: readonly Artboard[] }
export const MAX_ARTBOARDS = 32;
export function size(width: number, height: number): Size {
  if (![width, height].every(n => Number.isFinite(n) && n >= 320 && n <= 8192)) throw new Error('Artboard dimensions must be between 320 and 8192 CSS pixels.');
  return { width: Math.round(width), height: Math.round(height) };
}
export function supportsSize(dimensions: Size, modes?: readonly string[]): boolean {
  if (!modes || modes.includes('resizable') || modes.includes('fit')) return true;
  return (modes.includes('1512') && dimensions.width === 1512 && dimensions.height === 982)
    || (modes.includes('393') && dimensions.width === 393 && dimensions.height === 852);
}
export function createState(id: string): CanvasState { return { id, revision: 0, selected: null, artboards: [] }; }
export function add(state: CanvasState, id: string, target: Target, dimensions: Size): CanvasState {
  if (!id || state.artboards.some(b => b.id === id)) throw new Error('Artboard IDs must be unique.');
  if (state.artboards.length >= MAX_ARTBOARDS) throw new Error(`A canvas supports up to ${MAX_ARTBOARDS} artboards.`);
  if (!target.space.id || !target.src) throw new Error('An artboard needs a space and page.');
  return arrange({ ...state, revision: state.revision + 1, selected: id, artboards: [...state.artboards,
    { id, target, size: size(dimensions.width, dimensions.height), x: 0, y: 0, generation: 1, view: null }] });
}
export function arrange(state: CanvasState): CanvasState {
  let x = 0;
  return { ...state, artboards: state.artboards.map(b => { const next = { ...b, x, y: 32 }; x += b.size.width + 48; return next; }) };
}
export function select(state: CanvasState, id: string): CanvasState {
  if (!state.artboards.some(b => b.id === id)) throw new Error('That artboard is no longer open.');
  return { ...state, selected: id, revision: state.revision + 1 };
}
export function change(state: CanvasState, id: string, update: Partial<Pick<Artboard, 'target' | 'size' | 'view' | 'generation'>>): CanvasState {
  if (!state.artboards.some(b => b.id === id)) throw new Error('That artboard is no longer open.');
  return arrange({ ...state, revision: state.revision + 1, artboards: state.artboards.map(b => b.id === id ? { ...b, ...update } : b) });
}
export function remove(state: CanvasState, id: string): CanvasState {
  const at = state.artboards.findIndex(b => b.id === id);
  if (at < 0) return state;
  const artboards = state.artboards.filter(b => b.id !== id);
  return arrange({ ...state, revision: state.revision + 1, artboards,
    selected: state.selected === id ? (artboards[at] || artboards[at - 1])?.id || null : state.selected });
}
