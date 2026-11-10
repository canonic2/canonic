// Closed general-runtime-v1 receipts for the explicitly reviewed read-only profile.
import { parseJSON } from '../enforcement-response/json.mjs';
import { isDeepStrictEqual } from 'node:util';

export const PROTOCOL_VERSION = 1;
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const fields = (value, required, optional = []) => object(value)
  && required.every(key => Object.hasOwn(value, key))
  && Object.keys(value).every(key => required.includes(key) || optional.includes(key));
const text = (value, max = 4096) => typeof value === 'string' && value.length > 0
  && Buffer.byteLength(value) <= max && !value.includes('\0');
const nullableText = (value, max) => value === null || text(value, max);
const digest = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const decisions = ['allow', 'ask', 'deny'];
const codes = ['invalid_input', 'unsupported_version', 'unsupported_operation', 'project_unavailable',
  'state_invalid', 'stale_revision', 'request_unavailable', 'candidate_unavailable', 'approval_required',
  'grant_invalid', 'budget_exceeded', 'overloaded', 'cancelled', 'timeout', 'io_failure', 'response_invalid'];
const messages = Object.freeze(Object.fromEntries(codes.map(code => [code, `Shield runtime request failed: ${code}.`])));

function bounded(value, depth = 0) {
  if (depth > 64) return false;
  if (typeof value === 'number') return Number.isFinite(value);
  if (typeof value === 'string') return !value.includes('\0');
  if (Array.isArray(value)) return value.every(item => bounded(item, depth + 1));
  return !object(value) || Object.entries(value).every(([key, item]) => !key.includes('\0') && bounded(item, depth + 1));
}

function reference(value) {
  return value === null || (fields(value, ['scope', 'rule_id'], ['pack_selector'])
    && text(value.rule_id, 128) && (value.scope === 'custom' ? !Object.hasOwn(value, 'pack_selector')
      : value.scope === 'pack' && text(value.pack_selector)));
}

function check(value) {
  return fields(value, ['rule_ref', 'operation_ref', 'predicate_path', 'status', 'code', 'branch', 'applied'], ['evidence'])
    && reference(value.rule_ref) && text(value.operation_ref, 128) && text(value.predicate_path)
    && ['met', 'failed', 'unknown'].includes(value.status) && text(value.code, 128)
    && ['met', 'unmet'].includes(value.branch) && typeof value.applied === 'boolean';
}

function guidance(value) {
  return fields(value, ['alternatives', 'candidate_only', 'message', 'operation_ref'],
    ['branch', 'rule_ref', 'source', 'request', 'fetch_url'])
    && Array.isArray(value.alternatives) && value.alternatives.every(item => text(item))
    && value.candidate_only === true && nullableText(value.message)
    && text(value.operation_ref, 128)
    && (!Object.hasOwn(value, 'branch') || ['met', 'unmet'].includes(value.branch))
    && (!Object.hasOwn(value, 'rule_ref') || reference(value.rule_ref))
    && (!Object.hasOwn(value, 'source') || text(value.source, 128))
    && (!Object.hasOwn(value, 'fetch_url') || nullableText(value.fetch_url, 8192))
    && (!Object.hasOwn(value, 'request') || (fields(value.request, ['executable', 'arguments'], ['cwd'])
      && text(value.request.executable) && Array.isArray(value.request.arguments)
      && value.request.arguments.every(item => typeof item === 'string')
      && (!Object.hasOwn(value.request, 'cwd') || text(value.request.cwd))));
}

function explanation(value, decision) {
  return fields(value, ['schema_version', 'mode', 'decision', 'revision', 'matches',
    'requirement_checks', 'guidance', 'reason', 'cwd', 'project', 'packs'],
    ['mapped_commands', 'effect_evidence', 'semantic_coverage'])
    && value.schema_version === 2 && value.mode === 'preview'
    && value.decision === decision && text(value.revision, 128)
    && text(value.reason) && text(value.cwd) && text(value.project)
    && Array.isArray(value.packs) && value.packs.every(pack => fields(pack, ['id', 'version', 'digest', 'profile'])
      && text(pack.id) && text(pack.version, 128) && digest(pack.digest) && ['loose', 'recommended', 'strict'].includes(pack.profile))
    && Array.isArray(value.matches)
    && value.matches.every(match => fields(match, ['source', 'rule_id', 'decision', 'reason', 'specificity',
      'applied', 'branch', 'requirement_checks'], ['fallback', 'precondition', 'decision_provenance',
      'operation_ref', 'executable', 'rule_ref'])
      && text(match.source) && text(match.rule_id, 128) && decisions.includes(match.decision)
      && typeof match.reason === 'string' && Number.isSafeInteger(match.specificity)
      && typeof match.applied === 'boolean' && ['met', 'unmet'].includes(match.branch)
      && Array.isArray(match.requirement_checks) && match.requirement_checks.every(check)
      && (!Object.hasOwn(match, 'rule_ref') || reference(match.rule_ref))
      && (!Object.hasOwn(match, 'fallback') || typeof match.fallback === 'boolean')
      && (!Object.hasOwn(match, 'precondition') || typeof match.precondition === 'boolean')
      && (!Object.hasOwn(match, 'executable') || nullableText(match.executable))
      && (!Object.hasOwn(match, 'operation_ref') || text(match.operation_ref, 128))
      && !(match.applied && match.decision === 'deny' && decision !== 'deny'))
    && Array.isArray(value.requirement_checks) && value.requirement_checks.every(check)
    && Array.isArray(value.guidance) && value.guidance.every(guidance);
}

