// Installed Claude and Codex consume a real Rust pack, discover the local
// rustup spelling through MCP review, and run routine tools without approval.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir, writeFile, readFile, chmod, access} from 'node:fs/promises';
import {join, dirname, delimiter} from 'node:path';
import {createServer} from 'node:http';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createHash} from 'node:crypto';
import {query} from '@anthropic-ai/claude-agent-sdk';
import {enrolledRuntime, home, binary} from '../fixtures/runtime.mjs';
import {scriptedProvider} from './claude-provider.mjs';
import {approvalHost} from './codex-host.mjs';
import {applyArguments} from './policy-proposal.mjs';
const exec = promisify(execFile);
// Portable published hooks and MCP entries resolve `shield` through PATH.
process.env.PATH = `${dirname(binary)}${delimiter}${process.env.PATH}`;
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;

async function prepare(t, agent) {
  process.env.CARGO_HOME = join(home, '.cargo');
  const f = await enrolledRuntime(t);
  await exec('git', ['init', '-q', f.root]);
  const bytes = await readFile(process.env.SHIELD_TEST_RUST_PACK, 'utf8');
  const pack = JSON.parse(bytes);
  const cargo = join(home, '.cargo/bin/cargo');
  const marker = join(f.root, 'cargo-ran');
  await mkdir(join(home, '.cargo/bin'), {recursive:true});
  for (const tool of ['cargo','rustc','rustdoc','rustfmt','rustup']) {
    const path = join(home, '.cargo/bin', tool);
    await writeFile(path, `#!/bin/sh\nprintf '%s\\n' "$*" >> ${quote(marker)}\n`);
    await chmod(path, 0o755);
  }
  const source = join(f.root, 'registry');
  await mkdir(source);
  await writeFile(join(source,'pack.json'), bytes);
  await writeFile(join(source,'catalog.json'), JSON.stringify({schema_version:1,packs:[{
    id:pack.id,version:pack.version,path:'pack.json',sha256:createHash('sha256').update(bytes).digest('hex'),
  }]}));
  await f.operator(['registry','add','fixture',source]);
  const localPath = join(f.root, '.shield/config.local.yml');
  const enrolledLocal = await readFile(localPath, 'utf8');
  await f.operator(['pack','add',`fixture/${pack.id}`,'--profile','recommended']);
  assert.ok((await readFile(localPath, 'utf8')).includes(cargo), 'Fresh setup must discover the Cargo proxy.');
  await f.operator(['mcp', 'setup', '--agent', agent]);
  // Retain registry enrollment while reproducing an existing unbound pack.
  await writeFile(localPath, enrolledLocal);
  await f.operator(['pack','configure',pack.id]);
  assert.ok((await readFile(localPath, 'utf8')).includes(cargo), 'CLI repair must use the ordinary final review.');
  await writeFile(localPath, enrolledLocal);
  const installed = await f.cli(['pack', 'show', pack.id]);
  const impostor = join(f.root, 'tools/cargo');
  await mkdir(join(f.root, 'tools'));
  await writeFile(impostor, `#!/bin/sh\nprintf unexpected > ${quote(join(f.root, 'impostor-ran'))}\n`);
  await chmod(impostor, 0o755);
  // Only this test worker uses this environment, and every path belongs to it.
  process.env.CARGO_HOME = join(home, '.cargo');
  const routine = [
    'fmt', 'fmt --check', 'clippy --locked --workspace --all-targets',
    "nextest run --locked --lib -E 'test(acme)'",
  ].map(args => `${quote(cargo)} ${args}`).join(' && ');
  const blocked = `${quote(cargo)} build --target-dir ${quote(join(home, 'outside-output'))}`;
  return {...f, cargo, marker, impostor, routine, blocked, packId:pack.id, reference:installed.pack.selector};
}

async function verify(f, agent) {
  assert.deepEqual((await readFile(f.marker, 'utf8')).trim().split('\n'), [
    'fmt', 'fmt --check', 'clippy --locked --workspace --all-targets',
    'nextest run --locked --lib -E test(acme)',
  ]);
  await assert.rejects(access(join(f.root, 'impostor-ran')));
  const shared = await readFile(join(f.root, '.shield/config.yml'), 'utf8');
  assert.ok(!shared.includes(f.cargo), 'Machine paths must remain outside the shared policy.');
  const local = await readFile(join(f.root, '.shield/config.local.yml'), 'utf8');
  assert.ok(local.includes(f.cargo), 'Discovery must save the exact proxy locally.');
  const events = (await f.events()).filter(event => event.kind === 'decision' && event.provenance.agent === agent);
  const routine = events.find(event => event.details.original_input.command === f.routine);
  assert.equal(routine?.details.decision, 'allow', JSON.stringify(events));
  assert.equal(routine?.details.ask_fallback, null);
  assert.ok(events.some(event => event.details.original_input.command === f.blocked && event.details.host_decision === 'deny'));
  assert.ok(events.some(event => event.details.original_input.command?.includes(f.impostor) && event.details.decision === 'ask'));
}

for (const agent of ['claude', 'codex']) {
  test(`installed ${agent} discovers Rust proxies and runs routine Cargo without approval`,
    {skip: !process.env.SHIELD_TEST_RUST_PACK || process.platform === 'win32', timeout:120000}, async t => {
      const f = await prepare(t, agent);
      if (agent === 'claude') await claude(t, f);
      else await codex(t, f);
      await verify(f, agent);
    });
}

