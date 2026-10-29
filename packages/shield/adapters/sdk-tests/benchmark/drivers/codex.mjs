// Real Codex through its app-server protocol: one persisted thread, scenario-owned approvals.
import { spawn } from 'node:child_process';
import { appendFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

export async function startCodex({ project, home, logs, executable, decisions, limits, record, environment = {}, stubbedTools = false, writableRoots = [] }) {
  // Project configuration (Shield's hooks and MCP server) loads only for trusted projects.
  await appendFile(join(home, 'config.toml'), `\n[projects.${JSON.stringify(project)}]\ntrust_level = "trusted"\n`);
  const env = { ...process.env, ...environment, CODEX_HOME: home, SHIELD_DEBUG_LOG_DIR: logs };
  // Subscription login only: an inherited key would bill the run to an API account.
  for (const name of ['OPENAI_API_KEY', 'OPENAI_BASE_URL', 'CODEX_API_KEY']) delete env[name];
  const child = spawn(executable, ['app-server'], { cwd: project, env, stdio: ['pipe', 'pipe', 'pipe'] });
  const session = { id: null, model: null, version: null, rollout: null, tokens: null, background: {} };
  const pending = new Map();
  let sequence = 0;
  let turnWaiter;
  let failure;
  const shield = Promise.withResolvers();
  shield.promise.catch(() => {});
  const send = value => child.stdin.write(`${JSON.stringify(value)}\n`);
  const fail = error => {
    failure ??= error;
    for (const { reject } of pending.values()) reject(error);
    pending.clear();
    shield.reject(error);
    turnWaiter?.reject(error);
  };
  child.on('error', fail);
  const closed = new Promise(resolve => child.on('close', code => { fail(new Error(`Codex app-server exited (${code})`)); resolve(); }));
  child.stderr.on('data', bytes => record({ host: 'codex', stderr: String(bytes) }));
  const respond = (id, result) => send({ id, result });

  createInterface({ input: child.stdout }).on('line', line => {
    let message;
    try { message = JSON.parse(line); } catch { record({ host: 'codex', unparsed: line }); return; }
    record({ host: 'codex', message });
    if (message.method && message.id !== undefined) {
      const { method, params } = message;
      if (method === 'item/commandExecution/requestApproval' || method === 'item/fileChange/requestApproval') {
        respond(message.id, { decision: decisions.command('codex', { method, params }) });
      } else if (method === 'mcpServer/elicitation/request') {
        // Codex's prompt text names only the tool; the change itself is in the displayed tool parameters.
        const shown = [params.message, params._meta?.tool_params?.change, params._meta?.tool_params?.details].filter(Boolean).join('\n');
        const action = decisions.offer('codex', shown, { server: params.serverName, mode: params.mode, meta: params._meta });
        respond(message.id, { action, content: action === 'accept' ? {} : null, _meta: null });
      } else {
        // Permission profiles, user questions and dynamic tools have no scripted answer.
        decisions.unsupported('codex', { method, params });
        send({ id: message.id, error: { code: -32601, message: 'No user is available to answer this request in this session.' } });
      }
      return;
    }
    if (message.id !== undefined) {
      const waiter = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) waiter?.reject(new Error(JSON.stringify(message.error)));
      else waiter?.resolve(message.result);
      return;
    }
    const { method, params } = message;
    if (method === 'mcpServer/startupStatus/updated' && params?.name === 'shield') {
      if (params.status === 'ready') shield.resolve();
      else if (params.status !== 'starting') shield.reject(new Error(`Shield MCP: ${JSON.stringify(params)}`));
    } else if (method === 'thread/tokenUsage/updated') {
      session.tokens = params.tokenUsage ?? params;
      const total = session.tokens?.total?.totalTokens;
      if (total > limits.codex_max_total_tokens) fail(new Error(`token limit reached (${total})`));
    } else if (method === 'item/commandExecution/outputDelta' || method === 'process/outputDelta') {
      const key = params.itemId ?? params.processId ?? 'unknown';
      session.background[key] = (session.background[key] ?? 0) + (params.delta?.length ?? 0);
    } else if (method === 'turn/completed') {
      turnWaiter?.resolve(params.turn);
    }
  });

  const request = (method, params) => new Promise((resolve, reject) => {
    if (failure) return reject(failure);
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    send({ id, method, params });
  });
  // MCP startup is asynchronous; a turn started earlier can miss Shield's tools.
  const shieldReady = Promise.race([shield.promise,
    new Promise((_, reject) => setTimeout(() => reject(new Error('Shield MCP did not start')), 60000))]);
  shieldReady.catch(() => {});

  await request('initialize', { clientInfo: { name: 'shield-live-acceptance', version: '1' }, capabilities: { experimentalApi: true } });
  send({ method: 'initialized' });
  // The run stands in for the user's one-time /hooks review of Shield's generated hooks.
  // A login shell runs macOS path_helper, which puts system folders ahead of the
  // stub folder on PATH and would launch the real programs instead.
  const started = await request('thread/start', { cwd: project, approvalPolicy: 'on-request', sandbox: 'workspace-write',
    config: { bypass_hook_trust: true, ...(stubbedTools ? { allow_login_shell: false } : {}),
      // Tool stubs log their calls outside the project.
      ...(writableRoots.length ? { sandbox_workspace_write: { writable_roots: writableRoots } } : {}) } });
  session.id = started.thread.id;
  session.model = started.model;
  session.version = started.thread.cliVersion;
  session.rollout = started.thread.path;
  await shieldReady;

  return {
    session,
    async turn(prompt) {
      const done = new Promise((resolve, reject) => { turnWaiter = { resolve, reject }; });
      await request('turn/start', { threadId: session.id, input: [{ type: 'text', text: prompt }] });
      const timer = setTimeout(() => turnWaiter.reject(new Error('turn time limit reached')), limits.turn_minutes * 60000);
      try {
        const turn = await done;
        return { status: turn.status, error: turn.error ?? null, tokens: session.tokens };
      } finally { clearTimeout(timer); }
    },
    async backgroundSnapshot() { return { ...session.background }; },
    async transcript() { return session.rollout; },
    async close() {
      child.stdin.end();
      child.kill('SIGTERM');
      const force = setTimeout(() => child.kill('SIGKILL'), 3000);
      await closed;
      clearTimeout(force);
    },
  };
}
