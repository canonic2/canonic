/** @file Strict rejection observation and cross-realm checks for aggregate callback results. */
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';
import { expect, test, vi } from 'vitest';
import { synchronousResult } from './synchronous-result.js';

/**
 * Runs the owned fixture with strict host enforcement and no unhandled-event suppression.
 * @param {string} mode Scenario name.
 * @returns {Promise<{code: number | null, signal: NodeJS.Signals | null, out: string, err: string}>} Captured exit.
 */
function child(mode) {
  return new Promise((resolve, reject) => {
    const processHandle = spawn(
      process.execPath,
      [
        '--unhandled-rejections=strict',
        fileURLToPath(
          new URL('../../../tests/fixtures/aggregate-promise-policy.js', import.meta.url),
        ),
        mode,
      ],
      { stdio: ['ignore', 'pipe', 'pipe'] },
    );
    let out = '',
      err = '';
    const timer = setTimeout(() => {
      processHandle.kill('SIGKILL');
      reject(new Error('owned Promise-policy fixture deadline'));
    }, 3000);
    processHandle.stdout.on('data', (data) => {
      out += data;
    });
    processHandle.stderr.on('data', (data) => {
      err += data;
    });
    processHandle.once('error', (error) => {
      clearTimeout(timer);
      reject(error);
    });
    processHandle.once('exit', (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal, out, err });
    });
  });
}

test.each(['initial', 'read', 'add', 'remove', 'equals'])(
  '%s never accepts even a fulfilled Promise',
  (callback) => {
    expect(() => synchronousResult(Promise.resolve(1), callback)).toThrow(
      new TypeError(`Aggregate ${callback} must return a synchronous value, not a Promise.`),
    );
  },
);

test('already rejected and later rejecting Promises produce no unhandled rejection in a strict child', async () => {
  const result = await child('handled');
  expect(result.code).toBe(0);
  expect(result.signal).toBeNull();
  expect(result.err).toBe('');
  expect(result.out).toContain('process survived settlements');
});

test('the same strict child fails on an unobserved rejection positive control', async () => {
  const result = await child('unhandled-control');
  expect(result.code).toBe(1);
  expect(result.signal).toBeNull();
  expect(result.err).toContain('unhandled-positive-control');
});

test('uses the native Promise operation without reading or invoking an own then override', async () => {
  const value = Promise.reject(new Error('overridden then'));
  const readThen = vi.fn(() => {
    throw new Error('own then must not run');
  });
  Object.defineProperty(value, 'then', { get: readThen });
  expect(() => synchronousResult(value, 'read')).toThrow(TypeError);
  expect(readThen).not.toHaveBeenCalled();
  await new Promise((resolve) => setImmediate(resolve));
});

test('observes a cross-realm native Promise rejection without accepting its value', async () => {
  const value = /** @type {Promise<unknown>} */ (
    runInNewContext('Promise.reject(new Error("cross-realm rejection"))')
  );
  expect(() => synchronousResult(value, 'add')).toThrow(TypeError);
  await new Promise((resolve) => setImmediate(resolve));
});

test('does not invoke an arbitrary thenable passed to the native-only helper', () => {
  const then = vi.fn();
  expect(() => Reflect.apply(synchronousResult, undefined, [{ then }, 'remove'])).toThrow(
    TypeError,
  );
  expect(then).not.toHaveBeenCalled();
});