function history(value) {
  return fields(value, ['status', 'request_id', 'event_id'])
    && ['recorded', 'disabled', 'dropped'].includes(value.status)
    && (value.status === 'recorded' ? text(value.request_id, 128) && text(value.event_id, 128)
      : value.request_id === null && value.event_id === null);
}

function normalized(value) {
  const input = value.input;
  return fields(input, ['kind', 'action', 'sources', 'destination', 'recursive', 'original_input'])
    && input.kind === 'file' && input.action === 'read' && input.destination === null && input.recursive === false
    && Array.isArray(input.sources) && input.sources.length === 1 && text(input.sources[0])
    && object(input.original_input) && fields(value.context, ['cwd', 'environment'])
    && text(value.context.cwd) && value.context.environment === null;
}

/** Validate only owned child output; no receipt can authorize a reconstructed input. */
export function response(bytes, exit, expected) {
  if (bytes.length > 4_194_304) throw Error('invalid response');
  const value = parseJSON(bytes);
  if (!bounded(value) || !fields(value, ['protocol_version', 'correlation_id', 'operation', 'project_id',
    'request_id', 'policy_revision', 'result', 'error']) || value.protocol_version !== PROTOCOL_VERSION
    || value.correlation_id !== expected.correlation_id || value.operation !== expected.operation
    || !(value.error !== null && value.project_id === null || text(value.project_id, 128))
    || (expected.project_id !== undefined && value.project_id !== null && value.project_id !== expected.project_id)
    || !nullableText(value.request_id, 128)
    || !nullableText(value.policy_revision, 128)) throw Error('invalid response envelope');
  if (value.error !== null) {
    const error = value.error;
    if (exit !== 2 || value.result !== null || !fields(error, ['code', 'message', 'operator_route'])
      || !codes.includes(error.code) || error.message !== messages[error.code]
      || !nullableText(error.operator_route)) throw Error('invalid error response');
    return value;
  }
  if (exit !== 0 || value.result === null) throw Error('invalid outcome');
  const result = value.result;
  if (expected.operation === 'history.observe') {
    if (value.request_id !== null || value.policy_revision !== null
      || !fields(result, ['status', 'event_id', 'correlation', 'reported_request_id'])
      || !['recorded', 'disabled', 'dropped'].includes(result.status)
      || result.correlation !== 'deferred' || result.reported_request_id !== (expected.reported_request_id ?? null)
      || (result.status === 'recorded' ? !text(result.event_id, 128) : result.event_id !== null)) {
      throw Error('invalid observation receipt');
    }
    return value;
  }
  if (!['runtime.evaluate', 'runtime.consume'].includes(expected.operation)
    || !fields(result, ['mode', 'decision', 'disposition', 'normalized_request', 'explanation', 'analysis_coverage',
      'approval_request_id', 'requirement_checks', 'guidance', 'retry_ref', 'approval_id', 'consumed', 'history', 'input_digest'])
    || result.mode !== 'runtime' || !decisions.includes(result.decision)
    || !['allowed', 'approval_required', 'blocked'].includes(result.disposition)
    || !text(value.policy_revision, 128) || !digest(result.input_digest) || result.input_digest !== expected.inputDigest
    || !fields(result.normalized_request, ['input', 'context'])
    || !normalized(result.normalized_request)
    || !explanation(result.explanation, result.decision)
    || !fields(result.analysis_coverage, ['model', 'complete', 'unknown'])
    || result.analysis_coverage.model !== 'bounded-sdk-text-read-v1' || result.analysis_coverage.complete !== false
    || result.analysis_coverage.unknown !== 'Host performs the read after the callback.'
    || !isDeepStrictEqual(result.requirement_checks, result.explanation.requirement_checks)
    || !isDeepStrictEqual(result.guidance, result.explanation.guidance)
    || !nullableText(result.approval_request_id, 128) || !nullableText(result.approval_id, 128)
    || !(result.retry_ref === null || digest(result.retry_ref)) || typeof result.consumed !== 'boolean'
    || !history(result.history)) throw Error('invalid runtime result');
  const consume = expected.operation === 'runtime.consume';
  if ((!consume && (result.consumed || result.approval_id !== null))
    || (!result.consumed && result.approval_id !== null)
    || (result.disposition === 'approval_required' && result.decision !== 'ask')
    || (result.decision === 'deny' && result.disposition !== 'blocked')
    || (result.decision === 'allow' && (result.disposition !== 'allowed' || result.consumed))
    || (result.decision === 'ask' && result.disposition === 'allowed'
      && (!consume || !result.consumed || !text(result.approval_id, 128)
        || value.request_id !== expected.request_id || result.retry_ref !== expected.retry_ref))
    || (result.consumed && (result.decision !== 'ask' || result.disposition !== 'allowed'))
    || (result.approval_request_id !== null && (result.decision !== 'ask'
      || result.approval_request_id !== value.request_id || result.retry_ref === null))) {
    throw Error('contradictory runtime authority');
  }
  return value;
}
