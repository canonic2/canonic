// Durable queue: finish the existing first set, then run set 2 and confirmation.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const cp = require('node:child_process');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { publish, campaignReport } = require('./performance-report.cjs');
const { generate } = require('./fixtures.cjs');
const sha = body => crypto.createHash('sha256').update(body).digest('hex');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

function analyze(file, destination, inputsFile) {
  const root = path.dirname(path.dirname(file));
  const result = publish(file, destination, [[path.resolve(__dirname, '../../../..'), '<workspace>'], [root, '<lab>'], [os.homedir(), '<home>']]);
  const bytes = fs.readFileSync(file), report = JSON.parse(bytes);
  if (report.fatal) throw Error(report.fatal);
  const variants = report.mode === 'confirmation' ? [...new Set(report.samples.map(sample => sample.variant))] : ['0', '1', '2', 'D'];
  for (const variant of variants) for (const shape of ['Small', 'Overview', 'Code-heavy'])
    for (const cache of ['cold-startup', 'fresh-profile-after-examples-settled'])
      assert.equal(report.samples.filter(sample => sample.variant === variant && sample.shape === shape && sample.cache === cache).length, 20,
        `Incomplete attempted matrix: ${variant}/${shape}/${cache}`);
  if (inputsFile) {
    const scratch = path.join(path.dirname(inputsFile), 'manifest-derivation');
    const templates = {};
    try {
      for (const shape of ['Small', 'Overview', 'Code-heavy']) {
        const manifest = generate(path.join(scratch, shape), shape, path.join(root, 'assets'));
        const original = report.samples.find(sample => sample.shape === shape).fixture;
        assert.deepEqual(manifest.hashes, original.hashes, `Fixture inputs changed before manifest freezing: ${shape}`);
        templates[shape] = { dir: path.join(path.dirname(file), 'templates', shape), manifest };
      }
      fs.writeFileSync(inputsFile, JSON.stringify({ product: path.join(path.dirname(file), 'source'),
        source: report.source, reportSha256: sha(bytes), templates }));
    } finally { fs.rmSync(scratch, { recursive: true, force: true }); }
  }
  return result;
}

