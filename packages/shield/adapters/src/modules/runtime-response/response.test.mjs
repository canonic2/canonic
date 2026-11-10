import { test } from 'node:test';
import assert from 'node:assert/strict';
import { response } from './index.mjs';

const expected = { correlation_id: 'callback:1', operation: 'runtime.evaluate', project_id: 'project:1', inputDigest: 'a'.repeat(64) };
function receipt() {
  const explanation = { schema_version: 2, mode: 'preview', decision: 'allow', revision: 'selection-revision:1',
    matches: [], requirement_checks: [], guidance: [], reason: 'matched-rule', cwd: '/project', project: '/project', packs: [] };
  return { protocol_version: 1, correlation_id: 'callback:1', operation: 'runtime.evaluate', project_id: 'project:1',
    request_id: null, policy_revision: 'revision:1', error: null,
    result: { mode: 'runtime', decision: 'allow', disposition: 'allowed',
      normalized_request: { input: { kind: 'file', action: 'read', sources: ['/project/note.txt'],
        destination: null, recursive: false, original_input: { file_path: '/project/note.txt' } },
      context: { cwd: '/project', environment: null } }, explanation,
      analysis_coverage: { model: 'bounded-sdk-text-read-v1', complete: false, unknown: 'Host performs the read after the callback.' },
      approval_request_id: null, requirement_checks: [], guidance: [], retry_ref: null, approval_id: null,
      consumed: false, history: { status: 'disabled', request_id: null, event_id: null }, input_digest: 'a'.repeat(64) } };
}
const bytes = value => Buffer.from(JSON.stringify(value));

test('validates exact request binding and permits only the initial owned project pin', () => {
  const value = receipt();
  assert.deepEqual(response(bytes(value), 0, expected), value);
  assert.deepEqual(response(bytes(value), 0, { ...expected, project_id: undefined }), value);
  for (const changed of [{ project_id: 'other' }, { correlation_id: 'other' }, { operation: 'runtime.consume' }, { inputDigest: 'b'.repeat(64) }]) {
    assert.throws(() => response(bytes(value), 0, { ...expected, ...changed }));
  }
  assert.throws(() => response(bytes(value), 2, expected));
});

test('rejects synthesized, contradictory and prematurely consumed authority', () => {
  for (const mutate of [
    v => { v.result.consumed = true; },
    v => { v.result.decision = 'ask'; v.result.explanation.decision = 'ask'; },
    v => { v.result.decision = 'deny'; },
    v => { v.result.explanation.decision = 'deny'; },
    v => { v.result.approval_request_id = 'request:unbound'; },
    v => { v.result.history.status = 'recorded'; },
    v => { v.result.analysis_coverage.complete = true; },
    v => { v.result.normalized_request.context.environment = {}; },
    v => { v.result.guidance = [{ candidate_only: false }]; },
    v => { v.result.extra = 'future-authority'; },
  ]) {
    const value = receipt(); mutate(value);
    assert.throws(() => response(bytes(value), 0, expected));
  }
});

test('Ask consumption needs an exact private reference and a distinct consumed receipt', () => {
  const value = receipt();
  value.operation = 'runtime.consume'; value.request_id = 'request:1';
  Object.assign(value.result, { decision: 'ask', consumed: true, approval_id: 'approval:1',
    approval_request_id: 'request:1', retry_ref: 'b'.repeat(64) });
  value.result.explanation.decision = 'ask';
  const consume = { ...expected, operation: 'runtime.consume', request_id: 'request:1', retry_ref: 'b'.repeat(64) };
  assert.deepEqual(response(bytes(value), 0, consume), value);
  assert.throws(() => response(bytes(value), 0, { ...consume, retry_ref: 'c'.repeat(64) }));
  assert.throws(() => response(bytes(value), 0, { ...consume, request_id: 'request:other' }));
});

test('observation receipts acknowledge raw evidence without correlation or permission', () => {
  const observe = { ...expected, operation: 'history.observe', reported_request_id: 'request:1' };
  const value = { protocol_version: 1, correlation_id: expected.correlation_id, operation: observe.operation,
    project_id: 'project:1', request_id: null, policy_revision: null, error: null,
    result: { status: 'recorded', event_id: 'event:1', correlation: 'deferred', reported_request_id: 'request:1' } };
  assert.deepEqual(response(bytes(value), 0, observe), value);
  value.result.correlation = 'verified';
  assert.throws(() => response(bytes(value), 0, observe));
  value.result.correlation = 'deferred'; value.request_id = 'request:1';
  assert.throws(() => response(bytes(value), 0, observe));
});

test('rejects duplicate decoded keys, excess nesting, NUL and unsafe error messages', () => {
  const value = receipt();
  const duplicate = JSON.stringify(value).replace('"protocol_version":1', '"protocol_version":1,"protocol_versi\\u006fn":1');
  assert.throws(() => response(Buffer.from(duplicate), 0, expected));
  value.result.normalized_request.input.original_input.extra = '\0';
  assert.throws(() => response(bytes(value), 0, expected));
  const deep = receipt();
  let nested = null;
  for (let depth = 0; depth < 64; depth++) nested = [nested];
  deep.result.normalized_request.input.original_input.nested = nested;
  assert.throws(() => response(bytes(deep), 0, expected));
  const failure = { ...receipt(), result: null, error: { code: 'io_failure',
    message: 'Shield runtime request failed: io_failure.', operator_route: null } };
  assert.deepEqual(response(bytes(failure), 2, expected), failure);
  failure.error.message = '/private/user/secret';
  assert.throws(() => response(bytes(failure), 2, expected));
});
