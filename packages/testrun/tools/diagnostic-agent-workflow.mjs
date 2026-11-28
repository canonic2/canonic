// Agent-path diagnostics over `testrun mcp serve`: capabilities, plan, status,
// paged queries and projections followed through nextOperation, comparison and
// JSON/Markdown report export. Acceptance harnesses call it on their own
// retained investigation before cleanup; it collects nothing and changes no target.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFile} from 'node:fs/promises';
import {join} from 'node:path';
import {createInterface} from 'node:readline';

const TOOLS = ['diagnostics_capabilities', 'diagnostics_plan', 'diagnostics_status', 'diagnostics_query', 'diagnostics_project', 'diagnostics_compare', 'diagnostics_export'];
const MAX_PAGES = 64;

// One stdio MCP session against the harness's own data root and manager.
async function session(cli, cwd, env, record) {
  const child = spawn(cli, ['mcp', 'serve'], {cwd, env, stdio: ['pipe', 'pipe', 'pipe']});
  const pending = new Map(); let next = 1, stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-8192); });
  const exited = new Promise(accept => child.once('close', (code, signal) => accept({code, signal})));
  createInterface({input: child.stdout}).on('line', line => {
    const message = JSON.parse(line), waiter = pending.get(message.id);
    if (waiter) { pending.delete(message.id); waiter(message); }
  });
  exited.then(status => { for (const waiter of pending.values()) waiter({error: {message: `MCP server exited ${JSON.stringify(status)}: ${stderr}`}}); });
  async function request(method, params, timeoutMs = 60000) {
    const id = next++, before = performance.now(); let timer;
    const reply = await Promise.race([
      new Promise(accept => { pending.set(id, accept); child.stdin.write(JSON.stringify({jsonrpc: '2.0', id, method, params}) + '\n'); }),
      new Promise((_, reject) => { timer = setTimeout(() => reject(Error(`MCP ${method} reply deadline`)), timeoutMs); }),
    ]).finally(() => clearTimeout(timer));
    await record({mcp: method, params, reply, wallMs: performance.now() - before});
    assert.ok(!reply.error, `MCP ${method}: ${JSON.stringify(reply.error)}`);
    return reply.result;
  }
  const initialized = await request('initialize', {protocolVersion: '2025-06-18', capabilities: {}, clientInfo: {name: 'acme-acceptance', version: '1'}});
  child.stdin.write(JSON.stringify({jsonrpc: '2.0', method: 'notifications/initialized'}) + '\n');
  async function call(name, args) {
    const result = await request('tools/call', {name, arguments: args});
    assert.equal(result.isError, false, `${name}: ${JSON.stringify(result.content)}`);
    const envelope = result.structuredContent;
    assert.equal(envelope?.ok, true, `${name} structured result: ${JSON.stringify(envelope)}`);
    assert.ok(envelope.data && typeof envelope.data === 'object', `${name} result data`);
    return envelope.data;
  }
  async function close() { child.stdin.end(); const status = await exited; assert.equal(status.code, 0, `MCP server exit ${JSON.stringify(status)}: ${stderr}`); return status; }
  return {initialized, request, call, close, child};
}

// Follows nextOperation until the collection ends, as an agent would.
async function follow(mcp, tool, first, rowsOf) {
  let result = await mcp.call(tool, first); const rows = [...rowsOf(result)]; let pages = 1;
  while (typeof result.nextCursor === 'string') {
    assert.ok(pages < MAX_PAGES, `${tool} page bound`);
    const operation = result.nextOperation;
    assert.equal(operation?.tool, tool, `${tool} nextOperation names its own continuation: ${JSON.stringify(operation)}`);
    assert.equal(operation.arguments.cursor, result.nextCursor);
    result = await mcp.call(tool, operation.arguments); rows.push(...rowsOf(result)); pages++;
  }
  assert.equal(result.nextCursor ?? null, null);
  return {rows, pages, last: result};
}

const sha = bytes => createHash('sha256').update(bytes).digest('hex');

/**
 * Runs the agent path on an existing investigation.
 * baseline/corrected are two retained analyses of the same measurement, and
 * primary is one with failure rows; root receives the exported reports.
 */
