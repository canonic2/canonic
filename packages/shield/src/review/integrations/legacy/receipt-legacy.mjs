// Frozen response validator shipped with the historical SDK interceptor.
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value)
  && [Object.prototype, null].includes(Object.getPrototypeOf(value));
const fields = (value, allowed) => object(value) && Object.keys(value).every(key => allowed.includes(key));
const text = (value, max = 4096) => typeof value === 'string' && value.length > 0
  && new TextEncoder().encode(value).length <= max && !value.includes('\0');
const codes = ['registration', 'unsupported-operation', 'unverified-read', 'read-changed', 'allowed',
  'policy-deny', 'approval-required', 'invalid-input', 'input-read', 'runtime-unavailable',
  'project-changed', 'invalid-context', 'handoff-failed', 'completion-observed', 'allowed-once',
  'approval-stale', 'approval-expired', 'approval-consumed', 'approval-revoked', 'approval-unavailable'];
const failures = ['invalid-input', 'input-read', 'runtime-unavailable', 'project-changed', 'invalid-context', 'handoff-failed'];

export function response(bytes, exit, expected) {
  const value = JSON.parse(bytes);
  if (!fields(value, ['protocol_version', 'request_id', 'decision', 'code', 'identity', 'operation', 'history', 'approval'])
    || Object.keys(value).length !== 8 || value.protocol_version !== 1 || value.request_id !== expected.id
    || !['allow', 'deny'].includes(value.decision) || !codes.includes(value.code)) throw Error('invalid response');
  if (exit !== (failures.includes(value.code) ? 2 : 0)
    || (failures.includes(value.code) || value.code === 'completion-observed' ? value.identity !== null : value.identity === null)) throw Error('invalid outcome');
  const history = value.history;
  const approval = value.approval;
  if (!fields(approval, ['request_id', 'status', 'approval_id']) || Object.keys(approval).length !== 3
    || !['none', 'pending', 'consumed', 'stale', 'expired', 'revoked', 'unavailable'].includes(approval.status)
    || !(approval.request_id === null || text(approval.request_id, 128))
    || !(approval.approval_id === null || text(approval.approval_id, 128))
    || (['none', 'unavailable'].includes(approval.status) ? approval.request_id !== null || approval.approval_id !== null : !approval.request_id)
    || (value.code === 'allowed-once' ? approval.status !== 'consumed' || !approval.approval_id
      || approval.request_id !== expected.reference : approval.approval_id !== null)) throw Error('invalid approval receipt');
  if ((value.code === 'allowed' && approval.status !== 'none')
    || (approval.status === 'pending' && value.code !== 'approval-required')
    || ['stale', 'expired', 'revoked', 'unavailable'].some(status => approval.status === status && value.code !== `approval-${status}`)
    || (approval.status === 'consumed' && !['allowed-once', 'approval-consumed'].includes(value.code))) throw Error('inconsistent approval outcome');
  if (!fields(history, ['status', 'request_id', 'event_id']) || Object.keys(history).length !== 3
    || !['recorded', 'disabled', 'failed', 'not-requested'].includes(history.status)
    || (history.status === 'recorded'
      ? !text(history.event_id, 128) || !(history.request_id === null || text(history.request_id, 128))
      : history.event_id !== null || history.request_id !== null)) throw Error('invalid history receipt');
  if (expected.phase !== 'register' && value.decision === 'allow'
    && (history.status === 'not-requested' || (history.status === 'recorded' && history.request_id === null))) throw Error('missing history receipt');
  if (value.code === 'completion-observed' && expected.phase !== 'completion') throw Error('unexpected completion');
  const identity = value.identity;
  if (identity !== null && (!fields(identity, ['project_id', 'project_root', 'activation_id',
    'previous_activation_id', 'policy_revision', 'candidate_digest']) || Object.keys(identity).length !== 6
    || !['project_id', 'project_root', 'activation_id', 'policy_revision', 'candidate_digest'].every(key => text(identity[key]))
    || !(identity.previous_activation_id === null || text(identity.previous_activation_id))
    || identity.project_root !== expected.project || (expected.projectId && identity.project_id !== expected.projectId))) {
    throw Error('invalid identity');
  }
  if (value.decision === 'allow') {
    const operation = value.operation;
    if (exit !== 0 || !['allowed', 'allowed-once'].includes(value.code) || !identity || !expected.operation
      || !fields(operation, ['kind', 'path', 'offset', 'limit']) || Object.keys(operation).length !== 4
      || operation.kind !== 'read-text' || operation.path !== expected.operation.path
      || operation.offset !== (expected.operation.offset ?? null)
      || operation.limit !== (expected.operation.limit ?? null)) throw Error('invalid allow');
  } else if (value.operation !== null || ['allowed', 'allowed-once'].includes(value.code)) throw Error('invalid denial');
  return value;
}

// Test harness only; the validator above is the retained installed implementation.
import { readFileSync } from 'node:fs';
for (const entry of JSON.parse(readFileSync(process.argv[2], 'utf8'))) {
  response(JSON.stringify(entry.receipt), entry.exit, entry.expected);
}
