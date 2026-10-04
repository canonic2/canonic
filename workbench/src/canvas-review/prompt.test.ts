import test from 'node:test';
import assert from 'node:assert/strict';
import { canvasPrompt } from './prompt.ts';

test('handoff retains all labelled regions and routes each local review through the existing prompt formatter', () => {
  const visited: string[] = [];
  const text = canvasPrompt({ file: '/tmp/canvas.jpg', selected: 'b', artboards: ['a','b'].map((id, i) => ({ id, src: 'same.html', space: { id: 'space' + i, name: 'Space ' + i }, region: { x: i * 500, y: 32, width: 393, height: 852 }, capturedAt: i, status: i ? 'loading' : 'ready' })) }, payload => { visited.push(payload.id as string); return `Local annotations for ${payload.id}`; });
  assert.deepEqual(visited, ['a','b']);
  assert.match(text, /Space: Space 1 \(space1\)/); assert.match(text, /Status: loading/); assert.match(text, /coordinates.*local/);
  assert.throws(() => canvasPrompt({ file: '/tmp/canvas.jpg', artboards: [] }, () => ''), /artboards/);
});
