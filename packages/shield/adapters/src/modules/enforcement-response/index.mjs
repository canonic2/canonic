// Strict normalized core receipts for registered SDK callbacks.
import { parseJSON } from './json.mjs';

const object = value => value !== null && typeof value === 'object' && !Array.isArray(value)
  && [Object.prototype, null].includes(Object.getPrototypeOf(value));
const fields = (value, allowed) => object(value) && Object.keys(value).every(key => allowed.includes(key));
const text = (value, max = 4096) => typeof value === 'string' && value.length > 0
  && new TextEncoder().encode(value).length <= max && !value.includes('\0');
const codes = ['registration', 'unsupported-operation', 'unverified-read', 'read-changed', 'allowed',
  'policy-deny', 'approval-required', 'invalid-input', 'input-read', 'runtime-unavailable', 'runtime-busy',
  'compiled-cache-unavailable', 'project-changed', 'invalid-context', 'handoff-failed', 'completion-observed', 'allowed-once',
  'approval-stale', 'approval-expired', 'approval-consumed', 'approval-revoked', 'approval-unavailable'];
/** Receipt codes for a request the core could not decide; they exit 2. */
export const FAILURE_CODES = Object.freeze(['invalid-input', 'input-read', 'runtime-unavailable', 'runtime-busy',
  'compiled-cache-unavailable', 'project-changed', 'invalid-context', 'handoff-failed']);
const failures = FAILURE_CODES;

export function response(bytes, exit, expected) {
  const value = parseJSON(bytes);
  if (!fields(value, ['protocol_version', 'request_id', 'decision', 'code', 'identity', 'operation', 'history', 'approval', 'diagnostics'])
    || Object.keys(value).length !== 9 || value.protocol_version !== 1 || value.request_id !== expected.id
    || !['allow', 'deny'].includes(value.decision) || !codes.includes(value.code)) throw Error('invalid response');
  {
    const d = value.diagnostics;
    if (!fields(d, ['message', 'denied_commands', 'requirement_checks', 'guidance', 'truncated']) || Object.keys(d).length !== 5
      || !(d.message === null || text(d.message, 1024)) || typeof d.truncated !== 'boolean'
      || !Array.isArray(d.denied_commands) || d.denied_commands.length > 3
      || d.denied_commands.some(c => !fields(c, ['executable', 'rule_id', 'operation_ref']) || Object.keys(c).length !== 3
        || !(c.executable === null || text(c.executable)) || !text(c.rule_id, 128)
        || !(c.operation_ref === null || text(c.operation_ref, 128)))
      || !Array.isArray(d.requirement_checks) || d.requirement_checks.length > 32
      || !Array.isArray(d.guidance) || d.guidance.length > 3) throw Error('invalid diagnostics');
    const reference = r => fields(r, ['scope', 'rule_id', 'pack_selector']) && text(r.rule_id, 128)
      && (r.scope === 'custom' ? Object.keys(r).length === 2 : r.scope === 'pack'
        && Object.keys(r).length === 3 && text(r.pack_selector));
    if (d.requirement_checks.some(c => !fields(c, ['rule_ref', 'operation_ref', 'predicate_path', 'status', 'code', 'branch', 'applied', 'evidence'])
      || !reference(c.rule_ref) || !text(c.operation_ref, 128) || !text(c.predicate_path)
      || !['met', 'failed', 'unknown'].includes(c.status) || !text(c.code, 128)
      || !['met', 'unmet'].includes(c.branch) || typeof c.applied !== 'boolean'
      || (c.evidence !== undefined && !object(c.evidence)))) throw Error('invalid requirement checks');
    if (d.guidance.some(g => !fields(g, ['rule_ref', 'operation_ref', 'branch', 'message', 'alternatives', 'candidate_only'])
      || Object.keys(g).length !== 6 || !reference(g.rule_ref) || !text(g.operation_ref, 128)
      || !['met', 'unmet'].includes(g.branch) || !(g.message === null || text(g.message, 512))
      || !Array.isArray(g.alternatives) || g.alternatives.length > 3 || !g.alternatives.every(a => text(a, 256))
      || g.candidate_only !== true)) throw Error('invalid guidance');
  }
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
