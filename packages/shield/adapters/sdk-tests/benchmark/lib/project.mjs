// A run's fresh project: layout, Shield enrollment, packs under test, tool stubs
// and the processes the run owns.
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { chmod, copyFile, mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

export const exec = promisify(execFile);
const terminal = fileURLToPath(new URL('../../../../tests/fixtures/operator_prompts.py', import.meta.url));
const toolRunner = fileURLToPath(new URL('./tool-runner.mjs', import.meta.url));

export const digestFile = async path => createHash('sha256').update(await readFile(path)).digest('hex');
export const which = async name => (await exec('/usr/bin/which', [name])).stdout.trim();

/** `{run}` and `{project}` in scenario text name this run's folders. */
const expand = (text, run) => text.replaceAll('{run}', run.root).replaceAll('{project}', run.project);

async function writeFiles(base, entries, run) {
  for (const [name, text] of Object.entries(entries ?? {})) {
    const path = join(base, name);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, expand(text, run));
  }
}

/** Shell shims that run each scenario tool stub instead of the real program. */
async function installTools(run, tools) {
  await mkdir(run.bin, { recursive: true });
  for (const [name, { module, environment_prefix }] of Object.entries(tools)) {
    const shim = join(run.bin, name);
    await writeFile(shim, `#!/bin/sh\nexec ${JSON.stringify(process.execPath)} ${JSON.stringify(toolRunner)} ${JSON.stringify(module)} ${JSON.stringify(name)} ${JSON.stringify(environment_prefix ?? '')} "$@"\n`);
    await chmod(shim, 0o755);
  }
}

/** Creates the run layout: a fresh project, private cache and logs, support files and tools. */
export async function createRun(base, label, scenario) {
  const stamp = new Date().toISOString().replace(/[-:]/g, '').replace(/\..+/, '');
  const root = join(base, `${stamp}-${label}`);
  const project = join(root, scenario.project ?? 'acme-project');
  for (const directory of [project, join(root, 'shield-cache'), join(root, 'shield-logs')]) await mkdir(directory, { recursive: true });
  // Shield refuses a shared or group-readable cache or diagnostic directory.
  for (const directory of [root, join(root, 'shield-cache'), join(root, 'shield-logs')]) await chmod(directory, 0o700);
  const run = { root: await realpath(root), project: await realpath(project), cache: join(root, 'shield-cache'), logs: join(root, 'shield-logs'),
    bin: join(root, 'bin'), toolLogs: join(root, 'tool-calls'), toolLog: join(root, 'tool-calls', 'calls.jsonl') };
  // Sandboxed hosts must be able to write here; it holds nothing else.
  await mkdir(run.toolLogs, { recursive: true });
  await writeFiles(run.project, scenario.seed, run);
  await writeFiles(run.root, scenario.support, run);
  await installTools(run, scenario.tools);
  return run;
}

/** Environment the agent's commands see: tool stubs first on PATH, plus the scenario's variables. */
export function agentEnvironment(run, scenario) {
  const environment = Object.fromEntries(Object.entries(scenario.environment ?? {}).map(([name, value]) => [name, expand(value, run)]));
  if (Object.keys(scenario.tools).length) {
    environment.PATH = `${run.bin}:${process.env.PATH}`;
    environment.BENCHMARK_TOOL_LOG = run.toolLog;
  }
  return environment;
}

/** Runs a Shield operator command through a test-owned terminal with scripted answers. */
async function operator(shield, run, args, answers) {
  const { stdout } = await exec('python3', [terminal, shield, JSON.stringify([...args, '--cache-dir', run.cache, '--project', run.project]),
    `@${JSON.stringify(answers)}`], { timeout: 60000, env: { ...process.env, TERM: 'xterm-256color', NO_COLOR: '1' } });
  const result = JSON.parse(stdout);
  if (result.code !== 0) throw new Error(`shield ${args.join(' ')} failed:\n${result.terminal}\n${result.stderr}`);
  return result.terminal;
}

/** Runs the real `shield init`, keeping its defaults: Baseline and the agent integration. */
export const initialize = (shield, run, agent) =>
  // The tool-permission page keeps its preselection; the final step applies.
  operator(shield, run, ['init', '--agent', agent, '--profile', 'recommended'], ['', 'yes']);

