/** Serializable canvas state. Renderer handles and transport never enter it. */
import { accepts, validDimensions } from '../../../../contracts/src/sizes/browser/geometry.js';
import { MAX_LENGTH, MIN_LENGTH } from '../../../../contracts/src/sizes/browser/size.js';

export function artboardLabel(board) {
  return board.view?.label || board.target.label || board.target.src;
}
export const MAX_ARTBOARDS = 32;
export function size(width, height) {
  if (!validDimensions({ width, height }))
    throw new Error(
      `Artboard dimensions must be between ${MIN_LENGTH} and ${MAX_LENGTH} CSS pixels.`,
    );
  return { width: Math.round(width), height: Math.round(height) };
}
/** Whether a page that supports `sizes` can be shown at `dimensions`; every size fits a page that names none. */
export function supportsSize(dimensions, sizes) {
  return !sizes || sizes.some((candidate) => accepts(candidate, dimensions));
}
export function createState(id) {
  return { id, revision: 0, selected: null, artboards: [] };
}
export function add(state, id, target, dimensions) {
  if (!id || state.artboards.some((b) => b.id === id))
    throw new Error('Artboard IDs must be unique.');
  if (state.artboards.length >= MAX_ARTBOARDS)
    throw new Error(`A canvas supports up to ${MAX_ARTBOARDS} artboards.`);
  if (!target.space.id || !target.src) throw new Error('An artboard needs a space and page.');
  return arrange({
    ...state,
    revision: state.revision + 1,
    selected: id,
    artboards: [
      ...state.artboards,
      {
        id,
        target,
        size: size(dimensions.width, dimensions.height),
        x: 0,
        y: 0,
        generation: 1,
        view: null,
      },
    ],
  });
}
export function arrange(state, rowWidth = Infinity) {
  let x = 0,
    y = 32,
    rowHeight = 0;
  return {
    ...state,
    artboards: state.artboards.map((b) => {
      if (x > 0 && x + b.size.width > rowWidth) {
        x = 0;
        y += rowHeight + 80;
        rowHeight = 0;
      }
      const next = { ...b, x, y };
      x += b.size.width + 48;
      rowHeight = Math.max(rowHeight, b.size.height);
      return next;
    }),
  };
}
export function select(state, id) {
  if (!state.artboards.some((b) => b.id === id))
    throw new Error('That artboard is no longer open.');
  return { ...state, selected: id, revision: state.revision + 1 };
}
export function change(state, id, update) {
  if (!state.artboards.some((b) => b.id === id))
    throw new Error('That artboard is no longer open.');
  return arrange({
    ...state,
    revision: state.revision + 1,
    artboards: state.artboards.map((b) => (b.id === id ? { ...b, ...update } : b)),
  });
}
export function remove(state, id) {
  const at = state.artboards.findIndex((b) => b.id === id);
  if (at < 0) return state;
  const artboards = state.artboards.filter((b) => b.id !== id);
  return arrange({
    ...state,
    revision: state.revision + 1,
    artboards,
    selected:
      state.selected === id ? (artboards[at] || artboards[at - 1])?.id || null : state.selected,
  });
}
