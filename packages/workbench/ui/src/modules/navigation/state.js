/** Navigation rules. No DOM, transport, clock or renderer lifecycle. */
import { shallow } from '../../../../node_modules/@canonic2/lattice/src/index.js';
export const initial = () => ({
  id: null,
  requested: null,
  displayed: null,
  slots: [],
  phase: 'idle',
  problem: null,
  capturing: false,
});

export function slotsFor(destination) {
  return (
    !destination.docs && destination.artboards?.length ? destination.artboards : [{ id: '' }]
  ).map(({ id }) => ({ id, status: 'pending', view: null, problem: null }));
}

export function matches(destination, slot, view) {
  const design = !destination.lens || destination.lens === 'design';
  return (
    !!view &&
    view.src === destination.src &&
    (view.scenario || null) === (destination.scenario || null) &&
    (view.lens || 'design') === (destination.lens || 'design') &&
    (!!view.problem ||
      (view.iteration || null) === ((design && destination.iterations?.[slot]) || null))
  );
}

export function transition(state, event) {
  if (event.type === 'captureStarted') return { capturing: true };
  if (event.type === 'captureEnded') return { capturing: false };
  if (event.type === 'clear') return initial();
  if (event.type === 'picked' || event.type === 'navigated') {
    if (event.type === 'navigated' && event.from !== state.id) return {};
    return {
      id: event.id,
      requested: event.destination,
      slots: event.destination ? slotsFor(event.destination) : [],
      phase: event.id === state.id && state.phase === 'loading' ? 'loading' : 'pending',
      problem: null,
    };
  }
  if (event.id !== state.id || state.phase === 'idle') return {};
  if (event.type === 'deadline')
    return state.phase === 'pending' ? { phase: 'loading', displayed: null } : {};
  if (event.type === 'failed' && event.slot === undefined)
    return { phase: 'failed', problem: event.problem, displayed: null };
  if (!state.requested || state.problem || !['ready', 'failed'].includes(event.type)) return {};
  const slot = state.slots.find((s) => s.id === event.slot);
  if (
    !slot ||
    (event.type === 'ready' &&
      (!event.view?.ready || !matches(state.requested, slot.id, event.view)))
  )
    return {};
  if (
    slot.status === event.type &&
    slot.view === event.view &&
    slot.problem === (event.problem || null)
  )
    return {};
  const slots = state.slots.map((s) =>
    s === slot
      ? {
          id: s.id,
          status: event.type,
          view: event.view || null,
          problem: event.problem || null,
        }
      : s,
  );
  if (slots.some((s) => s.status === 'pending')) return { slots };
  return {
    slots,
    phase: slots.some((s) => s.status === 'ready') ? 'shown' : 'failed',
    displayed: { id: state.id, destination: state.requested, model: event.model || null },
  };
}

export const feedback = {
  equals: shallow,
  fields: ['phase', 'displayed', 'capturing', 'problem'],
  read: (state) => ({
    visible: state.phase === 'shown' || (state.phase === 'failed' && !state.problem),
    pending: state.phase === 'pending' || state.phase === 'loading',
    loading: state.phase === 'loading',
    inert: state.capturing || !['shown', 'failed'].includes(state.phase),
    displayed: state.displayed,
    problem: state.problem,
  }),
};
