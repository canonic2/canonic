// Independent observations after each turn. Nothing here talks to the agent.
import { spawn } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { configurationDigests, exec, listeningPorts, ownedProcesses, shieldJson } from './project.mjs';

/** Calls read-only Shield MCP tools on a separate observer connection. */
export async function shieldTools(shield, project, calls) {
  const child = spawn(shield, ['mcp', 'serve', '--project', project], { stdio: ['pipe', 'pipe', 'pipe'] });
  const frames = [
    { jsonrpc: '2.0', id: 0, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'shield-live-observer', version: '1' } } },
    { jsonrpc: '2.0', method: 'notifications/initialized' },
    ...calls.map(([name, args], index) => ({ jsonrpc: '2.0', id: index + 1, method: 'tools/call', params: { name, arguments: args } })),
  ];
  let stdout = '';
  child.stdout.on('data', bytes => { stdout += bytes; });
  child.stdin.end(frames.map(frame => JSON.stringify(frame)).join('\n') + '\n');
  await new Promise(resolve => child.on('close', resolve));
  const replies = new Map(stdout.split('\n').filter(Boolean).map(line => JSON.parse(line)).map(reply => [reply.id, reply]));
  return calls.map((_, index) => replies.get(index + 1)?.result?.structuredContent ?? replies.get(index + 1) ?? null);
}

async function git(project, args) {
  try { return (await exec('git', ['-C', project, ...args], { maxBuffer: 16 << 20 })).stdout; } catch (error) {
    return { error: error.stderr?.trim() || error.message };
  }
}

/** Finds the rule object with this ID anywhere in a candidate's parsed sources. */
export function findRule(value, id) {
  if (typeof value === 'string' && value.includes(id)) {
    try { return findRule(JSON.parse(value), id); } catch { return null; }
  }
  if (Array.isArray(value)) {
    for (const item of value) { const found = findRule(item, id); if (found) return found; }
  } else if (value && typeof value === 'object') {
    if (value.id === id && value.conditions) return value;
    for (const item of Object.values(value)) { const found = findRule(item, id); if (found) return found; }
  }
  return null;
}

const packIds = metadata => (metadata?.installed ?? []).map(row => `${row.id}@${row.version}:${row.profile}`);

async function readState(project) {
  const state = JSON.parse(await readFile(join(project, '.shield/review/state.json'), 'utf8'));
  return {
    policy_revision: state.policy_revision,
    active: state.active,
    proposals: (state.proposals ?? []).map(proposal => {
      const before = new Set(packIds(proposal.base_metadata));
      return {
        id: proposal.proposal_id, status: proposal.status, created_at: proposal.created_at,
        operation: proposal.operation, rule_id: proposal.rule_id || null, target_scope: proposal.target_scope,
        session_id: proposal.provenance?.session_id ?? null, agent: proposal.provenance?.agent ?? null,
        links: proposal.links, agent_reason: proposal.reasons?.agent_reason ?? null,
        packs_added: packIds(proposal.candidate_metadata).filter(id => !before.has(id)),
        rule: proposal.rule_id ? findRule(proposal.candidate, proposal.rule_id) : null,
        receipt: proposal.receipt ?? null,
      };
    }),
  };
}

export const packRows = packs => packs?.map(pack => ({ id: pack.id, version: pack.version, profile: pack.profile, digest: pack.digest }));

/** Tool stub calls logged so far, in order. */
export async function toolCalls(run) {
  try { return (await readFile(run.toolLog, 'utf8')).split('\n').filter(Boolean).map(line => JSON.parse(line)); } catch (error) {
    if (error.code === 'ENOENT') return [];
    throw error;
  }
}

/** Shield, Git, configuration and process state after one turn. */
export async function observe({ shield, run, sessionIds }) {
  const { project } = run;
  const history = await shieldJson(shield, project, ['history', 'list']);
  const status = await shieldJson(shield, project, ['status']);
  const rules = await shieldJson(shield, project, ['rule', 'list']).catch(error => ({ error: error.message }));
  const sessions = new Set([...sessionIds, ...(history.entries ?? []).map(entry => entry.provenance?.session_id).filter(Boolean)]);
  const suggestions = {};
  if (sessions.size) {
    const replies = await shieldTools(shield, project, [...sessions].map(id => ['suggestion_list', { session_id: id, limit: 100 }]));
    [...sessions].forEach((id, index) => { suggestions[id] = replies[index]; });
  }
  const processes = await ownedProcesses(project);
  return {
    at: new Date().toISOString(),
    history,
    status: { policy_revision: status.policy_revision, packs: packRows(status.installed_packs?.packs), repair_needed: status.repair_needed },
    state: await readState(project),
    custom_rules: (rules.rules ?? []).filter(rule => !rule.source?.selector)
      .map(({ id, decision, conditions, rule_ref, source }) => ({ id, decision, conditions, rule_ref, source })),
    suggestions,
    git: { log: await git(project, ['log', '--format=%H%x09%s', '--name-only']), status: await git(project, ['status', '--porcelain', '--untracked-files=all']), tracked: await git(project, ['ls-files']) },
    config: await configurationDigests(project),
    processes,
    ports: await listeningPorts(processes.map(row => row.pid)),
    tool_calls: (await toolCalls(run)).length,
  };
}
