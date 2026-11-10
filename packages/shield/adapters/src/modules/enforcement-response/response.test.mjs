import test from 'node:test';
import assert from 'node:assert/strict';
import { response } from './index.mjs';

const expected = { id: 'acme-request', phase: 'read-text', project: '/acme',
  projectId: 'acme-project', operation: { path: '/acme/read.txt' } };
const receipt = () => ({ protocol_version: 2, request_id: expected.id, decision: 'allow', code: 'allowed',
  identity: { project_id: expected.projectId, project_root: expected.project, activation_id: 'acme-active',
    previous_activation_id: null, policy_revision: 'acme-revision', candidate_digest: 'acme-digest', registry_revision: 'acme-registry-revision' },
  operation: { kind: 'read-text', path: expected.operation.path, offset: null, limit: null },
  history: { status: 'recorded', request_id: 'acme-history', event_id: 'acme-event', correlation: 'not-applicable', reported_request_id: null },
  approval: { status: 'none', request_id: null, approval_id: null },
  diagnostics: { message: null, denied_commands: [], requirement_checks: [], guidance: [], truncated: false } });
const decode = source => response(Buffer.from(source), 0, expected);
const evidenceReceipt = detail => {
  const value = receipt();
  value.diagnostics.requirement_checks.push({ rule_ref: { scope: 'custom', rule_id: 'acme-rule' },
    operation_ref: 'acme-operation', predicate_path: 'requirements', status: 'met', code: 'acme-check',
    branch: 'met', applied: true, evidence: { detail } });
  return value;
};

test('versioned completion receipts acknowledge publication without claiming a link or authority', () => {
  const completion = { ...expected, phase: 'completion', reportedRequestId: 'unverified-request' };
  const value = { ...receipt(), decision: 'deny', code: 'completion-observed', identity: null, operation: null,
    history: { status: 'recorded', request_id: null, event_id: 'acme-event', correlation: 'deferred', reported_request_id: completion.reportedRequestId } };
  const decodeCompletion = source => response(Buffer.from(JSON.stringify(source)), 0, completion);
  for (const status of ['recorded', 'disabled', 'failed']) {
    const valid = structuredClone(value);
    valid.history.status = status;
    if (status !== 'recorded') valid.history.event_id = null;
    assert.deepEqual(decodeCompletion(valid), valid);
  }
  const invalid = change => {
    const modified = structuredClone(value);
    change(modified);
    assert.throws(() => decodeCompletion(modified));
  };
  for (const change of [
    v => { v.protocol_version = 1; },
    v => { v.protocol_version = 3; },
    v => { delete v.history.correlation; },
    v => { v.history.correlation = 'verified'; },
    v => { v.history.correlation = 'not-applicable'; },
    v => { v.history.reported_request_id = 'different-request'; },
    v => { v.history.request_id = 'unverified-request'; },
    v => { v.history.event_id = null; },
    v => { v.history.status = 'not-requested'; v.history.event_id = null; },
    v => { v.history.extra = true; },
    v => { v.decision = 'allow'; },
    v => { v.code = 'policy-deny'; },
  ]) invalid(change);
  assert.throws(() => decode(JSON.stringify(value)), 'completion receipts cannot answer a permission request');
  const noReference = structuredClone(value);
  noReference.history.reported_request_id = null;
  assert.deepEqual(response(Buffer.from(JSON.stringify(noReference)), 0,
    { ...completion, reportedRequestId: null }), noReference);
});

test('decision receipts reject legacy versions and deferred or unverified history fields', () => {
  for (const change of [
    v => { v.protocol_version = 1; },
    v => { v.history.correlation = 'deferred'; },
    v => { v.history.reported_request_id = 'unverified-request'; },
    v => { v.history.request_id = null; },
  ]) {
    const value = receipt();
    change(value);
    assert.throws(() => decode(JSON.stringify(value)));
  }
});

