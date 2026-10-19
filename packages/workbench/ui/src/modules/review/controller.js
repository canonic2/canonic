/** Review owns the operation and partial failure; adapters own HTTP and downloads. */
export function createReview(ports) {
  let busy = false;
  return {
    busy: () => busy,
    async run(handoff) {
      if (busy) throw new Error('A canvas capture is already running.');
      if (!ports.available()) throw new Error('Wait for the requested page to finish loading.');
      busy = true;
      let release;
      try {
        release = ports.hold();
        await ports.capture(async (snapshot, runtimes) => {
          const result = await ports.acquire(snapshot, runtimes);
          if (!handoff) {
            ports.download(result.blob);
            return;
          }
          const file = await ports.save(result.blob);
          await ports.handoff({ ...result.payload, file });
          await Promise.all(
            snapshot.artboards
              .filter((board) => board.view?.ready)
              .map((board) => runtimes(board.id).request('clear-captured')),
          );
          ports.notify('Copied handoff');
        });
      } finally {
        try {
          release?.();
        } finally {
          busy = false;
        }
      }
    },
  };
}
