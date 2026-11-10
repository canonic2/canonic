import test from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { geminiProvider } from './gemini-provider.mjs';
import { preparePolicyChange } from './policy-proposal.mjs';

const exec = promisify(execFile);
async function geminiEnvironment(root, provider) {
  const home=join(root,'gemini-home');
  await mkdir(join(home,'.gemini'),{recursive:true});
  await writeFile(join(home,'.gemini/settings.json'),JSON.stringify({telemetry:{enabled:false},privacy:{usageStatisticsEnabled:false},security:{auth:{selectedType:'gemini-api-key'}}}));
  return {...process.env,GEMINI_CLI_HOME:home,GOOGLE_GEMINI_BASE_URL:provider.url,
    GEMINI_API_KEY:'acme-local-test-key',GEMINI_CLI_TRUST_WORKSPACE:'true',GOOGLE_GENAI_USE_GCA:'false',GOOGLE_GENAI_USE_VERTEXAI:'false',
    GEMINI_TELEMETRY_ENABLED:'false',HTTP_PROXY:'',HTTPS_PROXY:'',ALL_PROXY:'',NO_PROXY:'127.0.0.1,localhost'};
}

test('Gemini policy form cannot activate a proposal in headless yolo mode',{timeout:60000},async t=>{
  const f=await enrolledRuntime(t);
  await f.operator(['mcp','setup','--agent','gemini']);
  const args=await preparePolicyChange(f.root,'gemini');
  const before=JSON.parse(await readFile(f.statePath,'utf8'));
  const provider=await geminiProvider(t,[{name:body=>body.tools?.flatMap(tool=>tool.functionDeclarations??[]).find(tool=>tool.name==='policy_apply'||tool.name?.endsWith('_policy_apply'))?.name,input:args}]);
  const output=await exec('gemini',['--skip-trust','--approval-mode','yolo','--output-format','json','--model','gemini-2.5-flash','--prompt','Perform the Acme fixture tool exactly once.'],
    {cwd:f.root,timeout:25000,maxBuffer:1024*1024,env:await geminiEnvironment(f.root,provider)});
  assert.ok(provider.results.length,`No MCP result: ${output.stderr}`);
  assert.match(JSON.stringify(provider.results),/approval_|not approved|confirmation.*unavailable/i);
  const after=JSON.parse(await readFile(f.statePath,'utf8'));
  assert.equal(after.policy_revision,before.policy_revision);
  assert.equal(after.proposals.at(-1).status,'pending');
});
test('Gemini native hooks enforce Allow, Ask and Deny with a local scripted provider', { timeout: 90000 }, async t => {
  const f = await enrolledRuntime(t);
  await f.operator(['mcp', 'setup', '--agent', 'gemini']);
  t.diagnostic(`Installed events: ${Object.keys(JSON.parse(await readFile(join(f.root,'.gemini/settings.json'),'utf8')).hooks).join(', ')}`);
  const home = join(f.root, 'gemini-home');
  await mkdir(join(home, '.gemini'), { recursive: true });
  await writeFile(join(home, '.gemini/settings.json'), JSON.stringify({
    telemetry: { enabled: false }, privacy: { usageStatisticsEnabled: false },
    security: { auth: { selectedType: 'gemini-api-key' } },
  }));
  for (const [path, expected] of [[f.allowed, 'ACME_ALLOWED_TEXT'], [f.denied, 'Shield:'], [f.asked, 'approval']]) {
    const provider = await geminiProvider(t, [{ name: 'read_file', input: { file_path: path } }]);
    const pending = exec('gemini', ['--skip-trust', '--approval-mode', 'yolo', '--output-format', 'json',
      '--model', 'gemini-2.5-flash', '--prompt', 'Perform the Acme fixture tool exactly once.'], {
      cwd: f.root, timeout: 25000, maxBuffer: 1024 * 1024,
      env: { ...process.env, GEMINI_CLI_HOME: home, GOOGLE_GEMINI_BASE_URL: provider.url,
        GEMINI_API_KEY: 'acme-local-test-key', GEMINI_CLI_TRUST_WORKSPACE: 'true', GOOGLE_GENAI_USE_GCA: 'false', GOOGLE_GENAI_USE_VERTEXAI: 'false',
        GEMINI_TELEMETRY_ENABLED: 'false', HTTP_PROXY: '', HTTPS_PROXY: '', ALL_PROXY: '', NO_PROXY: '127.0.0.1,localhost' },
    });
    let awaiting = false;
    let messages = '';
    if (path === f.asked) pending.child.stderr.on('data', chunk => {
      messages += chunk;
      if (messages.includes('Hook system message: Shield: approval required.')) {
        awaiting = true;
        pending.child.kill('SIGTERM');
      }
    });
    let output;
    try { output = await pending; }
    catch (error) {
      if (!awaiting) throw error;
      output = { stdout: error.stdout, stderr: error.stderr };
    }
    if (path === f.asked) {
      assert.equal(awaiting, true, `Gemini did not wait for approval: ${output.stderr}`);
      assert.ok(!JSON.stringify(provider.results).includes('ACME_ASKED_TEXT'));
      assert.ok(!output.stdout.includes('ACME_ASKED_TEXT'));
      continue;
    }
    assert.ok(provider.results.length, `No tool result: ${output.stdout}\n${output.stderr}`);
    const results = JSON.stringify(provider.results);
    assert.ok(results.toLowerCase().includes(expected.toLowerCase()), `${results}\n${output.stdout}\n${output.stderr}\n${JSON.stringify(await f.events())}`);
    if (path !== f.allowed) assert.ok(!results.includes(path === f.denied ? 'ACME_DENIED_TEXT' : 'ACME_ASKED_TEXT'), results);
  }
});

test('Antigravity native hooks enforce policy independently of skip-permissions',
  { timeout: 90000, skip: process.env.SHIELD_LIVE_AGY !== '1' }, async t => {
    const f = await enrolledRuntime(t);
    await f.operator(['mcp', 'setup', '--agent', 'agy']);
    for (const [path, mode] of [[f.allowed, 'allow'], [f.denied, 'deny'], [f.asked, 'ask']]) {
      const args = ['--print', `Read ${JSON.stringify(path)} exactly once with read_file. Return the exact contents or the hook error; stop without retries or other tools.`,
        '--print-timeout', '20s', '--output-format', 'json', '--effort', 'low'];
      if (mode === 'deny') args.push('--dangerously-skip-permissions');
      const result = await exec('agy', args, { cwd: f.root, timeout: 25000, maxBuffer: 1024 * 1024 });
      const output = JSON.parse(result.stdout);
      if (mode === 'allow') assert.ok(output.response.includes('ACME_ALLOWED_TEXT'), result.stdout);
      else assert.ok(!result.stdout.includes(mode === 'deny' ? 'ACME_DENIED_TEXT' : 'ACME_ASKED_TEXT'), result.stdout);
      const decisions = (await f.events()).filter(event => event.kind === 'decision');
      assert.ok(decisions.some(event => event.details.decision === mode), JSON.stringify(decisions));
    }
  });
