import { createStoryCatalog } from './story-catalog.js';

/** Owns ordinary-renderer retention. Canvas ports own instances and visible geometry. */
export function createPreviewSessions(ports) {
  const catalog = createStoryCatalog(ports.stories);
  let parked, timer;
  // Transient outgoing instances are released when presentation commits or
  // reaches the loading deadline. The ordinary warm renderer keeps its pool.
  const departing = new Map();
  let disposed = false;
  function finish() {
    const previous = [...departing];
    departing.clear();
    for (const [id, keep] of previous) {
      if (keep) ports.conceal(id, true);
      else ports.close(id);
    }
  }
  function expire() {
    if (disposed || !parked) return;
    if (ports.busy()) {
      timer = setTimeout(expire, 1000);
      return;
    }
    ports.close(parked);
    parked = undefined;
  }
  function releaseParked() {
    clearTimeout(timer);
    if (parked) ports.close(parked);
    parked = undefined;
  }
  return {
    stories(implementation, title) {
      if (disposed) return Promise.reject(new Error('Preview sessions closed.'));
      return catalog.stories(implementation, title);
    },
    async activate(next) {
      if (disposed) throw new Error('Preview sessions closed.');
      const boards = ports.visible();
      const declared = next.artboards?.length ? next.artboards : undefined;
      const retained =
        !declared && boards.length === 1 && !boards[0].target.artboard && boards[0].initialized;
      const destination = {
        src: next.target.src,
        scenario: next.target.scenario,
        lens: next.target.lens,
        example: next.target.example,
        ...(next.target.trace ? { trace: next.target.trace } : {}),
      };
      if (retained) {
        await ports.command(boards[0].id, 'navigate', destination);
        return;
      }
      for (const board of boards) {
        const displayed = ports.displayed?.().includes(board.id);
        if (declared && !board.target.artboard) {
          releaseParked();
          if (displayed) departing.set(board.id, true);
          else ports.conceal(board.id, true);
          ports.park(board.id);
          parked = board.id;
          timer = setTimeout(expire, 15 * 60 * 1000);
        } else if (displayed) {
          departing.set(board.id, false);
          ports.park(board.id);
        } else ports.close(board.id);
      }
      if (declared) {
        const ids = [];
        try {
          for (const artboard of declared)
            ids.push(
              ports.create(
                {
                  ...next.target,
                  label: next.pageLabel + ' / ' + artboard.label,
                  artboard: artboard.id,
                },
                { width: artboard.width, height: artboard.height },
              ),
            );
          ports.choose(ids[0]);
        } catch (error) {
          const failures = [];
          for (const id of ids) {
            try {
              ports.close(id);
            } catch (cleanup) {
              failures.push(cleanup);
            }
          }
          if (failures.length)
            throw new AggregateError(
              [error, ...failures],
              'Artboard creation and cleanup failed.',
              { cause: error },
            );
          throw error;
        }
      } else if (parked) {
        const id = parked;
        parked = undefined;
        departing.delete(id);
        clearTimeout(timer);
        ports.conceal(id, false);
        ports.resume(id);
        await ports.command(id, 'navigate', destination);
      } else ports.create(next.target, next.size);
    },
    finish,
    refresh() {
      if (disposed) return Promise.reject(new Error('Preview sessions closed.'));
      catalog.clear();
      return Promise.all(
        [...ports.visible().map((board) => board.id), ...(parked ? [parked] : [])].map((id) =>
          ports.command(id, 'refresh'),
        ),
      );
    },
    clear() {
      finish();
      for (const board of ports.visible()) ports.close(board.id);
      releaseParked();
      catalog.clear();
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      finish();
      releaseParked();
      catalog.clear();
    },
  };
}
