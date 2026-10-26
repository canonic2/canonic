/** The requested destination and supersession belong here, not in DOM handlers. */
import { begin as beginTrace, phase } from '../trace/browser.js';
import { createStore } from '@canonic2/lattice';
import { initial, transition, feedback } from './state.js';
/**
 * The iterations the canvas shows, keyed like the address's `+` parts ("" for
 * a scenario's implicit artboard): the selection's, for the artboards on the
 * canvas, under the design lens only. Empty when every artboard shows its
 * authored design.
 */
export function shownIterations(selection, boards) {
  const shown = {};
  if (!selection || selection.docs || (selection.lens && selection.lens !== 'design')) return shown;
  for (const board of boards) {
    const key = board.target?.artboard || '';
    const id = selection.iterations?.[key];
    if (typeof id === 'string' && id) shown[key] = id;
  }
  return shown;
}
export function createNavigation(ports) {
  const store = createStore({ ...initial(), expanding: false, disposed: false });
  /** @type {import('@canonic2/lattice').StoreReader<ReturnType<typeof store.get>>} */
  const state = { get: store.get, watch: store.watch };
  const namespace = 'canvas:' + crypto.randomUUID() + ':';
  let serial = 0;
  const mint = () => namespace + ++serial;
  const send = (event) => store.update((current) => transition(current, event));
  const current = () => store.get();
  let trace, clicked;
  let loadingTimer;
  function cancelLoading() {
    clearTimeout(loadingTimer);
    loadingTimer = undefined;
  }
  function deferLoading() {
    cancelLoading();
    const id = current().id;
    loadingTimer = setTimeout(() => {
      loadingTimer = undefined;
      if (current().disposed) return;
      send({ type: 'deadline', id });
    }, 200);
  }
  let group = '';
  let sizedSelection;
  function clear() {
    cancelLoading();
    trace = null;
    clicked = null;
    group = '';
    sizedSelection = undefined;
    store.update({ ...initial(), expanding: false });
  }
  // Whether the selection's size rules apply to the view a renderer reports:
  // a page's single artboard showing the selected destination. Docs read at
  // the canvas's width and declared artboards keep their authored sizes.
  function sizes(view) {
    const selection = current().requested;
    return (
      !!selection &&
      !selection.docs &&
      !selection.artboards?.length &&
      !!view &&
      !view.problem &&
      view.src === selection.src &&
      view.scenario === selection.scenario &&
      view.lens === selection.lens
    );
  }
  return {
    state,
    select: { feedback },
    snapshot: () => {
      const value = current();
      const navigating = ['pending', 'loading'].includes(value.phase);
      return {
        selection: value.requested || undefined,
        awaitingSelection: navigating && !value.requested,
        navigating,
        expanding: value.expanding,
        disposed: value.disposed,
      };
    },
    captureStarted: () => send({ type: 'captureStarted' }),
    captureEnded: () => send({ type: 'captureEnded' }),
    adopt(from, id, destination) {
      if (
        current().disposed ||
        current().capturing ||
        from !== current().id ||
        typeof id !== 'string' ||
        !id
      )
        return false;
      send({
        type: 'navigated',
        from,
        id,
        destination: {
          ...current().requested,
          ...destination,
          iterations: { ...current().requested?.iterations, ...destination.iterations },
        },
      });
      deferLoading();
      return true;
    },
    observe(model) {
      const { id, requested, expanding, disposed } = current();
      if (disposed || !requested || expanding) return;
      for (const board of model.artboards) {
        const view = board.view;
        if (view?.navigation !== id || (!view.ready && !view.problem)) continue;
        send({
          type: view.problem ? 'failed' : 'ready',
          id,
          slot: board.target.artboard || '',
          view,
          problem: view.problem || undefined,
          model,
        });
      }
      const displayed = current().displayed;
      if (displayed?.id === id && displayed.model !== model)
        store.update({ displayed: { ...displayed, model } });
    },
    // A retained renderer reports the destination independently of the route.
    // Consume its requested preset only when that destination's size rules apply.
    takeSize(view) {
      const selection = current().requested;
      if (selection === sizedSelection || !sizes(view)) return null;
      sizedSelection = selection;
      return selection.size;
    },
    // The preset already applied to the shown destination, so the canvas can
    // resolve its filled axes again when its room changes. Null while a new
    // preset waits to be taken.
    appliedSize(view) {
      const selection = current().requested;
      return selection === sizedSelection && sizes(view) ? selection.size : null;
    },
    resize(size) {
      const selection = current().requested;
      if (selection && !current().disposed) store.update({ requested: { ...selection, size } });
    },
    presentation() {
      const { visible, pending, loading } = feedback.read(current());
      return { visible, pending, loading };
    },
    begin() {
      if (current().disposed || current().capturing || ports.busy()) return;
      clicked = beginTrace();
      trace = null;
      store.update({
        ...transition(current(), { type: 'picked', id: mint(), destination: null }),
        expanding: false,
      });
      deferLoading();
    },
    complete(outcome = 'visible') {
      cancelLoading();
      if (trace) {
        const completed = trace;
        trace = null;
        requestAnimationFrame(() => {
          if (!current().disposed && current().requested?.trace?.selection === completed.selection)
            phase(completed, outcome, {
              totalMs: performance.timeOrigin + performance.now() - completed.started,
            });
        });
      }
    },
    fail(error) {
      cancelLoading();
      group = '';
      if (current().phase === 'idle') send({ type: 'picked', id: mint(), destination: null });
      send({
        type: 'failed',
        id: current().id,
        problem:
          error instanceof Error ? error.message : String(error || 'Could not load preview.'),
      });
    },
    clear,
    async show(next) {
      trace = beginTrace(next, clicked);
      if (trace && clicked) trace.previous = clicked.started;
      clicked = null;
      if (trace) {
        next = { ...next, trace };
        phase(trace, 'selection');
      }
      if (current().disposed) return;
      if (current().capturing || ports.busy())
        throw new Error('Wait for the canvas capture to finish.');
      const awaiting = !current().requested && ['pending', 'loading'].includes(current().phase);
      const request = awaiting ? current().id : mint();
      send({ type: 'picked', id: request, destination: next });
      if (!awaiting) deferLoading();
      try {
        await ports.ready;
      } catch (error) {
        if (current().disposed || request !== current().id) return;
        throw error;
      }
      if (current().disposed || request !== current().id) return;
      if (current().capturing || ports.busy())
        throw new Error('Wait for the canvas capture to finish.');
      next = { ...next, navigation: request };
      const declared = !next.docs ? next.artboards : undefined;
      const key = JSON.stringify([
        next.src,
        next.scenario,
        next.lens,
        next.docs ? next.example : null,
        declared,
      ]);
      if (group !== key) {
        group = key;
        store.update({ expanding: true });
        try {
          await ports.activate(next);
        } catch (error) {
          if (current().disposed || request !== current().id) return;
          group = '';
          send({
            type: 'failed',
            id: request,
            problem: error instanceof Error ? error.message : String(error),
          });
          throw error;
        } finally {
          if (!current().disposed && request === current().id) {
            store.update({ expanding: false });
            ports.present();
          }
        }
        if (current().disposed || request !== current().id) return;
        ports.fit();
      } else {
        // The same artboards: only those whose iteration changed render again.
        try {
          await ports.iterate?.(next);
        } catch (error) {
          if (current().disposed || request !== current().id) return;
          throw error;
        }
        if (current().disposed || request !== current().id) return;
      }
      try {
        await ports.settle(next);
      } catch (error) {
        if (current().disposed || request !== current().id) return;
        throw error;
      }
      if (!current().disposed && request === current().id) ports.present();
    },
    dispose() {
      clear();
      store.update({ disposed: true });
    },
  };
}