export async function agentWorkflow({cli, cwd, env, record, root, investigation, primary, baseline, corrected, platform, preset}) {
  const mcp = await session(cli, cwd, env, record), summary = {platform, preset, steps: []};
  let failed = true;
  try {
    const listed = []; let cursor;
    do { const page = await mcp.request('tools/list', cursor ? {cursor} : {}); listed.push(...page.tools); cursor = page.nextCursor; } while (cursor);
    const names = new Set(listed.map(tool => tool.name));
    for (const tool of TOOLS) assert.ok(names.has(tool), `Catalog lists ${tool}`);
    summary.steps.push({step: 'catalog', tools: listed.length});

    const capabilities = await mcp.call('diagnostics_capabilities', {});
    assert.ok(Object.keys(capabilities).length, 'Capabilities result');
    summary.steps.push({step: 'capabilities', sections: Object.keys(capabilities).length});

    const plan = await mcp.call('diagnostics_plan', {preset, platform});
    assert.equal(plan.platform, platform); assert.equal(plan.preset, preset);
    assert.equal(plan.automaticExecution, false, 'A plan executes nothing');
    assert.equal(plan.readiness, 'not_probed', 'A plan probes no target');
    summary.steps.push({step: 'plan', gaps: plan.gaps?.length ?? 0, missing: plan.missing?.length ?? 0});

    const status = await mcp.call('diagnostics_status', {investigationId: investigation});
    assert.equal(status.settled, true, JSON.stringify(status));
    summary.steps.push({step: 'status', collectors: status.collectors?.length ?? 0});

    // Paging one row at a time must return the same rows as one large page.
    const whole = await mcp.call('diagnostics_query', {investigationId: investigation, analysisId: primary, kind: 'failure', limit: 100});
    assert.ok(whole.rows.length, 'Primary analysis has failure rows');
    const paged = await follow(mcp, 'diagnostics_query', {investigationId: investigation, analysisId: primary, kind: 'failure', limit: 1}, value => value.rows);
    assert.deepEqual(paged.rows, whole.rows);
    summary.steps.push({step: 'query-follow-next-operation', rows: paged.rows.length, pages: paged.pages});

    const projectedWhole = await mcp.call('diagnostics_project', {investigationId: investigation, analysisId: primary, scope: 'all', limit: 100});
    const projected = await follow(mcp, 'diagnostics_project', {investigationId: investigation, analysisId: primary, scope: 'all', limit: 1}, value => value.rows);
    assert.deepEqual(projected.rows, projectedWhole.rows);
    assert.equal(projected.rows.length, Math.min(projectedWhole.matched, 100));
    summary.steps.push({step: 'project-follow-cursor', rows: projected.rows.length, pages: projected.pages});

    const comparison = await mcp.call('diagnostics_compare', {investigationId: investigation, analysisId: baseline, otherInvestigationId: investigation, otherAnalysisId: corrected});
    assert.equal(comparison.baseline.analysisId, baseline); assert.equal(comparison.candidate.analysisId, corrected);
    assert.equal(typeof comparison.compatible, 'boolean');
    // Undeclared identities must be named, never silently treated as matching.
    if (!comparison.compatible) assert.ok(comparison.incompatible.length, JSON.stringify(comparison));
    const measured = side => side.measurements.groups.map(group => ({name: group.name, unit: group.unit, max: group.max}));
    summary.steps.push({step: 'compare', compatible: comparison.compatible, incompatible: comparison.incompatible ?? [], baseline: measured(comparison.baseline), corrected: measured(comparison.candidate)});

    const reports = [];
    for (const [format, file, report] of [
      ['investigation-json', 'agent-report.json', {}],
      ['investigation-markdown', 'agent-report.md', {privacy: 'private', analysisIds: [corrected]}],
    ]) {
      const path = join(root, file), args = {investigationId: investigation, analysisId: baseline, path, scope: 'all', format, report};
      const exported = await mcp.call('diagnostics_export', args), bytes = await readFile(path);
      assert.equal(exported.exported, true); assert.equal(exported.alreadyPresent, false);
      assert.equal(exported.remoteTransmission, false);
      assert.equal(exported.sha256, sha(bytes)); assert.equal(exported.bytes, bytes.length);
      if (format === 'investigation-json') JSON.parse(bytes); else assert.match(bytes.toString(), /^#/m);
      const repeated = await mcp.call('diagnostics_export', args);
      assert.equal(repeated.alreadyPresent, true, `${format} repeat reuses the report`);
      assert.equal(sha(await readFile(path)), sha(bytes), `${format} repeat keeps identical bytes`);
      reports.push({format, bytes: bytes.length, sha256: sha(bytes), privacy: exported.privacy});
    }
    summary.steps.push({step: 'export-reports', reports});
    failed = false;
  } finally {
    // A close failure reports only when the workflow itself passed.
    try { summary.mcpExit = await mcp.close(); } catch (error) { if (!failed) throw error; mcp.child.kill(); }
  }
  return summary;
}
