// Instant first page lab, stage 1 runner. See README.md for the plan.
//
//   node labs/instant-first-page/stage1.mjs --set <label> [--results <new directory>]
//     [--groups capabilities,index,builds] [--fixtures small,large-system,giant,colossal,formats]
//     [--samples 10] [--warmups 3] [--quiet 8] [--smoke] [--keep]
//
// Builds the Rust host with lab-owned caches (cargo.mjs), generates and
// installs fixtures in a new run directory under the system temporary
// folder, and drives the host over its line protocol. Entry sources come from
// the shipping pipeline's previewEntry, loaded read-only, so the Rust host and
// the @rspack/core side build the same entries.
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargo } from './cargo.mjs';
import { acquireLabLock, hostSnapshot, waitForQuiet } from '../fixtures/host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const workbench = path.resolve(here, '..', '..');

const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf('--' + name); return index === -1 ? fallback : args[index + 1]; };
const flag = name => args.includes('--' + name);
const smoke = flag('smoke');
const settings = {
  set: option('set', smoke ? 'smoke' : null),
  groups: option('groups', 'capabilities,index,builds').split(','),
  fixtures: option('fixtures', 'small,large-system,giant,colossal,formats').split(','),
  samples: Number(option('samples', smoke ? 1 : 10)),
  warmups: Number(option('warmups', smoke ? 0 : 3)),
  quiet: Number(option('quiet', 8)),
};
const log = message => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);

/** Hashes of the measured lab files, since a commit ID alone does not
 * identify uncommitted code. */
function measured() {
  const out = {};
  const walk = dir => { for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, item.name);
    if (item.isDirectory()) { if (!['results', 'target'].includes(item.name)) walk(file); }
    else out[path.relative(here, file)] = crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex').slice(0, 16);
  } };
  walk(here);
  for (const shared of ['../fixtures/acme-app.mjs', '../fixtures/formats.mjs', '../fixtures/host.mjs', '../fixtures/install.mjs']) out[shared] = crypto.createHash('sha256').update(fs.readFileSync(path.join(here, shared))).digest('hex').slice(0, 16);
  return out;
}

// ---------------------------------------------------------------- main

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (!settings.set) { console.error('Name the run set with --set <label>.'); process.exit(2); }
  const resultsDir = option('results', null);
  if (resultsDir && fs.existsSync(resultsDir)) { console.error('Refusing to overwrite ' + resultsDir); process.exit(2); }
  const built = cargo(['build', '--release'], { stdio: ['ignore', 'ignore', 'inherit'] });
  if (built.status !== 0) { console.error('The Rust host did not build.'); process.exit(1); }
  const runParent = path.join(os.tmpdir(), 'canonic-workbench-labs', 'instant-first-page', 'runs');
  fs.mkdirSync(runParent, { recursive: true });
  const runDir = path.join(fs.realpathSync(runParent), settings.set + '-' + new Date().toISOString().replace(/[:.]/g, '-'));
  fs.mkdirSync(path.join(runDir, 'fixtures'), { recursive: true });
  const release = acquireLabLock('instant-first-page ' + settings.set);
  const report = { plan: 'labs/instant-first-page/README.md', stage: 1, settings, smoke, host: { atStart: hostSnapshot(), platform: process.platform, arch: process.arch, node: process.version }, started: new Date().toISOString(), measured: measured(), results: [], checks: [] };
  try {
    const groups = await import('./stage1-groups.mjs');
    await groups.run({ runDir, settings, report, log, waitForQuiet, hostSnapshot });
  } catch (error) { report.failure = String(error.stack || error); log('Run failed: ' + report.failure); }
  finally {
    report.finished = new Date().toISOString();
    report.host.atEnd = hostSnapshot();
    const redact = text => text.split(runDir).join('<run>').split(workbench).join('<workbench>').split(os.homedir()).join('~');
    fs.writeFileSync(path.join(runDir, 'report.json'), redact(JSON.stringify(report, null, 2)));
    if (resultsDir) { fs.mkdirSync(resultsDir, { recursive: true }); fs.copyFileSync(path.join(runDir, 'report.json'), path.join(resultsDir, 'report.json')); }
    log('Report: ' + path.join(runDir, 'report.json'));
    if (!flag('keep')) try { fs.rmSync(path.join(runDir, 'fixtures'), { recursive: true, force: true }); } catch (error) { console.error('Cleanup failed: ' + error.message); }
    release();
  }
}