async function claude(t, f) {
  const provider = await scriptedProvider(t, [
    {name:'mcp__shield__pack_configure', input:{reference:f.reference}},
    {name:'mcp__shield__policy_apply', input:body => applyArguments(body.messages.flatMap(message => Array.isArray(message.content) ? message.content.filter(block => block.type === 'tool_result') : [])), before:async () => {
      const local = await readFile(join(f.root, '.shield/config.local.yml'), 'utf8');
      assert.ok(!local.includes(f.cargo), 'Discovery must remain inactive before approval.');
    }},
    {name:'Bash', input:{command:f.routine}},
    {name:'Bash', input:{command:f.blocked}},
    {name:'Bash', input:{command:quote(f.impostor)}},
  ]);
  const calls = [];
  const reviews = [];
  const executable = (await exec('which', ['claude'])).stdout.trim();
  const conversation = query({prompt:'Perform the Acme fixture tools in order.', options:{
    pathToClaudeCodeExecutable:executable, cwd:f.root, settingSources:['project','local'],
    tools:['Bash'],
    permissionMode:'default', persistSession:false, maxTurns:12,
    canUseTool:async (name, input) => { calls.push({name,input}); return {behavior:'deny',message:'Unexpected approval.'}; },
    onElicitation:async request => { reviews.push(request); return {action:'accept',content:{}}; },
    env:{...process.env, ANTHROPIC_API_KEY:'acme-local-test-key', ANTHROPIC_BASE_URL:provider.url,
      CLAUDE_CONFIG_DIR:join(f.root,'claude-config'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC:'1'},
  }});
  t.after(() => conversation.close());
  const messages = [];
  for await (const message of conversation) messages.push(message);
  assert.ok(messages.some(message => message.type === 'result' && message.subtype === 'success'), JSON.stringify(messages));
  assert.equal(reviews.length, 1, 'Only the policy activation review should be shown.');
  assert.ok(JSON.stringify(reviews).includes(f.cargo), 'The review must identify the executable being allowed.');
  assert.deepEqual(calls, [{name:'Bash',input:{command:quote(f.impostor)}}]);
}

async function codex(t, f) {
  const steps = [
    ['pack_configure', () => ({reference:f.reference})],
    ['policy_apply', body => applyArguments(body.input?.filter(item => item.type === 'function_call_output'))],
    ['shell', () => f.routine], ['shell', () => f.blocked], ['shell', () => quote(f.impostor)],
  ];
  let count = 0;
  const server = createServer(async (request, response) => {
    let bytes = '';
    for await (const chunk of request) bytes += chunk;
    const body = JSON.parse(bytes || '{}');
    const step = steps[count++];
    let item;
    if (step) {
      const namespace = body.tools?.find(tool => tool.type === 'namespace' && tool.name === 'mcp__shield');
      let name = step[0];
      let args;
      if (name === 'shell') {
        name = body.tools?.find(tool => ['exec_command','shell_command','shell'].includes(tool.name))?.name ?? 'exec_command';
        const command = step[1](body);
        args = name === 'exec_command' ? {cmd:command,workdir:f.root}
          : name === 'shell' ? {command:['/bin/sh','-c',command],workdir:f.root} : {command,workdir:f.root};
      } else {
        args = step[1](body);
        if (!namespace) name = `mcp__shield__${name}`;
      }
      item = {type:'function_call', id:`fc_acme_${count}`,call_id:`call_acme_${count}`,name,
        ...(step[0] !== 'shell' && namespace ? {namespace:namespace.name} : {}),arguments:JSON.stringify(args)};
    } else item = {type:'message',id:'msg_acme',role:'assistant',status:'completed',content:[{type:'output_text',text:'Acme fixture finished.',annotations:[]}]};
    const result = {id:`resp_acme_${count}`,object:'response',created_at:0,model:body.model,status:'completed',output:[item],usage:{input_tokens:10,output_tokens:10,total_tokens:20}};
    response.writeHead(200, {'content-type':'text/event-stream'});
    for (const event of [
      {type:'response.created',response:{...result,status:'in_progress',output:[]}},
      {type:'response.output_item.added',output_index:0,item},
      {type:'response.output_item.done',output_index:0,item},
      {type:'response.completed',response:result},
    ]) response.write(`data: ${JSON.stringify(event)}\n\n`);
    response.end();
  });
  await new Promise((resolve, reject) => { server.once('error',reject); server.listen(0,'127.0.0.1',resolve); });
  t.after(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  const codexHome = join(f.root,'codex-home');
  await mkdir(codexHome);
  await writeFile(join(codexHome,'config.toml'), `model = "acme-model"\nmodel_provider = "acme"\n[model_providers.acme]\nname = "Acme local fixture"\nbase_url = "http://127.0.0.1:${server.address().port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[projects.${JSON.stringify(f.root)}]\ntrust_level = "trusted"\n`);
  const result = await approvalHost(f.root,codexHome,'decline','accept',{waitForShield:true});
  assert.equal(result.mcpApprovals.length, 1, result.stdout + result.stderr);
  assert.ok(JSON.stringify(result.mcpApprovals).includes(f.cargo));
  assert.deepEqual(result.approvals, [], 'Routine Cargo must not ask for host permission.');
  assert.doesNotMatch(result.stdout + result.stderr, /Hook failed|unsupported permissionDecision/i);
}