/** Serves the packs under test from a run-local registry and installs them with their settings. */
export async function installPacks(shield, run, packs) {
  if (!packs.length) return [];
  const registry = join(run.root, 'registry');
  await mkdir(registry);
  const catalog = [];
  for (const pack of packs) {
    const path = `${pack.id.replace('/', '-')}-${pack.version}.yml`;
    await copyFile(pack.file, join(registry, path));
    catalog.push({ id: pack.id, version: pack.version, path, sha256: pack.sha256 });
  }
  await writeFile(join(registry, 'catalog.json'), JSON.stringify({ schema_version: 1, packs: catalog }));
  const terminals = [await operator(shield, run, ['registry', 'add', 'benchmark', registry], ['yes'])];
  for (const pack of packs) {
    const settings = Object.entries(pack.variables).flatMap(([name, value]) => ['--set', `${name}=${Array.isArray(value) ? JSON.stringify(value) : value}`]);
    terminals.push(await operator(shield, run, ['pack', 'add', `benchmark/${pack.id}@${pack.version}`, '--profile', pack.profile, ...settings], ['yes']));
  }
  return terminals;
}

export async function shieldJson(shield, project, args) {
  return JSON.parse((await exec(shield, [...args, '--json', '--project', project], { maxBuffer: 64 << 20 })).stdout);
}

/** Digests of agent and Shield configuration files, used to detect reformatting. */
export async function configurationDigests(project) {
  const files = ['.mcp.json', '.claude/settings.local.json', '.claude/settings.json', '.codex/config.toml',
    '.codex/hooks.json', '.codex/rules/shield.rules', '.shield/config.yml', '.shield/rules.yml', '.shield/local.yml'];
  const digests = {};
  for (const file of files) {
    try { digests[file] = await digestFile(join(project, file)); } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
  }
  return digests;
}

/** Processes whose working directory is inside the project; these belong to the run. */
export async function ownedProcesses(project) {
  let stdout = '';
  try {
    ({ stdout } = await exec('/usr/sbin/lsof', ['-a', '-d', 'cwd', '-Fpn'], { maxBuffer: 64 << 20 }));
  } catch (error) { stdout = error.stdout ?? ''; }
  const owned = [];
  let pid;
  for (const line of stdout.split('\n')) {
    if (line.startsWith('p')) pid = Number(line.slice(1));
    else if (line.startsWith('n') && pid && pid !== process.pid) {
      const path = line.slice(1);
      if (path === project || !relative(project, path).startsWith('..')) owned.push(pid);
    }
  }
  if (!owned.length) return [];
  const { stdout: table } = await exec('/bin/ps', ['-o', 'pid=,ppid=,command=', '-p', owned.join(',')]).catch(error => error);
  return (table ?? '').split('\n').filter(Boolean).map(row => {
    const [, id, parent, command] = row.match(/^\s*(\d+)\s+(\d+)\s+(.*)$/) ?? [];
    return { pid: Number(id), ppid: Number(parent), command };
  }).filter(row => row.pid);
}

export async function listeningPorts(pids) {
  if (!pids.length) return [];
  try {
    const { stdout } = await exec('/usr/sbin/lsof', ['-a', '-p', pids.join(','), '-iTCP', '-sTCP:LISTEN', '-P', '-n', '-Fpn']);
    const ports = new Set();
    for (const line of stdout.split('\n')) {
      const port = line.startsWith('n') && line.match(/:(\d+)$/);
      if (port) ports.add(Number(port[1]));
    }
    return [...ports];
  } catch { return []; }
}

/** Stops only processes started inside this run's project. */
export async function stopOwned(project) {
  const stopped = [];
  for (const signal of ['SIGTERM', 'SIGKILL']) {
    const processes = await ownedProcesses(project);
    if (!processes.length) break;
    for (const { pid, command } of processes) {
      try { process.kill(pid, signal); stopped.push({ pid, command, signal }); } catch {}
    }
    await new Promise(resolve => setTimeout(resolve, 1500));
  }
  return { stopped, remaining: await ownedProcesses(project) };
}