test('versioned identities require the exact registry revision field', () => {
  for (const change of [
    value => { delete value.identity.registry_revision; },
    value => { value.identity.registry_revision = null; },
    value => { value.identity.registry_revision = ''; },
    value => { value.identity.unverified_registry = true; },
  ]) {
    const value = receipt();
    change(value);
    assert.throws(() => decode(JSON.stringify(value)));
  }
});

test('receipt decoding preserves Unicode and escaped text without confusing separate object keys', () => {
  const value = evidenceReceipt({ message: '雪 🛡️', nested: { message: 'quote " slash \\ braces {}[] colon :' } });
  value.diagnostics.message = 'literal \\u0000 and replacement character �';
  assert.deepEqual(decode(JSON.stringify(value)), value);
});

test('a briefly locked runtime is a failure receipt that never allows', () => {
  const value = { ...receipt(), decision: 'deny', code: 'runtime-busy', identity: null, operation: null,
    history: { status: 'not-requested', request_id: null, event_id: null, correlation: 'not-applicable', reported_request_id: null } };
  assert.deepEqual(response(Buffer.from(JSON.stringify(value)), 2, expected), value);
  assert.throws(() => response(Buffer.from(JSON.stringify(value)), 0, expected));
});

test('receipt decoding accepts bounded denied command forms and rejects other shapes', () => {
  const value = receipt();
  value.diagnostics.denied_commands.push({ executable: '/usr/bin/acme', rule_id: 'acme-deny', operation_ref: 'request' },
    { executable: null, rule_id: 'acme-file', operation_ref: null });
  assert.deepEqual(decode(JSON.stringify(value)), value);
  const missing = receipt();
  delete missing.diagnostics.denied_commands;
  const extra = receipt();
  extra.diagnostics.denied_commands.push({ executable: 'acme', rule_id: 'acme-deny', operation_ref: 'request', argv: [] });
  const many = receipt();
  for (let i = 0; i < 4; i++) many.diagnostics.denied_commands.push({ executable: 'acme', rule_id: `acme-${i}`, operation_ref: null });
  for (const invalid of [missing, extra, many]) assert.throws(() => decode(JSON.stringify(invalid)));
});

test('receipt decoding rejects duplicate decoded keys at every depth', () => {
  const source = JSON.stringify(receipt());
  for (const invalid of [
    source.replace('"decision":"allow"', '"decision":"deny","decision":"allow"'),
    source.replace('"decision":"allow"', '"decis\\u0069on":"deny","decision":"allow"'),
    source.replace('"message":null', '"message":"hidden","message":null'),
    JSON.stringify(evidenceReceipt('acme')).replace('"detail":"acme"', '"detail":false,"det\\u0061il":"acme"'),
  ]) assert.throws(() => decode(invalid));
});

test('receipt decoding rejects malformed UTF-8, BOMs and trailing JSON', () => {
  const source = JSON.stringify(receipt());
  const prefix = Buffer.from(source.replace('"message":null', '"message":"ACME_MARKER"').split('ACME_MARKER')[0]);
  const suffix = Buffer.from(source.replace('"message":null', '"message":"ACME_MARKER"').split('ACME_MARKER')[1]);
  for (const malformed of [[0xff], [0xc0, 0xaf], [0xed, 0xa0, 0x80], [0xf0, 0x9f]]) {
    assert.throws(() => response(Buffer.concat([prefix, Buffer.from(malformed), suffix]), 0, expected));
  }
  assert.throws(() => response(Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from(source)]), 0, expected));
  assert.throws(() => decode(source + '{}'));
  assert.throws(() => decode(source.replace('"message":null', '"message":"\\ud800"')));
  assert.throws(() => decode(source.replace('"message":null', '"message":"\\udfff"')));
});

test('receipt decoding bounds combined object and array nesting before schema validation', () => {
  // Root, diagnostics, checks, check and evidence account for five containers.
  let detail = null;
  for (let i = 0; i < 123; i++) detail = [detail];
  const value = evidenceReceipt(detail);
  assert.deepEqual(decode(JSON.stringify(value)), value);
  assert.throws(() => decode(JSON.stringify(evidenceReceipt([detail]))));
});
