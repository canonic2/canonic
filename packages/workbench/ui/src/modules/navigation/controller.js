/** The requested destination and supersession belong here, not in DOM handlers. */
export function createNavigation(ports) {
  let sequence = 0,
    group = '',
    disposed = false;
  let selection, sizedSelection;
  let awaitingSelection = false,
    navigating = false,
    expanding = false;
  function clear() {
    sequence++;
    group = '';
    selection = undefined;
    sizedSelection = undefined;
    awaitingSelection = false;
    navigating = false;
    expanding = false;
  }
  return {
    snapshot: () => ({ selection, awaitingSelection, navigating, expanding, disposed }),
    // A retained renderer reports the destination independently of the route.
    // Consume its requested preset only when that destination's size rules apply.
    takeSize(view) {
      if (
        !selection ||
        selection === sizedSelection ||
        selection.docs ||
        selection.artboards?.length ||
        !view ||
        view.problem ||
        view.src !== selection.src ||
        view.state !== selection.state ||
        view.lens !== selection.lens
      )
        return null;
      sizedSelection = selection;
      return selection.size;
    },
    resize(size) {
      if (selection && !disposed) selection = { ...selection, size };
    },
    presentation(boards) {
      const declared =
        !selection?.docs && selection?.artboards?.length ? selection.artboards : undefined;
      const matches = (view) =>
        !!view &&
        view.src === selection?.src &&
        view.state === selection?.state &&
        view.lens === selection?.lens;
      const visible =
        !disposed &&
        !awaitingSelection &&
        !!selection &&
        (declared
          ? !expanding &&
            boards.length === declared.length &&
            boards.every(
              (board) =>
                matches(board.target) &&
                declared.some((child) => child.id === board.target.artboard),
            )
          : boards.length === 1 &&
            matches(boards[0].view) &&
            !!(boards[0].view?.ready || boards[0].view?.problem));
      return { visible, pending: !visible && (navigating || !!selection) };
    },
    begin() {
      if (disposed || ports.busy()) return;
      sequence++;
      expanding = false;
      awaitingSelection = true;
      navigating = true;
      selection = undefined;
      ports.present();
    },
    complete() {
      navigating = false;
    },
    fail: clear,
    clear,
    async show(next) {
      const request = ++sequence;
      await ports.ready;
      if (disposed || request !== sequence) return;
      if (ports.busy()) throw new Error('Wait for the canvas capture to finish.');
      selection = next;
      awaitingSelection = false;
      ports.present();
      const declared = !next.docs ? next.artboards : undefined;
      const key = JSON.stringify([
        next.src,
        next.state,
        next.lens,
        next.docs ? next.example : null,
        declared,
      ]);
      if (group !== key) {
        group = key;
        expanding = true;
        try {
          await ports.activate(next);
        } catch (error) {
          if (disposed || request !== sequence) return;
          group = '';
          throw error;
        } finally {
          if (!disposed && request === sequence) {
            expanding = false;
            ports.present();
          }
        }
        if (disposed || request !== sequence) return;
        ports.fit();
      }
      await ports.settle(next);
      if (!disposed && request === sequence) ports.present();
    },
    dispose() {
      disposed = true;
      clear();
    },
  };
}
