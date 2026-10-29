// Agent benchmark: drive a real agent through a scenario, observe independently, judge offline.
// Usage: node benchmark/run.mjs --host claude|codex --decisions <set> [--scenario <name|folder>]
//        [--runs <folder>] [--agent-home <folder>] [--shield <executable>] [--label <name>]
//        [--setup-only] [--record [--role <role>] [--note <text>]]
import { chmod, mkdir } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { exec, which } from './lib/project.mjs';
import { loadScenario } from './lib/scenario.mjs';
import { runScenario } from './lib/session.mjs';
import { score } from './score.mjs';

const { values: options } = parseArgs({ options: {
  host: { type: 'string' }, decisions: { type: 'string' },
  scenario: { type: 'string', default: 'development-workflow' },
  runs: { type: 'string', default: join(homedir(), 'ShieldSessions/runs') },
  'agent-home': { type: 'string' }, shield: { type: 'string' }, label: { type: 'string' },
  // Prepare and observe the fixture without starting an agent or spending inference.
  'setup-only': { type: 'boolean', default: false },
  // Commit-ready summary in the scenario's results folder after scoring.
  record: { type: 'boolean', default: false }, role: { type: 'string' }, note: { type: 'string' },
} });
if (!['claude', 'codex'].includes(options.host) || !options.decisions) {
  throw new Error('usage: node benchmark/run.mjs --host claude|codex --decisions <set> [--scenario <name|folder>]');
}
const scenario = await loadScenario(options.scenario);
const executable = await which(options.host);
const home = options['agent-home'] ?? join(homedir(), 'ShieldSessions/agents', options.host);
await mkdir(home, { recursive: true });
await chmod(home, 0o700);
if (options.host === 'codex' && !options['setup-only']) {
  const status = await exec(executable, ['login', 'status'], { env: { ...process.env, CODEX_HOME: home } }).catch(error => error);
  if (status.code) throw new Error(`Codex is not logged in for ${home}. Run: CODEX_HOME=${home} codex login`);
}

const { output, root } = await runScenario({ scenario, host: options.host, decisionSet: options.decisions,
  shield: options.shield ?? await which('shield'), executable, home, runs: options.runs,
  label: options.label ?? `${scenario.name}-${options.decisions}`, setupOnly: options['setup-only'] });

if (options['setup-only'] && !output.error) {
  process.stdout.write(`Fixture ready without an agent: ${root}\n`);
} else if (output.turns.at(-1)?.observation) {
  process.stdout.write(`${(await score(root, options.record ? { role: options.role, note: options.note } : null)).text}\n${root}\n`);
} else {
  process.stdout.write(`Run stopped before any observation: ${output.error?.message}\n${root}\n`);
}
process.exitCode = output.error ? 2 : 0;
