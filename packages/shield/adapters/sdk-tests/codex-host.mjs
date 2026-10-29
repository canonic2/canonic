// Isolated native approval host using Codex's actual app-server protocol.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

export async function approvalHost(root, home, decision, mcpDecision, options = {}) {
  const child = spawn('codex', ['--disable', 'plugins', '--disable', 'remote_plugin', 'app-server'], {
    env: { ...process.env, CODEX_HOME: home }, stdio: ['pipe', 'pipe', 'pipe'],
  });
  let stderr = '';
  const messages = [];
  const pending = new Map();
  let sequence = 0;
  const approvals = [];
  const mcpApprovals = [];
  const lines = createInterface({ input: child.stdout });
  let finish;
  let fail;
  let ready;
  let notReady;
  const shieldReady = new Promise((resolve, reject) => { ready = resolve; notReady = reject; });
  shieldReady.catch(() => {});
  const complete = new Promise((resolve, reject) => { finish = resolve; fail = reject; });
  // Attach before startup RPCs, so a startup failure cannot become unhandled.
  complete.catch(() => {});
  const stop = error => {
    for (const { reject } of pending.values()) reject(error);
    pending.clear();
    notReady(error);
    fail(error);
  };
  child.on('error', stop);
  const closed = new Promise(resolve => child.on('close', code => {
    stop(new Error(`Codex app-server closed (${code}): ${stderr.slice(-1500)}`));
    resolve();
  }));
  child.stderr.on('data', bytes => { stderr = (stderr + bytes).slice(-8000); });
  const send = value => child.stdin.write(`${JSON.stringify(value)}\n`);
  lines.on('line', line => {
    try {
      const message = JSON.parse(line);
      messages.push(message);
      if (message.method && message.id !== undefined) {
        if (message.method === 'item/commandExecution/requestApproval') {
          approvals.push(message);
          send({ id: message.id, result: { decision } });
        } else if (message.method === 'mcpServer/elicitation/request' && mcpDecision) {
          mcpApprovals.push(message);
          send({ id: message.id, result: { action: mcpDecision, content: null, _meta: null } });
        } else {
          send({ id: message.id, error: { code: -32601, message: 'Unsupported Acme fixture request' } });
          stop(new Error(`Unexpected Codex host request: ${message.method}`));
        }
      } else if (message.id !== undefined) {
        const waiter = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) waiter?.reject(new Error(JSON.stringify(message.error)));
        else waiter?.resolve(message.result);
      } else if (options.waitForShield && message.method === 'mcpServer/startupStatus/updated' && message.params.name === 'shield') {
        if (message.params.status === 'ready') ready();
        else if (message.params.status !== 'starting') stop(new Error(`Shield MCP startup failed: ${JSON.stringify(message.params)}`));
      } else if (message.method === 'turn/completed') {
        if (message.params.turn.status === 'failed') stop(new Error(JSON.stringify(message.params.turn.error)));
        else finish();
      }
    } catch (error) { stop(error); }
  });
  const request = (method, params) => new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    send({ id, method, params });
  });
  const timer = setTimeout(() => stop(new Error('Codex approval fixture timed out')), 45000);
  try {
    await request('initialize', { clientInfo: { name: 'shield-acme-test', version: '1' }, capabilities: { experimentalApi: true } });
    send({ method: 'initialized' });
    const { thread } = await request('thread/start', {
      cwd: root, model: 'acme-model', modelProvider: 'acme', ephemeral: true,
      approvalPolicy: options.approvalPolicy ?? 'on-request', sandbox: 'workspace-write',
      config: { bypass_hook_trust: true },
    });
    // MCP startup is asynchronous. Starting the turn before this notification
    // can snapshot a tool list that does not contain Shield at all.
    if (options.waitForShield) await shieldReady;
    await request('turn/start', { threadId: thread.id, input: [{ type: 'text', text: 'Perform the Acme fixture command.' }] });
    await complete;
    return { stdout: JSON.stringify(messages), stderr, approvals, mcpApprovals };
  } finally {
    clearTimeout(timer);
    lines.close();
    child.stdin.end();
    child.kill('SIGTERM');
    const force = setTimeout(() => child.kill('SIGKILL'), 2000);
    await closed;
    clearTimeout(force);
  }
}
