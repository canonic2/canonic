// Scenario folders: prompts, packs, decisions and limits as data in scenario.json;
// the scenario's own checks and observation hooks as code in scenario.mjs.
import { createHash } from 'node:crypto';
import { readdir, readFile, stat } from 'node:fs/promises';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const shieldScenarios = fileURLToPath(new URL('../scenarios/', import.meta.url));
// Each turn is a full agent round trip with real model usage.
const MAX_TURNS = 10;

/** A bare name is one of Shield's scenarios; anything with a slash is a folder path. */
export const scenarioDirectory = name => name.includes('/') ? resolve(name) : join(shieldScenarios, name);

async function files(directory, found = []) {
  for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) await files(path, found);
    else if (!entry.name.endsWith('.test.mjs')) found.push(path);
  }
  return found;
}

/** Digest of everything that defines the scenario, so a changed prompt or check is visible. */
async function folderDigest(directory) {
  const hash = createHash('sha256');
  for (const path of await files(directory)) hash.update(relative(directory, path)).update('\0').update(await readFile(path));
  return hash.digest('hex');
}

const semver = version => version.split(/[.+-]/).slice(0, 3).map(Number);
const newer = (a, b) => semver(b).reduce((order, part, index) => order || part - semver(a)[index], 0);

/** A pack folder, or a pack's parent folder whose newest version folder is used. */
async function packSource(path) {
  if (await stat(join(path, 'pack.yml')).then(() => true, () => false)) return path;
  const versions = (await readdir(path, { withFileTypes: true })).filter(entry => entry.isDirectory() && /^\d+\.\d+\.\d+/.test(entry.name));
  if (!versions.length) throw new Error(`No pack versions under ${path}`);
  return join(path, versions.map(entry => entry.name).sort(newer)[0]);
}

async function loadPack(directory, pack) {
  const folder = await packSource(resolve(directory, pack.source));
  const bytes = await readFile(join(folder, 'pack.yml'));
  const document = JSON.parse(bytes);
  return { id: document.id, version: document.version, profile: pack.profile ?? 'recommended', variables: pack.variables ?? {},
    file: join(folder, 'pack.yml'), sha256: createHash('sha256').update(bytes).digest('hex') };
}

function validate(data, directory) {
  const problems = [];
  for (const key of ['name', 'description', 'subject', 'results', 'turns', 'decisions', 'limits']) if (data[key] === undefined) problems.push(`missing ${key}`);
  if (data.turns && (!data.turns.length || data.turns.length > MAX_TURNS)) problems.push(`turns must number 1-${MAX_TURNS}`);
  if (new Set(data.turns?.map(turn => turn.id)).size !== data.turns?.length) problems.push('turn ids must be unique');
  if (data.subject !== 'shield' && !data.packs?.some(pack => pack.source)) problems.push('a pack subject needs its pack in packs');
  if (problems.length) throw new Error(`Invalid scenario ${directory}: ${problems.join('; ')}`);
}

export async function loadScenario(name) {
  const directory = scenarioDirectory(name);
  const data = JSON.parse(await readFile(join(directory, 'scenario.json'), 'utf8'));
  validate(data, directory);
  const code = await import(pathToFileURL(join(directory, 'scenario.mjs')).href);
  if (typeof code.checks !== 'function') throw new Error(`${directory}/scenario.mjs must export checks(run, context, kit)`);
  const packs = await Promise.all((data.packs ?? []).map(pack => loadPack(directory, pack)));
  const subject = data.subject === 'shield' ? { kind: 'shield' }
    : { kind: 'pack', ...packs.find(pack => pack.id === data.subject) };
  if (subject.kind === 'pack' && !subject.id) throw new Error(`Scenario subject ${data.subject} is not one of its packs`);
  const tools = Object.fromEntries(Object.entries(data.tools ?? {}).map(([tool, spec]) =>
    [tool, { module: resolve(directory, spec.module), environment_prefix: spec.environment_prefix ?? null }]));
  return { ...data, directory, packs, subject, tools, hooks: code.hooks ?? {}, checks: code.checks,
    results: resolve(directory, data.results), sha256: await folderDigest(directory) };
}
