// Neutral benchmark results: one JSON summary per recorded run, grouped by the
// subject's version (Shield's or the pack's), with generated README pages.
import { createHash } from 'node:crypto';
import { mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, join } from 'node:path';
import { offerClassifier } from './decisions.mjs';

const RECORD_COMMAND = 'npm --prefix packages/shield/adapters/sdk-tests run benchmark:record';

async function oracleDigest(scenario) {
  const hash = createHash('sha256');
  for (const name of ['checks.mjs', 'decisions.mjs', 'transcript.mjs']) hash.update(await readFile(new URL(`./${name}`, import.meta.url)));
  return hash.update(scenario.sha256).digest('hex');
}

/** Replaces machine-specific paths and identifiers with neutral placeholders. */
function neutral(value, run) {
  const replacements = [[run.project, '<project>'], [run.root, '<run>'], [homedir(), '~'],
    [homedir().replaceAll('/', '-'), '~'], [run.identities.session_id, '<session>']].filter(([from]) => from);
  if (typeof value === 'string') return replacements.reduce((text, [from, to]) => text.replaceAll(from, to), value);
  if (Array.isArray(value)) return value.map(item => neutral(item, run));
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, neutral(item, run)]));
  return value;
}

const shieldVersion = run => run.identities.shield_version.replace(/^shield\s+/, '');

/** The committed summary of one run: identities, conditions, outcomes and costs, never raw content. */
export async function summary(run, result, scenario, { role, note } = {}) {
  const classify = offerClassifier(scenario.offers);
  const seconds = turn => Math.round((Date.parse(turn.finished_at) - Date.parse(turn.started_at)) / 1000);
  const turns = run.turns.map(turn => ({ id: turn.id, status: turn.result?.status ?? null, seconds: seconds(turn),
    cost_usd: turn.result?.cost_usd ?? null, total_tokens: turn.result?.tokens?.total?.totalTokens ?? null }));
  const last = turns.at(-1) ?? {};
  return neutral({
    schema_version: 3,
    scenario: run.scenario,
    scenario_sha256: run.scenario_sha256 ?? null,
    scoring_scenario_sha256: scenario.sha256,
    oracle_sha256: await oracleDigest(scenario),
    subject: run.identities.subject,
    role: role ?? null,
    note: note ?? null,
    started_at: run.turns[0]?.started_at ?? null,
    shield: { version: shieldVersion(run), sha256: run.identities.shield_sha256 },
    host: { name: run.host, cli: run.identities.host_cli, version: run.identities.host_version ?? null, model: run.identities.model ?? null },
    platform: run.identities.platform,
    node: run.identities.node,
    packs: run.identities.packs,
    conditions: run.conditions,
    decision_set: run.decision_set,
    decisions: run.decision_table,
    verdict: result.verdict,
    counts: result.counts,
    checks: result.checks.map(({ id, status, attribution, summary: text }) => ({ id, status, attribution, summary: text })),
    friction: result.friction,
    offers: run.decision_log.filter(item => item.kind === 'offer')
      .map(item => ({ offer: item.text === undefined ? item.offer : classify(item.text), action: item.action, at: item.at })),
    turns,
    totals: { seconds: turns.reduce((sum, turn) => sum + turn.seconds, 0), cost_usd: last.cost_usd, total_tokens: last.total_tokens },
  }, run);
}

const usage = row => row.totals.cost_usd != null ? `$${row.totals.cost_usd.toFixed(2)} est.`
  : row.totals.total_tokens != null ? `${(row.totals.total_tokens / 1e6).toFixed(2)}M tokens` : '';
const minutes = row => `${Math.round(row.totals.seconds / 60)} min`;
const outcome = row => ['pass', 'fail', 'unverified'].map(status => row.counts[status] ?? 0).join(' / ');
const semver = version => version.split(/[.+-]/).slice(0, 3).map(Number);
const newestFirst = (a, b) => semver(b).reduce((order, part, index) => order || part - semver(a)[index], 0);

async function recorded(root) {
  const versions = {};
  for (const version of await readdir(root, { withFileTypes: true }).catch(() => [])) {
    if (!version.isDirectory() || !/^\d+\.\d+\.\d+/.test(version.name)) continue;
    for (const file of (await readdir(join(root, version.name))).filter(name => name.endsWith('.json')).sort()) {
      (versions[version.name] ??= []).push({ file, ...JSON.parse(await readFile(join(root, version.name, file), 'utf8')) });
    }
  }
  return versions;
}

/** Regenerates each version's page and the index of one results folder. */
export async function index(root, subject) {
  const title = subject.kind === 'shield' ? 'Shield' : subject.id;
  const versions = await recorded(root);
  const header = ['| Run | Scenario | Role | Host and model | Shield | Verdict | Pass / fail / unverified | Time | Usage |',
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- |'];
  const row = (prefix, item) => `| [${item.file.replace(/\.json$/, '')}](${prefix}${item.file}) | ${item.scenario} | ${item.role ?? ''} | ${item.host.name} ${item.host.version ?? ''}, ${item.host.model ?? ''} | ${item.shield.version} \`${item.shield.sha256.slice(0, 12)}\` | ${item.verdict} | ${outcome(item)} | ${minutes(item)} | ${usage(item)} |`;
  const generated = `Generated by \`${RECORD_COMMAND}\`; do not edit by hand.`;
  for (const [version, items] of Object.entries(versions)) {
    const name = file => file.replace(/\.json$/, '');
    const failures = items.flatMap(item => item.checks.filter(check => check.status !== 'pass')
      .map(check => `- \`${name(item.file)}\` ${check.id}: ${check.status}${check.attribution ? ` (${check.attribution})` : ''}. ${check.summary}`));
    const notes = items.filter(item => item.note).map(item => `- \`${name(item.file)}\`: ${item.note}`);
    await writeFile(join(root, version, 'README.md'), [`# ${title} ${version} benchmarks`, '', generated, '',
      ...header, ...items.map(item => row('', item)), '',
      ...(notes.length ? ['## Notes', '', ...notes, ''] : []),
      ...(failures.length ? ['## Checks that did not pass', '', ...failures, ''] : [])].join('\n'));
  }
  await writeFile(join(root, 'README.md'), [`# ${title} benchmarks`, '',
    `Benchmark runs of real agents through fixed scenarios, grouped by ${subject.kind === 'shield' ? 'Shield' : 'pack'} version.`,
    'Every file is a neutral summary written by Shield\'s agent benchmark harness: identities, conditions,',
    'decisions, check outcomes, policy friction, time and usage. Transcripts and raw evidence are never committed.', '',
    generated, '',
    ...Object.keys(versions).sort(newestFirst).flatMap(version => [`## [${version}](${version}/README.md)`, '', ...header,
      ...versions[version].map(item => row(`${version}/`, item)), ''])].join('\n'));
}

/** Writes the summary of a scored run under its subject's version and regenerates the pages. */
export async function record(run, result, scenario, options = {}) {
  const entry = await summary(run, result, scenario, options);
  const version = entry.subject.kind === 'pack' ? entry.subject.version : entry.shield.version;
  const folder = join(scenario.results, version);
  await mkdir(folder, { recursive: true });
  const stamp = basename(run.root).match(/^\d{8}T\d{6}/)?.[0] ?? entry.started_at.replace(/[-:]/g, '').replace(/\..+/, '');
  const path = join(folder, `${stamp}-${run.decision_set}.json`);
  await writeFile(path, `${JSON.stringify(entry, null, 2)}\n`);
  await index(scenario.results, entry.subject);
  return path;
}
