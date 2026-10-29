import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, writeFile, readFile, access, symlink, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:http';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { approvalHost } from './codex-host.mjs';
const exec = promisify(execFile);
const quote = s => `'${s.replaceAll("'", "'\\''")}'`;

async function fixture(t, mode) {
  let server;
  let removeProject;
  t.after(async () => {
    try {
      if (server?.listening) await new Promise((resolve, reject) => {
        server.close(error => error ? reject(error) : resolve());
        server.closeAllConnections();
      });
    } finally {
      await removeProject?.();
    }
  });
  const f = await enrolledRuntime({ after: cleanup => { removeProject = cleanup; } });
  await exec('git', ['init', '-q', f.root]);
  const anchoredGit = mode === 'permission-git';
  const bareGit = mode === 'permission-git-bare';
  const nodeCheck = mode === 'permission-node-check';
  const executionCwd = mode === 'permission-workdir' || anchoredGit ? join(f.root, 'selected-workdir') : f.root;
  if (executionCwd !== f.root) await mkdir(executionCwd);
  const temporary = mode === 'temporary-files' ? await mkdtemp(join(tmpdir(), 'shield-codex-acme-')) : null;
  if (temporary) t.after(() => rm(temporary, { recursive: true, force: true }));
  const marker = join(temporary ?? (anchoredGit ? f.root : executionCwd), 'codex-side-effect');
  const command = temporary ? `printf ACME_TEMPORARY_TEXT > ${quote(marker)} && cat ${quote(marker)}`
    : mode === 'protected-relative' ? 'cd .codex && printf tampered > hooks.json'
    : mode === 'protected-symlink' ? 'printf tampered > ordinary.txt'
      : mode === 'safe-move' ? 'mv incoming/codex-side-effect .'
        : nodeCheck ? 'node --check check-source.mjs'
          : bareGit ? 'git init codex-side-effect && git add acme.txt && git diff --cached --name-only'
          : anchoredGit ? `git -C ${quote(f.root)} init codex-side-effect && git -C ${quote(f.root)} status --short`
          : mode === 'permission-workdir' ? 'touch codex-side-effect' : `touch ${quote(marker)}`;
  if (nodeCheck) {
    const source = join(f.root, 'check-source.mjs');
    await writeFile(source, `import { writeFileSync } from 'node:fs';\nwriteFileSync(${JSON.stringify(marker)}, 'executed');\nthrow new Error('Must not run');\n`);
    const path = join(f.root, '.shield/rules.yml');
    const rules = await f.readRules();
    rules.rules.push({ id: 'syntax-source', decision: 'allow', conditions: { domain: 'file', action: 'read', sources: { all: { exact: { root: 'absolute', path: source } } } } });
    await writeFile(path, JSON.stringify(rules));
    await f.operator(['policy', 'review'], 'manual\nyes\n');
  }
  if (temporary) {
    const path = join(f.root, '.shield/rules.yml');
    const rules = await f.readRules();
    rules.capabilities.push('requirements-v1', 'access-paths-v1');
    rules.rules.push(
      { id: 'printf', decision: 'allow', conditions: { domain: 'argv', executable: 'printf', arguments: { kind: 'any' } } },
      { id: 'temporary-write', decision: 'allow', conditions: { domain: 'file', action: 'write' }, requirements: { destination: { within: { root: 'write' } } }, on_unmet: { decision: 'deny' } },
      { id: 'temporary-cat', decision: 'allow', conditions: { domain: 'argv', executable: 'cat', arguments: { kind: 'any' } }, requirements: { targets: { action: 'read', all: { within: { root: 'read' } } } }, on_unmet: { decision: 'deny' } },
    );
    await writeFile(path, JSON.stringify(rules));
    await f.operator(['policy', 'review'], 'manual\nyes\n');
  }
  if (anchoredGit || bareGit) {
    if (bareGit) await writeFile(join(f.root, 'acme.txt'), 'Acme initial file');
    const path = join(f.root, '.shield/rules.yml');
    const rules = await f.readRules();
    if (anchoredGit) {
      rules.capabilities.push('directory-options-v1');
      rules.directory_options = [{ executable: 'git', option: '-C', repeat: 'chain', paths: 'follow' }];
    }
    for (const subcommand of ['init', 'status', 'add', 'diff']) rules.rules.push({ id: `git-${subcommand}`, decision: 'allow', reason: 'Acme Git command policy',
      conditions: { domain: 'argv', executable: 'git', arguments: { kind: 'prefix', values: [subcommand] } } });
    await writeFile(path, JSON.stringify(rules));
    await f.operator(['policy', 'review'], 'manual\nyes\n');
  }
  if (mode === 'exact-deny') {
    const rule = (id, decision, conditions) => ({ id, decision, conditions, reason: 'Acme exact request fixture' });
    await writeFile(join(f.root, '.shield/rules.yml'), JSON.stringify({ schema_version: 1, kind: 'custom', capabilities: ['structured-rules-v1', 'argument-rules-v1'], rules: [
      rule('literal', 'allow', { domain: 'argv', executable: 'touch', arguments: { kind: 'any' } }),
      rule('exact-deny', 'deny', { domain: 'exact_request', input: { kind: 'shell', source: command, dialect: 'posix', dialect_version: null,
        original_input: { command } }, context: { cwd: f.root, environment: null }, executable: null }),
    ] }));
    await f.operator(['policy', 'review'], 'manual\nyes\n');
  }
  if (['allow', 'policy-deny', 'permission-allow', 'permission-deny', 'permission-workdir', 'permission-ask', 'permission-approve'].includes(mode)) {
    const path = join(f.root, '.shield/rules.yml');
    const rules = await f.readRules();
    rules.rules.push({ id: 'marker', decision: mode === 'policy-deny' ? 'deny' : ['permission-ask', 'permission-approve'].includes(mode) ? 'ask' : 'allow', reason: 'Acme fixture marker',
      conditions: { domain: 'argv', executable: 'touch', arguments: { kind: 'any' } } });
    if (mode === 'permission-deny') rules.rules.push({ id: 'permission-denial', decision: 'deny', reason: 'Acme permission request refusal',
      conditions: { domain: 'exact_request', input: { kind: 'shell', source: command, dialect: 'posix', dialect_version: null,
        original_input: { command, description: 'Acme permission-request fixture' } }, context: { cwd: f.root, environment: null }, executable: null } });
    await writeFile(path, JSON.stringify(rules));
    await f.operator(['policy', 'review'], 'manual\nyes\n');
  }
  if (mode === 'required-option') {
    const path = join(f.root, '.shield/rules.yml');
    const rules = await f.readRules();
    rules.capabilities.push('requirements-v1');
    rules.rules.push({ id: 'touch-existing', decision: 'allow',
      conditions: { domain: 'argv', executable: 'touch' },
      requirements: { arguments: { kind: 'options', prefix: [], flags: ['-c'], values: [], operands: true, require_all: ['-c'] } },
      on_unmet: { decision: 'deny', message: 'Add -c before touching a file and submit again.', alternatives: ['touch -c existing.txt'] } });
    await writeFile(path, JSON.stringify(rules));
    await f.operator(['policy', 'review'], 'manual\nyes\n');
  }
  await f.operator(['mcp', 'setup', '--agent', 'codex']);
  if (mode === 'deny') {
    // Seed an existing operator Block choice in this isolated fixture, then
    // exercise setup's confirmed preservation of it. Fresh setups choose Allow.
    const state = JSON.parse(await readFile(f.statePath, 'utf8'));
    state.operator.integration.ask_fallback.codex = 'block';
    await writeFile(f.statePath, JSON.stringify(state));
    const review = await f.operator(['mcp', 'setup', '--agent', 'codex']);
    assert.match(review.output, /Ask requests are blocked/);
  }
  const installedHooks = await readFile(join(f.root, '.codex/hooks.json'), 'utf8');
  if (mode === 'permission-deny') {
    const hooks = JSON.parse(installedHooks);
    const allow = JSON.stringify({ hookSpecificOutput: { hookEventName: 'PermissionRequest', decision: { behavior: 'allow' } } });
    hooks.hooks.PermissionRequest.push({ matcher: 'Bash', hooks: [{ type: 'command', command: `printf '%s\\n' '${allow}'` }] });
    await writeFile(join(f.root, '.codex/hooks.json'), JSON.stringify(hooks));
  }
  if (mode === 'protected-symlink') await symlink(join(f.root, '.codex/hooks.json'), join(f.root, 'ordinary.txt'));
  if (mode === 'safe-move') {
    await mkdir(join(f.root, 'incoming'));
    await writeFile(join(f.root, 'incoming/codex-side-effect'), 'Acme move fixture');
  }
  if (['timeout', 'ask', 'rewrite-approval'].includes(mode)) {
    const path = join(f.root, '.codex/hooks.json');
    const hooks = JSON.parse(await readFile(path, 'utf8'));
    hooks.hooks.PreToolUse[0].hooks[0] = mode === 'timeout'
      ? { type: 'command', command: 'sleep 3', timeout: 1 }
      : mode === 'ask'
        ? { type: 'command', command: `printf '%s\\n' '${JSON.stringify({ hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'ask' } })}'`, timeout: 10 }
        : { type: 'command', command: `node ${quote(join(f.root, 'rewrite-hook.mjs'))}`, timeout: 10 };
    if (mode === 'rewrite-approval') await writeFile(join(f.root, 'rewrite-hook.mjs'), `let bytes = ''; for await (const chunk of process.stdin) bytes += chunk;
const input = JSON.parse(bytes).tool_input;
console.log(JSON.stringify({ hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'allow', updatedInput: { ...input, sandbox_permissions: 'require_escalated', justification: 'Acme approval probe' } } }));\n`);
    await writeFile(path, JSON.stringify(hooks));
  }
  const envelopeCapture = join(f.root, 'permission-envelope.jsonl');
  if (mode.startsWith('permission-')) {
    // Passive observer: record shape and cwd relationships, never command text
    // or a permission decision. Shield's generated hook remains installed.
    const observer = join(f.root, 'capture-permission.mjs');
    await writeFile(observer, `import { appendFile } from 'node:fs/promises';
let bytes = ''; for await (const chunk of process.stdin) bytes += chunk;
const host = JSON.parse(bytes);
const shape = { toolName: host.tool_name, toolInputKeys: Object.keys(host.tool_input).sort(),
  hasToolUseId: Object.hasOwn(host, 'tool_use_id'),
  cwdIsSession: host.cwd === ${JSON.stringify(f.root)}, cwdIsSelected: host.cwd === ${JSON.stringify(executionCwd)} };
await appendFile(${JSON.stringify(envelopeCapture)}, JSON.stringify(shape) + '\\n');
console.log('{}');\n`);
    const path = join(f.root, '.codex/hooks.json');
    const hooks = JSON.parse(await readFile(path, 'utf8'));
    hooks.hooks.PermissionRequest.push({ matcher: 'Bash', hooks: [{ type: 'command', command: `node ${quote(observer)}`, timeout: 10 }] });
    await writeFile(path, JSON.stringify(hooks));
  }
  const home = join(f.root, 'codex-test-home');
  await mkdir(home);
  let count = 0;
  const requests = [];
  server = createServer(async (req, res) => {
    let bytes = '';
    for await (const chunk of req) bytes += chunk;
    const body = JSON.parse(bytes || '{}');
    requests.push(body);
    const shell = body.tools?.find(tool => ['exec_command', 'shell_command', 'shell'].includes(tool.name));
    const name = shell?.name ?? 'exec_command';
    const arguments_ = name === 'exec_command' ? { cmd: command, workdir: executionCwd }
      : name === 'shell' ? { command: ['/bin/sh', '-c', command], workdir: executionCwd }
        : { command, workdir: executionCwd };
    if (mode.startsWith('permission-')) {
      arguments_.sandbox_permissions = 'require_escalated';
      arguments_.justification = 'Acme permission-request fixture';
    }
    const item = count++ === 0
      ? { type: 'function_call', id: 'fc_acme', call_id: 'call_acme', name, arguments: JSON.stringify(arguments_) }
      : { type: 'message', id: 'msg_acme', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: 'Acme fixture finished.', annotations: [] }] };
    const response = { id: `resp_acme_${count}`, object: 'response', created_at: 0, model: body.model, status: 'completed', output: [item], usage: { input_tokens: 10, output_tokens: 10, total_tokens: 20 } };
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    for (const event of [
      { type: 'response.created', response: { ...response, status: 'in_progress', output: [] } },
      { type: 'response.output_item.added', output_index: 0, item },
      { type: 'response.output_item.done', output_index: 0, item },
      { type: 'response.completed', response },
    ]) res.write(`data: ${JSON.stringify(event)}\n\n`);
    res.end();
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  await writeFile(join(home, 'config.toml'), `model = "acme-model"\nmodel_provider = "acme"\n[model_providers.acme]\nname = "Acme local fixture"\nbase_url = "http://127.0.0.1:${server.address().port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[projects.${JSON.stringify(f.root)}]\ntrust_level = "trusted"\n`);
  // The isolated host needs shell hooks, not background plugin marketplace clones.
  const running = mode.startsWith('permission-') ? null : exec('codex', ['exec', '--ephemeral', '--disable', 'plugins', '--disable', 'remote_plugin', '--dangerously-bypass-hook-trust', '--json', '-C', f.root, 'Perform the Acme fixture command.'], {
    env: { ...process.env, CODEX_HOME: home }, timeout: 45000, maxBuffer: 2_000_000,
  });
  running?.child.stdin.end();
  const result = running ? await running : await approvalHost(f.root, home, mode === 'permission-approve' ? 'accept' : 'decline');
  const toolOutputs = requests.flatMap(r => r.input ?? []).filter(item => item.type === 'function_call_output').map(item => item.output).join('\n');
  const nextRequest = requests.find(request => (request.input ?? []).some(item => item.type === 'function_call_output'));
  if (!['timeout', 'ask', 'rewrite-approval'].includes(mode)) {
    assert.doesNotMatch(JSON.stringify(requests) + result.stdout + result.stderr,
      /unsupported permissionDecision|Hook failed/i, 'Owned hooks must succeed, not rely on host continuation after failure');
  }
  if (['protected-relative', 'protected-symlink'].includes(mode)) {
    assert.equal(await readFile(join(f.root, '.codex/hooks.json'), 'utf8'), installedHooks);
    assert.match(toolOutputs, /Command blocked by PreToolUse hook:/);
    assert.ok(nextRequest, 'The model must receive guidance after the protected-path denial.');
    const delivered = JSON.stringify(nextRequest.input);
    assert.match(delivered, /Shield: protected path\./);
    if (mode === 'protected-symlink') {
      assert.match(toolOutputs, /Access to `ordinary\.txt` is restricted because it protects the agent's security settings/);
      assert.match(delivered, /treated as an edit to `ordinary\.txt`, which can disable agent protection/);
    } else {
      assert.match(toolOutputs, /This request reaches protected files or directories/);
      assert.match(delivered, /Never read or change \.shield\/review/);
    }
    assert.match(delivered, /shield protection set/);
    assert.doesNotMatch(toolOutputs, /Shield: protected path\.|treated as an edit|Never read or change|shield protection set/);
  } else if (['deny', 'policy-deny', 'exact-deny', 'required-option', 'permission-ask', 'permission-deny'].includes(mode)) {
    await assert.rejects(access(marker));
    const deniedReason = mode === 'permission-deny' ? /Shield: blocked\. The active policy does not allow this request/ : mode.startsWith('permission-') ? /approval|approv|reject/i : mode === 'deny' ? /Codex cannot request approval here/
      : mode === 'required-option' ? /Shield: blocked\. Missing required option: -c/
        : /Shield: blocked\. The active policy does not allow this request/;
    assert.match(toolOutputs, deniedReason, `Acme Codex tool outputs: ${toolOutputs.slice(-2500)}`);
    const events = await f.events();
    assert.ok(events.some(e => e.kind === 'decision' && e.provenance.agent === 'codex' && e.details.operation.kind === 'shell'
      && e.details.decision === (['deny', 'permission-ask'].includes(mode) ? 'ask' : 'deny')));
    if (['policy-deny', 'exact-deny'].includes(mode)) {
      assert.ok(nextRequest, 'The model must receive policy guidance after the native denial.');
      const delivered = JSON.stringify(nextRequest.input);
      assert.match(delivered, /Shield: denied/);
      assert.match(delivered, /policy_propose/);
      assert.doesNotMatch(toolOutputs, /policy_propose|source_entry_id|apply_arguments/);
    }
    if (mode === 'deny') {
      assert.ok(nextRequest, 'The model must receive a next request after the native denial.');
      const delivered = JSON.stringify(nextRequest.input);
      assert.match(delivered, /Codex cannot prompt here/);
      assert.match(delivered, /shield_docs_search/);
      assert.match(delivered, /shield_docs_read/);
      assert.match(delivered, /policy_propose/);
      assert.match(delivered, /source_entry_id/);
      assert.match(delivered, /policy_apply/);
      assert.doesNotMatch(delivered, /policy_propose with change|rule_add/);
      assert.doesNotMatch(toolOutputs, /policy_propose|source_entry_id|apply_arguments/);
    }
    if (mode === 'required-option') {
      assert.ok(nextRequest, 'The model must receive a next request after the requirement denial.');
      const delivered = JSON.stringify(nextRequest.input);
      assert.match(delivered, /Add -c before touching a file and submit again/);
      assert.match(delivered, /Candidate: touch -c existing.txt/);
      assert.match(delivered, /Re-evaluate any revised request/);
      assert.doesNotMatch(toolOutputs, /Candidate:|Re-evaluate any revised request|Add -c before touching/);
      await assert.rejects(access(join(f.root, 'existing.txt')));
    }
  } else {
    if (nodeCheck) {
      await assert.rejects(access(marker));
      assert.match(toolOutputs, /Process exited with code 0/, 'The valid source must complete its syntax check successfully');
    }
    else await access(marker);
    if (temporary) {
      assert.equal(await readFile(marker, 'utf8'), 'ACME_TEMPORARY_TEXT');
      assert.match(toolOutputs, /ACME_TEMPORARY_TEXT/);
      const decisions = (await f.events()).filter(event => event.kind === 'decision');
      assert.ok(decisions.length > 0);
      assert.ok(decisions.every(event => event.details.decision === 'allow'), JSON.stringify(decisions));
    }
    if (mode === 'safe-move') {
      assert.equal(await readFile(marker, 'utf8'), 'Acme move fixture');
      await assert.rejects(access(join(f.root, 'incoming/codex-side-effect')));
    }
    if (mode === 'allow') {
      assert.ok(JSON.stringify(requests).includes('Shield active project policy permits this request'),
        'Codex must accept and deliver the permitted-request context to the model');
      const events = await f.events();
      assert.ok(events.some(e => e.kind === 'decision' && e.provenance.agent === 'codex' && e.details.decision === 'allow'));
    }
    if (mode === 'fallback-allow') {
      assert.ok(JSON.stringify(requests).includes('Shield: policy asks for approval; your configured Ask fallback allows this request'),
        'Codex must accept and deliver the fallback context to the model');
      const events = await f.events();
      assert.ok(events.some(e => e.kind === 'decision' && e.provenance.agent === 'codex'
        && e.details.decision === 'ask' && e.details.disposition === 'allowed' && e.details.ask_fallback === 'allow'
        && e.details.approval.status === 'none'));
    }
  }
  if (mode.startsWith('permission-')) {
    const envelopes = (await readFile(envelopeCapture, 'utf8')).trim().split('\n').map(line => JSON.parse(line));
    assert.equal(envelopes.length, 1);
    assert.deepEqual(envelopes[0], {
      toolName: 'Bash', toolInputKeys: ['command', 'description'], hasToolUseId: false,
      cwdIsSession: true, cwdIsSelected: mode !== 'permission-workdir' && !anchoredGit,
    }, 'Observed Codex shell approval shape; retain missing context without changing policy Allow');
    t.diagnostic(`${mode} envelope: ${JSON.stringify(envelopes[0])}`);
    const events = await f.events();
    const permission = events.filter(e => e.provenance.host_event_name === 'PermissionRequest');
    assert.ok(permission.length > 0, `Codex must discover the generated PermissionRequest hook: ${JSON.stringify({events:events.map(e=>({event:e.provenance.host_event_name,code:e.details.code})),stderr:result.stderr.slice(-1000)})}`);
    assert.ok(permission.every(e => e.details.ask_fallback === null));
    const policyAllowed = ['permission-allow', 'permission-workdir', 'permission-git', 'permission-git-bare', 'permission-node-check'].includes(mode);
    assert.ok(permission.some(e => e.details.disposition === (mode === 'permission-deny' ? 'blocked' : policyAllowed ? 'allowed' : 'approval_required')));
    assert.ok(permission.every(e => e.details.host_context_complete === false), 'Missing workdir remains an evidence limit, even when policy allows execution');
    if (policyAllowed) assert.ok(permission.every(e => e.details.host_decision === 'allow' && e.details.code === 'policy-allow'));
    if (anchoredGit || bareGit) {
      await access(join(marker, '.git'));
      if (anchoredGit) await assert.rejects(access(join(executionCwd, 'codex-side-effect')));
      if (bareGit) assert.equal((await exec('git', ['-C', f.root, 'diff', '--cached', '--name-only'])).stdout.trim(), 'acme.txt');
    }
    if (mode === 'permission-workdir') {
      assert.ok(permission.every(e => e.provenance.cwd === f.root), 'Host reports session cwd rather than execution cwd');
      await assert.rejects(access(join(f.root, 'codex-side-effect')));
    }
    assert.ok(permission.every(e => e.details.approval.status === 'none'));
    assert.equal(result.approvals.length, mode === 'permission-deny' || policyAllowed ? 0 : 1,
      'Ask must reach host approval; Deny must stop before approval even with a competing allowing hook');
  }
  assert.ok(requests.length >= 2);
}
test('real Codex loads owned project hooks and rejects a shell call before dispatch', { timeout: 60000 }, t => fixture(t, 'deny'));
test('real Codex executes a policy-allowed command through owned hooks', { timeout: 60000 }, t => fixture(t, 'allow'));
test('real Codex reads and writes arbitrary temporary files outside its checkout', { timeout: 60000 }, t => fixture(t, 'temporary-files'));
test('real Codex PermissionRequest approves policy Allow despite missing workdir', { timeout: 60000 }, t => fixture(t, 'permission-allow'));
test('real Codex autoapproves explicit Git cwd despite a different hidden execution workdir', { timeout: 60000 }, t => fixture(t, 'permission-git'));
test('real Codex autoapproves bare git init and git add using reported cwd without native rules', { timeout: 60000 }, t => fixture(t, 'permission-git-bare'));
test('real Codex approves node syntax checks through file-read policy without executing the source', { timeout: 60000 }, t => fixture(t, 'permission-node-check'));
test('real Codex PermissionRequest Ask reaches host approval and a declined request cannot execute', { timeout: 60000 }, t => fixture(t, 'permission-ask'));
test('real Codex PermissionRequest Ask executes only after the host client approves it', { timeout: 60000 }, t => fixture(t, 'permission-approve'));
test('real Codex PermissionRequest Deny defeats a competing hook Allow before approval', { timeout: 60000 }, t => fixture(t, 'permission-deny'));
test('real Codex PermissionRequest follows policy with a hidden execution workdir and retains reported cwd', { timeout: 60000 }, t => fixture(t, 'permission-workdir'));
test('real Codex executes Ask through confirmed fallback and preserves policy Ask history', { timeout: 60000 }, t => fixture(t, 'fallback-allow'));
test('real Codex cannot overwrite hook configuration through a relative redirected path', { timeout: 60000 }, t => fixture(t, 'protected-relative'));
test('real Codex cannot overwrite hook configuration through a symlink alias', { timeout: 60000 }, t => fixture(t, 'protected-symlink'));
test('real Codex honors exact request Deny over an allowed literal command and Ask fallback', { timeout: 60000 }, t => fixture(t, 'exact-deny'));
test('real Codex moves an ordinary file into the project root through Ask fallback', { timeout: 60000 }, t => fixture(t, 'safe-move'));
test('real Codex blocks explicit policy Deny', { timeout: 60000 }, t => fixture(t, 'policy-deny'));
test('real Codex receives requirement guidance and never executes its static alternative', { timeout: 60000 }, t => fixture(t, 'required-option'));
test('Codex host hook timeout can continue a disposable command', { timeout: 60000 }, t => fixture(t, 'timeout'));
test('Codex unsupported ask output can continue a disposable command', { timeout: 60000 }, t => fixture(t, 'ask'));
test('Codex shell input rewrite cannot request approval via sandbox permissions', { timeout: 60000 }, t => fixture(t, 'rewrite-approval'));
