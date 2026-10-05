import { test } from 'node:test';
import assert from 'node:assert/strict';
import { markColor, isLight } from './space.ts';

test('space branding supports named/hex colors and readable contrast', () => {
  assert.equal(markColor('green'), '#2f7d55');
  assert.equal(markColor('#abc'), '#abc');
  assert.equal(markColor('url(https://example.com)'), '');
  assert.equal(isLight('#fff'), true);
  assert.equal(isLight('#f5d76e'), true);
  assert.equal(isLight('#2f7d55'), false);
});
