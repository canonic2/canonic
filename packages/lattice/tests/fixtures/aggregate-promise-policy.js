/** @file Owned strict-unhandled-rejection fixture; failure control verifies the host policy. */
import { synchronousResult } from '../../src/modules/collection/synchronous-result.js';

if (process.argv[2] === 'unhandled-control') {
  Promise.reject(new Error('unhandled-positive-control'));
} else {
  let caught = 0;
  for (const callback of ['initial', 'read', 'add', 'remove', 'equals']) {
    const payload = {},
      fulfilled = Promise.resolve(payload);
    Object.defineProperty(payload, 'then', {
      get() {
        throw new Error('discarded fulfillment must not be assimilated');
      },
    });
    const values = [
      Promise.reject(new Error(`already-rejected-${callback}`)),
      new Promise((_, reject) =>
        queueMicrotask(() => reject(new Error(`later-rejection-${callback}`))),
      ),
      fulfilled,
    ];
    for (const value of values) {
      try {
        synchronousResult(value, callback);
      } catch (error) {
        if (!(error instanceof TypeError) || !error.message.includes(callback)) throw error;
        caught++;
      }
    }
  }
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
  if (caught !== 15) throw new Error('Promise result was accepted');
  process.stdout.write(
    'all Promise results rejected synchronously; process survived settlements\n',
  );
}
