// Real Claude Code through the Agent SDK: one session for every turn, scenario-owned approvals.
import { query } from '@anthropic-ai/claude-agent-sdk';
import { readdir, stat } from 'node:fs/promises';
import { join } from 'node:path';

function inputQueue() {
  const waiting = [];
  const queued = [];
  let closed = false;
  return {
    push(text) {
      const message = { type: 'user', message: { role: 'user', content: text }, parent_tool_use_id: null, session_id: '' };
      if (waiting.length) waiting.shift()({ value: message, done: false });
      else queued.push(message);
    },
    close() {
      closed = true;
      while (waiting.length) waiting.shift()({ value: undefined, done: true });
    },
    [Symbol.asyncIterator]() {
      return { next: () => queued.length ? Promise.resolve({ value: queued.shift(), done: false })
        : closed ? Promise.resolve({ value: undefined, done: true })
          : new Promise(resolve => waiting.push(resolve)) };
    },
  };
}

function text(content) {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) return content.map(block => block.text ?? text(block.content) ?? '').join('\n');
  return '';
}

export async function startClaude({ project, home, logs, executable, decisions, limits, record, environment = {} }) {
  const input = inputQueue();
  const env = { ...process.env, ...environment, CLAUDE_CONFIG_DIR: home, SHIELD_DEBUG_LOG_DIR: logs };
  // Subscription login only: an inherited key or gateway would bill or reroute the run.
  for (const name of ['ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_BASE_URL', 'CLAUDE_CODE_USE_BEDROCK', 'CLAUDE_CODE_USE_VERTEX']) delete env[name];
  const session = { id: null, model: null, version: null, cost: 0, backgroundFiles: new Set() };
  let resolveTurn;
  let rejectTurn;
  const q = query({
    prompt: input,
    options: {
      cwd: project,
      env,
      pathToClaudeCodeExecutable: executable,
      systemPrompt: { type: 'preset', preset: 'claude_code' },
      settingSources: ['user', 'project', 'local'],
      // Account-level claude.ai connectors would add unrelated tools to the model's view.
      settings: { disableClaudeAiConnectors: true },
      permissionMode: 'default',
      maxBudgetUsd: limits.claude_max_budget_usd,
      stderr: line => record({ host: 'claude', stderr: line }),
      canUseTool: async (tool, toolInput, options) => {
        const detail = { tool, input: toolInput, title: options.title, reason: options.decisionReason, tool_use_id: options.toolUseID };
        if (tool === 'AskUserQuestion') {
          // An unattended run has no one to answer; say so instead of inventing answers.
          decisions.unsupported('claude', detail);
          return { behavior: 'deny', message: 'No user is available to answer questions in this session.' };
        }
        // Policy activation is approved through Shield's confirmation form, not this prompt.
        decisions.command('claude', detail);
        return { behavior: 'allow', updatedInput: toolInput };
      },
      onElicitation: async request => {
        const action = decisions.offer('claude', request.message ?? '', { server: request.serverName, title: request.title });
        return action === 'accept' ? { action: 'accept', content: {} } : { action };
      },
    },
  });
  const pump = (async () => {
    for await (const message of q) {
      record({ host: 'claude', message });
      if (message.type === 'system' && message.subtype === 'init') {
        session.id = message.session_id;
        session.model = message.model;
        session.version = message.claude_code_version ?? null;
        session.tools = message.tools;
        session.mcp = message.mcp_servers;
      }
      if (message.type === 'user') {
        for (const block of Array.isArray(message.message?.content) ? message.message.content : []) {
          const output = block.type === 'tool_result' ? text(block.content) : '';
          // The host ends this sentence with a period that is not part of the path.
          for (const match of output.matchAll(/Output is being written to:\s*(\S+?)\.?(?:\s|$)/g)) session.backgroundFiles.add(match[1]);
        }
      }
      if (message.type === 'result') {
        session.cost = message.total_cost_usd ?? session.cost;
        resolveTurn?.(message);
      }
    }
  })().catch(error => rejectTurn?.(error));
  pump.catch(() => {});

  return {
    session,
    async turn(prompt) {
      const done = new Promise((resolve, reject) => { resolveTurn = resolve; rejectTurn = reject; });
      input.push(prompt);
      const timer = setTimeout(() => rejectTurn(new Error('turn time limit reached')), limits.turn_minutes * 60000);
      try {
        const result = await done;
        return { status: result.subtype, is_error: result.is_error, cost_usd: session.cost, usage: result.usage, final: result.result };
      } finally { clearTimeout(timer); }
    },
    /** Sizes of background command output files the host reported, for watcher reruns. */
    async backgroundSnapshot() {
      const sizes = {};
      for (const file of session.backgroundFiles) {
        try { sizes[file] = (await stat(file)).size; } catch { sizes[file] = null; }
      }
      return sizes;
    },
    async transcript() {
      if (!session.id) return null;
      const projects = join(home, 'projects');
      for (const directory of await readdir(projects).catch(() => [])) {
        const path = join(projects, directory, `${session.id}.jsonl`);
        if (await stat(path).then(() => true, () => false)) return path;
      }
      return null;
    },
    async close() {
      input.close();
      try { await q.interrupt(); } catch {}
      q.close?.();
      await Promise.race([pump, new Promise(resolve => setTimeout(resolve, 5000))]);
    },
  };
}
