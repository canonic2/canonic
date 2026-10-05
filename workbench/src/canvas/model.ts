/** Serializable canvas state. Renderer handles and transport never enter it. */
import { accepts, validDimensions } from '../sizes/browser/geometry.ts';
import { MAX_LENGTH, MIN_LENGTH } from '../sizes/browser/size.ts';
import type { ResolvedSize } from '../sizes/browser/size.ts';
export interface Space { id: string; name: string; url: string; root?: string }
export interface Target { space: Space; src: string; state: string | null; lens: string | null; example?: string | null }
export interface Size { width: number; height: number }
export interface ViewInfo {
  src: string; state: string | null; lens: string | null; label: string;
  reference: string; hash: string; ready: boolean; problem: string | null;
  payload: Record<string, unknown>; states: { id: string; label: string; current?: boolean }[];
  lenses: { id: string; label: string }[];
  tool?: string;
  /** The sizes the page supports, as the space resolves them; absent on a docs page. */
  sizes?: ResolvedSize[];
}
export interface Artboard { id: string; target: Target; size: Size; x: number; y: number; generation: number; view: ViewInfo | null }
export interface CanvasState { id: string; revision: number; selected: string | null; artboards: readonly Artboard[] }
export const MAX_ARTBOARDS = 32;
export function size(width: number, height: number): Size {
  if (!validDimensions({ width, height })) throw new Error(`Artboard dimensions must be between ${MIN_LENGTH} and ${MAX_LENGTH} CSS pixels.`);
  return { width: Math.round(width), height: Math.round(height) };
}
/** Whether a page that supports `sizes` can be shown at `dimensions`; every size fits a page that names none. */
export function supportsSize(dimensions: Size, sizes?: readonly ResolvedSize[]): boolean {
  return !sizes || sizes.some(candidate => accepts(candidate, dimensions));
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
