import test from 'node:test';
import assert from 'node:assert/strict';
import { APPROVAL_LIMIT, APPROVAL_RETENTION_MS, ApprovalReferences, PENDING_LIMIT, PendingReads, denialReason } from './state.mjs';

test('read denials route each code to the actor who can resolve it', () => {
  for (const code of ['approval-stale', 'approval-expired', 'approval-consumed', 'read-changed']) {
    const reason = denialReason(code, null, null, '/srv/acme');
    assert.match(reason, /Retry to submit a fresh request/, code);
    assert.doesNotMatch(reason, /doctor/, code);
  }
  const revoked = denialReason('approval-revoked', null, null, '/srv/acme');
  assert.match(revoked, /revoked or denied/);
  assert.match(revoked, /Do not retry automatically/);
  assert.match(denialReason('approval-unavailable', null, null, '/srv/acme'), /Submit a fresh action request/);
  const busy = denialReason('runtime-busy', null, null, '/srv/acme');
  assert.match(busy, /briefly locked; retry/);
  assert.doesNotMatch(busy, /doctor/);
  const context = denialReason('invalid-context', null, null, '/srv/acme');
  assert.match(context, /working directory/);
  assert.doesNotMatch(context, /tool input/);
  assert.match(denialReason('invalid-read-input', null, null, '/srv/acme'), /Correct the tool input/);
  assert.match(denialReason('runtime-unavailable', null, null, '/srv/acme'), /shield doctor/);
});

test('pending reads keep correlating after the limit by evicting the oldest', () => {
  const pending = new PendingReads();
  for (let index = 0; index < PENDING_LIMIT + 10; index++) pending.allow(`tool-${index}`, { request: `request-${index}` });
  assert.equal(pending.size, PENDING_LIMIT);
  assert.equal(pending.complete('tool-0'), undefined);
  assert.deepEqual(pending.complete(`tool-${PENDING_LIMIT + 9}`), { request: `request-${PENDING_LIMIT + 9}` });
  // A redelivered completion still correlates; a reused host ID never does.
  assert.deepEqual(pending.complete(`tool-${PENDING_LIMIT + 9}`), { request: `request-${PENDING_LIMIT + 9}` });
  pending.allow(`tool-${PENDING_LIMIT + 9}`, { request: 'reused' });
  assert.equal(pending.complete(`tool-${PENDING_LIMIT + 9}`), null);
});

test('approval references drop abandoned asks so a later grant stays usable', () => {
  let now = 0;
  const approvals = new ApprovalReferences(() => now);
  for (let index = 0; index < APPROVAL_LIMIT + 5; index++) approvals.pending(`ask-${index}`, `request-${index}`);
  assert.equal(approvals.size, APPROVAL_LIMIT);
  assert.equal(approvals.get('ask-0'), undefined);
  now = 1000;
  approvals.pending('approved', 'request-approved');
  assert.equal(approvals.size, APPROVAL_LIMIT);
  assert.equal(approvals.get('approved'), 'request-approved');
  now = APPROVAL_RETENTION_MS + 500;
  // A repeated pending reference keeps its first time.
  approvals.pending('approved', 'request-approved');
  assert.equal(approvals.size, 1, 'abandoned asks expire');
  assert.equal(approvals.get('approved'), 'request-approved');
  now = APPROVAL_RETENTION_MS + 1000;
  assert.equal(approvals.get('approved'), undefined);
  assert.equal(approvals.size, 0);
});

test('approval references survive undecided receipts and end with a final outcome', () => {
  const references = new ApprovalReferences(() => 0);
  const receipt = (code, status, request_id = null) => ({ code, approval: { status, request_id } });
  references.record('read', receipt('approval-required', 'pending', 'acme-request'));
  // A briefly locked runtime or another undecided failure keeps the reference,
  // so the retry its message asks for still consumes the same approval.
  for (const code of ['runtime-busy', 'runtime-unavailable', 'handoff-failed', 'project-changed']) {
    references.record('read', receipt(code, 'none'));
    assert.equal(references.get('read'), 'acme-request', code);
  }
  // A final approval outcome ends it.
  for (const status of ['consumed', 'stale', 'expired', 'revoked', 'unavailable']) {
    references.record('read', receipt('approval-required', 'pending', 'acme-request'));
    references.record('read', receipt(status === 'consumed' ? 'allowed-once' : `approval-${status}`, status, 'acme-request'));
    assert.equal(references.get('read'), undefined, status);
  }
  // So does a policy decision that needs no approval.
  for (const code of ['allowed', 'policy-deny']) {
    references.record('read', receipt('approval-required', 'pending', 'acme-request'));
    references.record('read', receipt(code, 'none'));
    assert.equal(references.get('read'), undefined, code);
  }
});
