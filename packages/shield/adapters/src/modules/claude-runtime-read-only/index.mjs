import { realpath, readFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { transport } from '../process-transport/index.mjs';
import { response, PROTOCOL_VERSION } from '../runtime-response/index.mjs';
import { pinBinary, launchEnvironment } from './binary.mjs';

const TOKEN = 'SHIELD_SDK_READ_CONTEXT_TOKEN';
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value)
  && [Object.prototype, null].includes(Object.getPrototypeOf(value));
const fields = (value, allowed) => object(value) && Object.keys(value).every(key => allowed.includes(key));
const text = (value, max = 4096) => typeof value === 'string' && value.length > 0
  && Buffer.byteLength(value) <= max && !value.includes('\0');
const identifier = value => text(value, 128) && /^[a-zA-Z0-9._:-]+$/.test(value);
const positive = value => Number.isSafeInteger(value) && value > 0 && value <= 1_000_000;
const common = ['session_id', 'transcript_path', 'cwd', 'permission_mode', 'hook_event_name',
  'tool_name', 'tool_input', 'tool_use_id', 'agent_id', 'agent_type', 'mcp_server', 'prompt_id', 'effort', 'scratchpad_dir'];
const warning = () => ({ systemMessage: 'Shield history unavailable. The protection decision is unchanged.' });
const deny = request => ({ hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'deny',
  permissionDecisionReason: request ? `Shield requires operator approval for request ${request}.` : 'Shield could not authorize this registered text read.' } });

async function mappingDigest() {
  const hash = createHash('sha256').update('shield-sdk-read-mapping-v1\0');
  for (const path of ['claude-runtime-read-only/index.mjs', 'claude-runtime-read-only/binary.mjs', 'runtime-response/index.mjs',
    'enforcement-response/json.mjs', 'process-transport/index.mjs']) {
    const name = Buffer.from(path);
    const bytes = await readFile(new URL(`../${path}`, import.meta.url));
    for (const part of [name, bytes]) {
      const length = Buffer.alloc(8);
      length.writeBigUInt64BE(BigInt(part.length));
      hash.update(length).update(part);
    }
  }
  return hash.digest('hex');
}

// Capture only the closed vendor mapping before any asynchronous operation.
function capture(input, toolUseId, completion = false) {
  if (!fields(input, completion ? [...common, 'tool_response', 'error', 'is_interrupt', 'duration_ms'] : common)
    || input.tool_name !== 'Read' || !text(input.cwd) || !isAbsolute(input.cwd)
    || !identifier(input.tool_use_id) || (toolUseId !== undefined && toolUseId !== input.tool_use_id)
    || (input.session_id !== undefined && !identifier(input.session_id))
    || !(completion ? ['PostToolUse', 'PostToolUseFailure'].includes(input.hook_event_name) : input.hook_event_name === 'PreToolUse')
    || (completion && input.is_interrupt !== undefined && typeof input.is_interrupt !== 'boolean')
    || !fields(input.tool_input, ['file_path', 'offset', 'limit'])
    || !text(input.tool_input.file_path) || !isAbsolute(input.tool_input.file_path)
    || (Object.hasOwn(input.tool_input, 'offset') && !positive(input.tool_input.offset))
    || (Object.hasOwn(input.tool_input, 'limit') && !positive(input.tool_input.limit))) throw Error('Invalid read mapping');
  const tool = { file_path: input.tool_input.file_path,
    ...(Object.hasOwn(input.tool_input, 'offset') ? { offset: input.tool_input.offset } : {}),
    ...(Object.hasOwn(input.tool_input, 'limit') ? { limit: input.tool_input.limit } : {}) };
  return { input: { kind: 'file', action: 'read', sources: [tool.file_path], destination: null,
    recursive: false, original_input: tool }, context: { cwd: input.cwd, environment: null },
  provenance: { session_id: input.session_id ?? null, tool_use_id: input.tool_use_id, event: input.hook_event_name },
  interrupted: input.is_interrupt === true };
}

/** Explicit opt-in to the reviewed general-runtime-v1 bounded Read profile.
 * The token and retry capabilities stay in this trusted closure, outside model tool data.
 */
