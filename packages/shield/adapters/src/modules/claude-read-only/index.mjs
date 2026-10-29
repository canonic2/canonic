import { realpath } from 'node:fs/promises';
import { isAbsolute, resolve } from 'node:path';
import { transport } from '../process-transport/index.mjs';
import { randomUUID } from 'node:crypto';
import { response } from '../enforcement-response/index.mjs';
import { ApprovalReferences, PendingReads, denialReason } from './state.mjs';

const object = value => value !== null && typeof value === 'object' && !Array.isArray(value)
  && [Object.prototype, null].includes(Object.getPrototypeOf(value));
const fields = (value, allowed) => object(value) && Object.keys(value).every(key => allowed.includes(key));
const text = (value, max = 4096) => typeof value === 'string' && value.length > 0
  && Buffer.byteLength(value) <= max && !value.includes('\0');
const integer = value => Number.isSafeInteger(value) && value > 0 && value <= 1_000_000;
const deny = (code, request, detail, project) => ({ hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'deny',
  permissionDecisionReason: denialReason(code, request, detail, project) } });
/** Registered Claude Agent SDK callback hooks. Only bounded plain .txt Read tools can proceed. */
export async function createClaudeReadOnlyHooks(options) {
  if (!fields(options, ['binary', 'project', 'timeoutMs']) || !text(options.binary) || !isAbsolute(options.binary)
    || !text(options.project) || !isAbsolute(options.project)) throw Error('Invalid Shield registration');
  const timeoutMs = options.timeoutMs ?? 5000;
  const selectedBinary = options.binary;
  const selectedProject = options.project;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 60_000) throw Error('Invalid Shield deadline');
  const [binary, project] = await Promise.all([realpath(selectedBinary), realpath(selectedProject)]);
  const consumerId = randomUUID();
  const approvals = new ApprovalReferences();
  const invoke = (id, cwd, operation, projectId, signal, host, reference) => {
    const input = Buffer.from(JSON.stringify({ protocol_version: 1, request_id: id,
      expected_project_id: projectId ?? null, cwd, operation, host, approval_request_id: reference ?? null }));
    if (input.length > 65_536) throw Error('Shield request limit');
    return transport({ binary, args: ['__internal', 'enforce-request', '--project', project], timeoutMs, signal, input,
      expected: { id, project, projectId, reference, phase: operation.kind, operation: operation.kind === 'read-text' ? { ...operation, path: resolve(operation.path) } : undefined } },
    response);
  };
  const registered = await invoke('registration', project, { kind: 'register' });
  if (registered.code !== 'registration' || !registered.identity) throw Error('Shield runtime registration failed');
  const projectId = registered.identity.project_id;
  const pending = new PendingReads();
  const commonFields = ['session_id', 'transcript_path', 'cwd', 'permission_mode', 'hook_event_name',
    'tool_name', 'tool_input', 'tool_use_id', 'agent_id', 'agent_type', 'mcp_server', 'prompt_id', 'effort', 'scratchpad_dir'];
  const host = input => ({ agent: 'claude-sdk', session_id: input.session_id ?? null,
    tool_use_id: input.tool_use_id, event: input.hook_event_name, consumer_id: consumerId });
  const key = input => JSON.stringify([input.session_id ?? null, input.tool_use_id]);
  const readOperation = tool => ({ kind: 'read-text', path: tool.file_path, offset: tool.offset ?? null, limit: tool.limit ?? null });
  const validRead = tool => fields(tool, ['file_path', 'offset', 'limit']) && text(tool.file_path) && isAbsolute(tool.file_path)
    && (tool.offset === undefined || integer(tool.offset)) && (tool.limit === undefined || integer(tool.limit));
  const warning = () => ({ systemMessage: 'Shield history unavailable. The protection decision is unchanged.' });
  const callback = async (input, toolUseId, context = {}) => {
    try {
      if (!fields(input, commonFields)
        || input.hook_event_name !== 'PreToolUse' || !text(input.cwd) || !isAbsolute(input.cwd)
        || !text(input.tool_name, 128) || !text(input.tool_use_id, 128)
        || (toolUseId !== undefined && toolUseId !== input.tool_use_id)
        || (input.session_id !== undefined && !text(input.session_id))
        || !object(input.tool_input) || (context.signal !== undefined && !(context.signal instanceof AbortSignal))) {
        return { ...deny('invalid-host-input'), ...warning() };
      }
      const id = input.tool_use_id;
      const cwd = input.cwd;
      const identity = key(input);
      let operation = { kind: 'unsupported', name: input.tool_name };
      if (input.tool_name === 'Read') {
        const tool = input.tool_input;
        if (!validRead(tool)) {
          return { ...deny('invalid-read-input'), ...warning() };
        }
        operation = readOperation(tool);
      }
      const approvalKey = JSON.stringify([input.session_id ?? null, cwd, operation]);
      const reference = approvals.get(approvalKey);
      const result = await invoke(id, cwd, operation, projectId, context.signal, host(input), reference);
      approvals.record(approvalKey, result);
      const diagnostic = result.history.status === 'failed' ? warning() : {};
      if (result.decision !== 'allow') return { ...deny(result.code, result.approval.status === 'pending' ? result.approval.request_id : null, result.diagnostics?.message, project), ...diagnostic };
      const read = result.operation;
      pending.allow(identity, { operation, cwd, request: result.history.request_id });
      return { ...diagnostic, hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'allow',
        permissionDecisionReason: result.code === 'allowed-once' ? 'Shield consumed operator approval for this registered text read.' : 'Shield active policy permits this registered text read.',
        updatedInput: { file_path: read.path,
          ...(read.offset === null ? {} : { offset: read.offset }),
          ...(read.limit === null ? {} : { limit: read.limit }) } } };
    } catch {
      return { ...deny('protection-failure'), ...warning() };
    }
  };
  const completion = async (input, toolUseId, context = {}) => {
    try {
      if (!fields(input, [...commonFields, 'tool_response', 'error', 'is_interrupt', 'duration_ms'])
        || !['PostToolUse', 'PostToolUseFailure'].includes(input.hook_event_name)
        || input.tool_name !== 'Read' || !validRead(input.tool_input)
        || !text(input.cwd) || !isAbsolute(input.cwd) || !text(input.tool_use_id, 128)
        || (input.session_id !== undefined && !text(input.session_id))
        || (toolUseId !== undefined && toolUseId !== input.tool_use_id)
        || (input.is_interrupt !== undefined && typeof input.is_interrupt !== 'boolean')) return warning();
      const operation = readOperation(input.tool_input);
      const prior = pending.complete(key(input));
      const request = prior && prior.cwd === input.cwd && JSON.stringify(prior.operation) === JSON.stringify(operation)
        ? prior.request : null;
      const result = await invoke(input.tool_use_id, input.cwd, { kind: 'completion',
        delivery_id: request ? `${request}-${input.hook_event_name}` : randomUUID(), request_id: request,
        status: input.hook_event_name === 'PostToolUse' ? 'completed' : input.is_interrupt ? 'cancelled' : 'failed', operation,
      }, projectId, context.signal, host(input));
      return result.code === 'completion-observed' && result.history.status !== 'failed' ? {} : warning();
    } catch { return warning(); }
  };
  const matcher = hook => Object.freeze([Object.freeze({ hooks: Object.freeze([hook]), timeout: Math.ceil((timeoutMs + 1000) / 1000) })]);
  return Object.freeze({ PreToolUse: matcher(callback),
    PostToolUse: matcher(completion), PostToolUseFailure: matcher(completion) });
}