async function campaign(root, firstReport, output, firstPid) {
  const lock = path.join(root, 'performance-campaign.lock');
  fs.writeFileSync(lock, JSON.stringify({ pid: process.pid, started: new Date().toISOString() }), { flag: 'wx' });
  const stateFile = path.join(root, 'performance-campaign.json');
  const state = { pid: process.pid, started: new Date().toISOString(), phase: 'waiting-for-set1', firstReport, output, results: [] };
  let child, cancelled = false;
  const save = () => { fs.writeFileSync(stateFile + '.partial', JSON.stringify(state, null, 2)); fs.renameSync(stateFile + '.partial', stateFile); };
  const emit = event => { Object.assign(state, event); save(); process.stdout.write(JSON.stringify(event) + '\n'); };
  const stop = signal => {
    cancelled = true;
    if (child) child.kill(signal);
    else if (state.phase === 'waiting-for-set1' && firstPid) {
      try {
        const args = cp.execFileSync('ps', ['-p', String(firstPid), '-o', 'args='], { encoding: 'utf8' }).trim();
        if (args.endsWith(`${root} set1`)) process.kill(firstPid, signal);
      } catch {}
    }
  };
  process.on('SIGINT', () => stop('SIGINT'));
  process.on('SIGTERM', () => stop('SIGTERM'));
  const runChild = (args, env = process.env, capture = false) => new Promise((resolve, reject) => {
    child = cp.spawn(process.execPath, args, { env, stdio: ['ignore', capture ? 'pipe' : 'inherit', 'inherit'] });
    let stdout = '';
    if (capture) child.stdout.on('data', chunk => { stdout += chunk; });
    child.once('error', reject);
    child.once('exit', code => { child = null; code === 0 && !cancelled ? resolve(stdout) : reject(Error(`Campaign child exited ${code}; cancelled=${cancelled}`)); });
  });
  const analyzeChild = async (file, name, inputs) => {
    const args = [__filename, 'analyze', file, path.join(output, name)];
    if (inputs) args.push(inputs);
    const result = JSON.parse((await runChild(args, process.env, true)).trim());
    state.results.push({ name, ...result }); save();
    return result;
  };
  const runSet = async (mode, variants, inputs) => {
    emit({ phase: mode });
    const before = new Set(fs.readdirSync(root));
    const args = [path.join(__dirname, 'run.cjs'), root, mode];
    if (variants) args.push(variants.join(','));
    await runChild(args, { ...process.env, WB_HIGHLIGHT_LAB_INPUTS: inputs });
    const directories = fs.readdirSync(root).filter(name => !before.has(name) && name.endsWith('-' + mode));
    assert.equal(directories.length, 1, 'Run directory must be unique');
    return path.join(root, directories[0], 'report.json');
  };
  try {
    save();
    // Read only the tail, avoiding repeated parsing of the growing first report.
    while (!cancelled) {
      let finished = false;
      try {
        const fd = fs.openSync(firstReport, 'r');
        try {
          const size = fs.fstatSync(fd).size, tail = Buffer.alloc(Math.min(size, 1024));
          fs.readSync(fd, tail, 0, tail.length, size - tail.length);
          finished = /\n  "finished": "[^"\n]+"\n\}$/.test(tail.toString());
        } finally { fs.closeSync(fd); }
      } catch (error) { if (error.code !== 'ENOENT') throw error; }
      if (finished) break;
      if (firstPid) {
        try { process.kill(firstPid, 0); }
        catch (error) { if (error.code === 'ESRCH') throw Error('First runner exited without a terminal report; partial evidence retained'); }
      }
      await delay(5000);
    }
    if (cancelled) throw Error('Campaign cancelled');
    await delay(3000);
    fs.mkdirSync(output, { recursive: false });
    const inputs = path.join(root, 'performance-campaign-inputs.json');
    assert.equal(fs.existsSync(inputs), false, 'Refuse to replace existing frozen campaign inputs');
    emit({ phase: 'analyzing-set1' });
    await analyzeChild(firstReport, 'set1', inputs);
    const secondReport = await runSet('set2', null, inputs);
    emit({ phase: 'analyzing-set2' });
    await analyzeChild(secondReport, 'set2');
    const summaries = ['set1', 'set2'].map(name => JSON.parse(fs.readFileSync(path.join(output, name, 'summary.json'))));
    const scores = ['1', '2'].map(variant => {
      const cells = summaries.flatMap(summary => ['cold-startup', 'fresh-profile-after-examples-settled'].map(cache =>
        summary.metrics.find(row => row.variant === variant && row.shape === 'Code-heavy' && row.cache === cache && row.metric === 'first.colouredMs')));
      return { variant, cells, meanP50: cells.every(Boolean) ? cells.reduce((n, row) => n + row.p50, 0) / cells.length : Infinity };
    }).sort((a, b) => a.meanP50 - b.meanP50);
    assert(Number.isFinite(scores[0].meanP50), 'No measurable performance leader');
    state.leader = scores[0].variant;
    emit({ phase: 'leader-selected-for-performance-confirmation', scores, leader: state.leader });
    const confirmationReport = await runSet('confirmation', ['0', state.leader], inputs);
    emit({ phase: 'analyzing-confirmation' });
    await analyzeChild(confirmationReport, 'confirmation');
    const final = { started: state.started, finished: new Date().toISOString(), leader: state.leader,
      selectionRule: 'Lowest mean Code-heavy first-colour p50 over two caches and two sets; performance only, not adoption eligibility',
      scores, sets: state.results.map(({ name, samples, completeNavigation, executionFailures }) => ({ name, samples, completeNavigation, executionFailures })),
      note: 'Known correctness findings remain. Full data and actual n are in each set; no product adoption is authorized.' };
    fs.writeFileSync(path.join(output, 'campaign.json'), JSON.stringify(final, null, 2));
    campaignReport(output, final);
    emit({ phase: 'complete', finished: final.finished });
  } catch (error) {
    emit({ phase: cancelled ? 'cancelled' : 'error', error: String(error.stack || error) });
    throw error;
  } finally { fs.unlinkSync(lock); }
}

if (require.main === module) {
  if (process.argv[2] === 'analyze') {
    const result = analyze(...process.argv.slice(3));
    const destination = process.argv[4];
    if (path.basename(destination) === 'confirmation') {
      const stateFile = path.join(path.dirname(path.dirname(process.argv[3])), 'performance-campaign.json');
      if (fs.existsSync(stateFile)) {
        const state = JSON.parse(fs.readFileSync(stateFile));
        if (state.output === path.dirname(destination) && state.results.length === 2)
          campaignReport(state.output, { leader: state.leader,
            sets: [...state.results, { name: 'confirmation', ...result }] });
      }
    }
    process.stdout.write(JSON.stringify(result) + '\n');
  }
  else campaign(path.resolve(process.argv[2]), path.resolve(process.argv[3]), path.resolve(process.argv[4]), Number(process.argv[5])).catch(error => { console.error(error); process.exitCode = 1; });
}
module.exports = { analyze };
