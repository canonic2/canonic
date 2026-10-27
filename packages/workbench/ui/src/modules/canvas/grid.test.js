import test from 'node:test';
import assert from 'node:assert/strict';
import { gridSpacing, gridUniforms } from './grid.js';

test('grid spacing is 24 world pixels and doubles until at least 12 screen pixels', () => {
  assert.equal(gridSpacing(1), 24);
  assert.equal(gridSpacing(8), 192);
  assert.equal(gridSpacing(0.5), 12);
  assert.equal(gridSpacing(0.25), 12);
  assert.ok(Math.abs(gridSpacing(0.2) - 19.2) < 1e-9);
});

test('grid uniforms carry the device-space camera, dot size and colours', () => {
  const values = [...gridUniforms({ x: 10, y: -4, scale: 1 }, 2, [0.1, 0.1, 0.1, 1], [0.4, 0.2, 0.6, 0.5])];
  assert.deepEqual(values.slice(0, 4), [20, -8, 48, 1]);
  assert.deepEqual(values.slice(4, 8).map(v => +v.toFixed(4)), [0.1, 0.1, 0.1, 1]);
  // Dots are the line colour at half intensity and fully opaque.
  assert.deepEqual(values.slice(8).map(v => +v.toFixed(4)), [0.2, 0.1, 0.3, 1]);
});

test('dot half-size never drops below 0.7 device pixels', () => {
  assert.equal(gridUniforms({ x: 0, y: 0, scale: 1 }, 1, [0, 0, 0, 1], [0, 0, 0, 1])[3], Math.fround(0.7));
});