export async function createClaudeRuntimeReadOnlyClient(options) {
  if (!fields(options, ['binary', 'binaryDigest', 'project', 'timeoutMs', 'onApprovalRequired']) || !text(options.binary) || !isAbsolute(options.binary)
    || !text(options.project) || !isAbsolute(options.project)
    || (options.onApprovalRequired !== undefined && typeof options.onApprovalRequired !== 'function')) throw Error('Invalid Shield runtime registration');
  const onApprovalRequired = options.onApprovalRequired;
  const timeoutMs = options.timeoutMs ?? 5000;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 60_000) throw Error('Invalid Shield deadline');
  const selectedBinary = options.binary;
  const binaryDigest = options.binaryDigest;
  const selectedProject = options.project;
  const [binary, project] = await Promise.all([realpath(selectedBinary), realpath(selectedProject)]);
  const pin = await pinBinary(binary, binaryDigest);
  const launchEnv = launchEnvironment(process.env);
  let token;
  try {
    token = await pin.guard(async () => transport({ binary, args: ['__internal', 'sdk-read-open', '--mapping-digest', await mappingDigest(), '--project', project],
      timeoutMs, input: Buffer.alloc(0), env: launchEnv, stdoutLimit: 65 }, (bytes, exit) => {
      if (exit !== 0 || bytes.length !== 65 || !/^[a-f0-9]{64}\n$/.test(bytes.toString('ascii'))) throw Error('Invalid context');
      return bytes.toString('ascii').slice(0, 64);
    }));
  } catch (error) {
    await pin.close();
    if (error?.code === 'binary_identity' || error?.code === 'unsupported_platform') throw error;
    throw Error('Shield SDK context could not open. Verify the registered binary and retry with a new client.');
  }
  let projectId;
  let closing = false;
  let closed = false;
  const inflight = new Set();
  const pending = new Map();
  const completed = new Map();
  const seen = new Map();
  const controllers = new Set();
  const prune = map => {
    const now = Date.now();
    for (const [key, entry] of map) if (now - entry.at > 70 * 60_000) map.delete(key);
    while (map.size > 128) map.delete(map.keys().next().value);
  };
  const track = (operation, overloaded) => {
    if (inflight.size >= 8 || closing || closed) return Promise.resolve(overloaded());
    const promise = operation();
    inflight.add(promise);
    promise.then(() => inflight.delete(promise), () => inflight.delete(promise));
    return promise;
  };
  const invoke = async (operation, payload, signal, deadline) => {
    const remaining = deadline === undefined ? timeoutMs : Math.min(timeoutMs, Math.floor(deadline - performance.now()));
    if (remaining < 1 || signal?.aborted) throw Error('Shield callback expired');
    const frame = Buffer.from(JSON.stringify({ protocol_version: PROTOCOL_VERSION,
      correlation_id: randomUUID(), operation, payload }));
    if (frame.length > 524_288) throw Error('Shield request limit');
    const envelope = JSON.parse(frame.toString('utf8'));
    const result = await pin.guard(() => transport({ binary, args: ['__internal', 'runtime-request', '--project', project, '--timeout-ms', String(remaining)],
      timeoutMs: remaining, signal, input: frame, env: { ...launchEnv, [TOKEN]: token }, expected: {
        correlation_id: envelope.correlation_id, operation, project_id: projectId,
        inputDigest: createHash('sha256').update(frame).digest('hex'), request_id: payload.request_id,
        retry_ref: payload.retry_ref, reported_request_id: payload.request_id } }, response));
    if (result.error !== null) throw Error('Shield runtime refused the request');
    projectId ??= result.project_id;
    return result;
  };
  const callback = (input, toolUseId, context = {}) => track(async () => {
    const controller = new AbortController();
    const deadline = performance.now() + timeoutMs;
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    controllers.add(controller);
    try {
      if (closing || closed || (context.signal !== undefined && !(context.signal instanceof AbortSignal))) return deny();
      const signal = context.signal ? AbortSignal.any([context.signal, controller.signal]) : controller.signal;
      const call = capture(input, toolUseId);
      const reportedCwd = call.context.cwd;
      const identity = JSON.stringify([call.provenance.session_id, call.provenance.tool_use_id]);
      prune(pending); prune(completed);
      const ambiguous = seen.has(identity);
      const generation = randomUUID();
      const tracked = ambiguous || seen.size < 128;
      if (tracked) seen.set(identity, { generation });
      pending.delete(identity);
      completed.delete(identity);
      const effectiveCwd = await realpath(reportedCwd);
      const payload = { input: call.input, context: call.context, provenance: call.provenance,
        retry_ref: null };
      let result = await invoke('runtime.evaluate', payload, signal, deadline);
      if (result.result.disposition === 'approval_required' && onApprovalRequired !== undefined) {
        const { approval_request_id: requestId, retry_ref: retryRef } = result.result;
        if (!requestId || !retryRef) throw Error('Missing private continuation');
        let abort;
        try {
          await Promise.race([Promise.resolve().then(() => onApprovalRequired(Object.freeze({ request_id: requestId }), Object.freeze({ signal }))),
            new Promise((_, reject) => { abort = () => reject(Error('Shield callback expired')); signal.addEventListener('abort', abort, { once: true }); if (signal.aborted) abort(); })]);
        } finally { if (abort) signal.removeEventListener('abort', abort); }
        result = await invoke('runtime.consume', { ...payload, request_id: requestId, retry_ref: retryRef }, signal, deadline);
      }
      const receipt = result.result;
      const diagnostic = receipt.history.status === 'dropped' ? warning() : {};
      if (receipt.disposition !== 'allowed') return { ...deny(receipt.approval_request_id), ...diagnostic };
      if (tracked && !ambiguous && seen.get(identity)?.generation === generation) pending.set(identity, {
        call: { ...call, context: { ...call.context, cwd: effectiveCwd } }, reportedCwd,
        request_id: result.request_id, at: Date.now() });
      else pending.delete(identity);
      prune(pending);
      await pin.check();
      return { ...diagnostic, hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'allow',
        permissionDecisionReason: receipt.consumed ? 'Shield consumed operator approval for this registered text read.' : 'Shield active policy permits this registered text read.',
        updatedInput: { ...call.input.original_input } } };
    } catch { return { ...deny(), ...warning() }; }
    finally { clearTimeout(timer); controllers.delete(controller); }
  }, () => ({ ...deny(), ...warning() }));
  const completion = (input, toolUseId, context = {}) => track(async () => {
    try {
      if (closing || closed || (context.signal !== undefined && !(context.signal instanceof AbortSignal))) return warning();
      const call = capture(input, toolUseId, true);
      const identity = JSON.stringify([call.provenance.session_id, call.provenance.tool_use_id]);
      prune(pending); prune(completed);
      const prior = pending.get(identity);
      pending.delete(identity);
      const saved = completed.get(identity);
      const matches = entry => entry && entry.reportedCwd === call.context.cwd && JSON.stringify(entry.call.input) === JSON.stringify(call.input);
      const exact = matches(prior) ? prior : matches(saved) ? saved : undefined;
      if (exact) call.context = exact.call.context;
      const requestId = exact ? exact.request_id : null;
      const baseId = exact ? requestId ?? exact.delivery_id ?? randomUUID() : undefined;
      const deliveryId = exact ? `${baseId}-${call.provenance.event}` : randomUUID();
      if (exact && !saved) {
        completed.set(identity, { ...exact, delivery_id: baseId, at: Date.now() });
        prune(completed);
      }
      const result = await invoke('history.observe', { input: call.input, context: call.context, provenance: call.provenance,
        delivery_id: deliveryId, request_id: requestId,
        observation: { kind: call.provenance.event === 'PostToolUse' ? 'completed' : call.interrupted ? 'cancelled' : 'failed',
          occurred_at: null, run_id: null, exit_code: null, signal: null, host_approval: null } }, context.signal);
      return result.result.status === 'dropped' ? warning() : {};
    } catch { return warning(); }
  }, warning);
  const matcher = hook => Object.freeze([Object.freeze({ hooks: Object.freeze([hook]), timeout: Math.ceil((timeoutMs + 1000) / 1000) })]);
  const hooks = Object.freeze({ PreToolUse: matcher(callback), PostToolUse: matcher(completion), PostToolUseFailure: matcher(completion) });
  return Object.freeze({ hooks, close: async () => {
    if (closed) return;
    closing = true;
    for (const controller of controllers) controller.abort();
    await Promise.allSettled([...inflight]);
    try {
      await pin.guard(() => transport({ binary, args: ['__internal', 'sdk-read-close', '--project', project], timeoutMs,
        input: Buffer.alloc(0), stdoutLimit: 0, env: { ...launchEnv, [TOKEN]: token } }, (bytes, exit) => {
        if (exit !== 0 || bytes.length !== 0) throw Error('Invalid close');
      }));
    } catch { await pin.close(); closed = true; token = undefined; throw Error('Shield SDK context could not close.'); }
    closed = true;
    token = undefined;
    await pin.close();
    pending.clear(); completed.clear(); seen.clear();
  } });
}
