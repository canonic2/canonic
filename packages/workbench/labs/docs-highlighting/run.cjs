const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const cp = require('node:child_process');
const { createInterface } = require('node:readline');
const { Browser, findChrome } = require('../../scripts/chrome.cjs');
const { overlay } = require('./overlay.cjs');
const { generate } = require('./fixtures.cjs');
const { edits, faults, lifecycle, copyAndIsolation, exampleFailure } = require('./correctness.cjs');
const { monitor, workerHeaps, watchWorkers } = require('./memory.cjs');
const { compareGroup } = require('./parity.cjs');
const product = path.resolve(__dirname, '../..');
const inputsFile = process.env.WB_HIGHLIGHT_LAB_INPUTS;
const fixedInputs = inputsFile ? JSON.parse(fs.readFileSync(inputsFile, 'utf8')) : null;
const inputProduct = fixedInputs?.product || product;
const root = path.resolve(process.argv[2]);
const mode = process.argv[3] || 'smoke';
const runRoot = path.join(root, new Date().toISOString().replace(/[:.]/g, '-') + '-' + mode);
const runtimeCache = path.join(runRoot, 'runtime-cache');
fs.mkdirSync(runRoot, { recursive: false });
fs.copyFileSync(path.join(__dirname, 'README.md'), path.join(runRoot, 'plan.md'));
const labSource = path.join(runRoot, 'lab-source');
fs.cpSync(__dirname, labSource, {
  recursive: true,
  filter: (file) =>
    !file.includes(path.sep + 'results' + path.sep) && path.basename(file) !== 'results',
});
const probeSource = fs.readFileSync(path.join(labSource, 'probe.js'), 'utf8');
const hash = (value) => crypto.createHash('sha256').update(value).digest('hex');
const report = {
  mode,
  planSha256: hash(fs.readFileSync(path.join(__dirname, 'README.md'))),
  started: new Date().toISOString(),
  host: {
    platform: os.platform(),
    arch: os.arch(),
    cpus: os.cpus().length,
    cpu: os.cpus()[0].model,
    memory: os.totalmem(),
    load: os.loadavg(),
    node: process.version,
    chrome: findChrome(),
  },
  source: {
    binary: hash(
      fs.readFileSync(
        path.join(inputProduct, 'server/bin', process.platform + '-' + process.arch, 'workbench-server'),
      ),
    ),
  },
  samples: [],
  failures: [],
  cleanupFailures: [],
};
report.source.publicHead = cp
  .execFileSync(
    'git',
    ['-C', path.resolve(product, '../../.gate/repos/public'), 'rev-parse', 'HEAD'],
    { encoding: 'utf8' },
  )
  .trim();
report.source.privateHead = cp
  .execFileSync(
    'git',
    ['-C', path.resolve(product, '../../.gate/repos/private'), 'rev-parse', 'HEAD'],
    { encoding: 'utf8' },
  )
  .trim();
report.source.rustToolchain = fs.readFileSync(
  path.join(product, 'server/rust-toolchain.toml'),
  'utf8',
);
report.source.assets = JSON.parse(fs.readFileSync(path.join(root, 'assets.json'), 'utf8'));
report.host.osVersion = os.release();
report.host.power = 'not inspected (host policy disallows pmset)';
report.host.bundledRuntime = JSON.parse(
  fs.readFileSync(path.join(inputProduct, 'capture/runtime/runtime.json'), 'utf8'),
);
report.source.files = {};
const identify = (dir) => {
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const name = path.join(dir, ent.name);
    if (ent.isDirectory() && !['target', 'node_modules', 'runtime', 'results'].includes(ent.name))
      identify(name);
    else if (ent.isFile() && /\.(js|cjs|ts|css|html|rs|json)$/.test(name))
      report.source.files[path.relative(product, name)] = hash(fs.readFileSync(name));
  }
};
for (const dir of ['ui', 'contracts', 'server/node', 'server/src', 'labs/docs-highlighting'])
  identify(path.join(product, dir));
if (fixedInputs) {
  const workspaceHeadsAtLaunch = { public: report.source.publicHead, private: report.source.privateHead };
  const harnessFiles = Object.fromEntries(Object.entries(report.source.files).filter(([name]) => name.startsWith('labs/docs-highlighting/')));
  require('node:assert/strict').equal(report.source.binary, fixedInputs.source.binary);
  report.source = { ...fixedInputs.source, files: { ...fixedInputs.source.files, ...harnessFiles },
    workspaceHeadsAtLaunch, fixedInputsSha256: hash(fs.readFileSync(inputsFile)),
    inheritedFromReportSha256: fixedInputs.reportSha256 };
}
const raw = path.join(runRoot, 'report.json');
const save = () => {
  fs.writeFileSync(raw + '.partial', JSON.stringify(report, null, 2));
  fs.renameSync(raw + '.partial', raw);
};
const size = (folder) => {
  let bytes = 0;
  if (!fs.existsSync(folder)) return 0;
  for (const item of fs.readdirSync(folder, { withFileTypes: true })) {
    const file = path.join(folder, item.name);
    if (item.isDirectory()) bytes += size(file);
    else if (item.isFile()) bytes += fs.statSync(file).size;
  }
  return bytes;
};
const delay = (ms) => new Promise((r) => setTimeout(r, ms));
const children = new Set(),
  browsers = new Set();
let stopping = false,
  fatalClosing = false;
const fatal = (error) => {
  if (fatalClosing) return;
  fatalClosing = true;
  stopping = true;
  report.fatal = String(error?.stack || error);
  save();
  for (const child of children) child.stdin.end();
  void Promise.allSettled([...browsers].map((browser) => browser.close())).then(() => {
    report.finished = new Date().toISOString();
    save();
    process.exit(1);
  });
};
process.on('unhandledRejection', fatal);
process.on('uncaughtException', fatal);
function requireCheck(value, message) {
  if (!value) throw Error(message);
}
async function server(snapshot, fixture, dir) {
  const tmp = path.join(dir, 'tmp');
  fs.mkdirSync(tmp, { recursive: true });
  const child = cp.spawn(
    process.execPath,
    [path.join(labSource, 'serve.cjs'), snapshot, fixture, runtimeCache],
    { env: { ...process.env, TMPDIR: tmp }, stdio: ['pipe', 'pipe', 'pipe'] },
  );
  children.add(child);
  const logs = [];
  let stderr = '';
  child.stderr.on('data', (b) => (stderr += b));
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(Error('Server start timeout ' + stderr)), 20000);
    child.on('exit', () => {
      clearTimeout(timer);
      reject(Error('Server exited ' + stderr));
    });
    createInterface({ input: child.stdout }).on('line', (line) => {
      try {
        const msg = JSON.parse(line);
        if (msg.url) {
          clearTimeout(timer);
          resolve(msg.url);
        } else if (msg.error) {
          clearTimeout(timer);
          reject(Error(msg.error));
        } else if (msg.log) logs.push(msg.log);
      } catch (e) {
        reject(e);
      }
    });
  });
  const close = async () => {
    child.stdin.end();
    await Promise.race([new Promise((r) => child.once('exit', r)), delay(7000)]);
    if (child.exitCode === null && child.signalCode === null) {
      child.kill('SIGTERM');
      report.cleanupFailures.push('Host needed SIGTERM');
    }
    children.delete(child);
  };
  try {
    return { url: await ready, logs, child, close };
  } catch (e) {
    await close();
    throw e;
  }
}
async function sample(variant, shape, index, snapshot, settled, template) {
  const dir = path.join(runRoot, `${index}-${variant}-${shape}-${settled ? 'settled' : 'startup'}`);
  fs.mkdirSync(dir);
  const fixture = path.join(dir, 'fixture');
  fs.cpSync(template.dir, fixture, { recursive: true, mode: fs.constants.COPYFILE_FICLONE });
  const manifest = template.manifest;
  const entry = {
    variant,
    shape,
    index,
    cache: settled ? 'fresh-profile-after-examples-settled' : 'cold-startup',
    fixture: { files: manifest.files, bytes: manifest.bytes, hashes: manifest.hashes },
    cells: [],
    checks: {},
    loadBefore: os.loadavg(),
  };
  let running, browser, target, memory;
  try {
    running = await server(snapshot, fixture, dir);
    if (settled) {
      const url = new URL('/_workbench/docs/bundle', running.url);
      url.searchParams.set('page', manifest.first);
      url.searchParams.set('lens', 'html');
      const r = await fetch(url);
      requireCheck(r.ok, 'Examples settlement failed');
      await r.arrayBuffer();
    }
    browser = new Browser({profile:path.join(dir,'profile'),spawn:(file,args,options)=>cp.spawn(file,args,{...options,env:{...process.env,TMPDIR:path.join(dir,'tmp')}})});
    browsers.add(browser);
    await browser.launch();
    target = await browser.createTarget();
    entry.browserVersion = await browser.pipe.send('Browser.getVersion');
    memory = monitor(running.child.pid, () => browser.child?.pid);
    await target.send('Emulation.setDeviceMetricsOverride', {
      width: 1400,
      height: 1000,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await target.send('Performance.enable');
    const errors = [];
    target.on('Runtime.exceptionThrown', (e) =>
      errors.push({
        text: e.exceptionDetails.text,
        description: e.exceptionDetails.exception?.description,
        url: e.exceptionDetails.url,
        line: e.exceptionDetails.lineNumber,
      }),
    );
    const external = [];
    await target.send('Network.enable');
    target.on('Network.requestWillBeSent', (e) => {
      if (/^https?:/.test(e.request.url) && !e.request.url.startsWith(new URL(running.url).origin))
        external.push(e.request.url);
    });
    await target.send('Page.addScriptToEvaluateOnNewDocument', { source: probeSource });
    await target.navigate(running.url);
    await target.evaluate(
      `(async()=>{const deadline=performance.now()+15000;while(!window.__labSelect||!window.wbItem?.(${JSON.stringify(manifest.first)})){if(performance.now()>deadline)throw Error('Page list never became selectable');await new Promise(r=>setTimeout(r,5));}return true;})()`,
    );
    const select = async (src, action) => {
      const value = await target.evaluate(
        `__labSelect(${JSON.stringify(src)},${JSON.stringify(action)})`,
      );
      requireCheck(
        value.assets.fonts === 'loaded' && value.assets.images,
        'Readiness assets incomplete',
      );
      requireCheck(
        value.assets.examples.every((e) => e.status === 'ready'),
        'Example failed',
      );
      entry.cells.push(value);
      return value;
    };
    const first = await select(manifest.first, 'first');
    const expected = manifest.expected[manifest.first] || [];
    requireCheck(
      JSON.stringify(first.blocks.map((b) => ({ language: b.language, text: b.text }))) ===
        JSON.stringify(expected),
      'Fence source differs',
    );
    entry.sourceFirst = await target.evaluate(
      `__labSource(${JSON.stringify(manifest.first)},${JSON.stringify(manifest.example)})`,
    );
    for (let warmup = 0; warmup < 3; warmup++)
      await target.evaluate(
        `__labSource(${JSON.stringify(manifest.first)},${JSON.stringify(manifest.example)})`,
      );
    entry.sourceReopen = await target.evaluate(
      `__labSource(${JSON.stringify(manifest.first)},${JSON.stringify(manifest.example)})`,
    );
    entry.checks.initial = await target.evaluate(
      `__labCorrectness(${JSON.stringify(manifest.first)})`,
    );
    requireCheck(
      entry.checks.initial.geometry && entry.checks.initial.typography,
      'Code geometry or typography changed',
    );
    requireCheck(!entry.checks.initial.unexpectedMarkup, 'Code injected markup');
    requireCheck(!entry.checks.initial.workerErrors.length, 'Worker failed');
    if (variant !== 'D' && first.blocks.length)
      requireCheck(
        first.blocks.every((b) => b.spans > 0),
        'Fence colouring fell back',
      );
    if (variant !== 'D') requireCheck(entry.sourceFirst.spans > 0, 'Show code colouring fell back');
    await select(manifest.second, 'second');
    for (let warmup = 0; warmup < 3; warmup++) {
      await target.evaluate(`__labSelect(${JSON.stringify(manifest.first)},'retained-warmup')`);
      await target.evaluate(`__labSelect(${JSON.stringify(manifest.second)},'retained-warmup')`);
    }
    const retained = await select(manifest.first, 'retained');
    requireCheck(retained.documentId === first.documentId, 'Retained document was reloaded');
    entry.checks.retained = true;
    for (let warmup = 0; warmup < 3; warmup++) {
      for (let i = 0; i < 5; i++)
        await target.evaluate(`__labSelect('evict-${i}.md','eviction-warmup')`);
      await target.evaluate(`__labSelect(${JSON.stringify(manifest.first)},'eviction-warmup')`);
    }
    for (let i = 0; i < 5; i++) await select(`evict-${i}.md`, 'eviction-transit');
    const evicted = await select(manifest.first, 'evicted');
    requireCheck(evicted.documentId !== first.documentId, 'Eviction did not evict first document');
    entry.checks.evicted = true;
    entry.memoryAfterReturns = await memory.snapshot();
    requireCheck(!entry.memoryAfterReturns.errors.length, 'Process memory unavailable');
    await watchWorkers(browser, target);
    await target.send('Runtime.evaluate', { expression: 'true', returnByValue: true });
    entry.workerHeaps = await workerHeaps(browser);
    entry.workerHeapObservation =
      'after navigation timings and normal RSS; inspector detached before subsequent sequences';
    await target.send('Target.setAutoAttach', {
      autoAttach: false,
      waitForDebuggerOnStart: false,
      flatten: true,
    });
    // Source identity and example output are checked independently of provider HTML.
    const original = fs.readFileSync(
      path.join(fixture, manifest.first.replace('.md', '.examples.ts')),
      'utf8',
    );
    const start = original.indexOf(
      'export const ' + (shape === 'Overview' ? 'tokensInUse' : 'basic'),
    );
    requireCheck(
      entry.sourceFirst.text === original.slice(start).trimEnd(),
      'Show code source differs',
    );
    entry.checks.source = true;
    entry.navigationMeasurementsComplete = true;
    const corpus = await select('corpus.md', 'corpus');
    requireCheck(
      JSON.stringify(corpus.blocks.map((b) => ({ language: b.language, text: b.text }))) ===
        JSON.stringify(manifest.expected['corpus.md']),
      'Corpus source differs',
    );
    entry.checks.corpus = await target.evaluate(
      `__labRoles('corpus.md',${JSON.stringify(manifest.corpus)})`,
    );
    entry.checks.tokenRoles =
      variant === 'D'
        ? 'diagnostic'
        : entry.checks.corpus.every((b) => b.roles.every((r) => r.pass));
    // Keep this correctness finding visible while finishing the smoke group.
    // A sample with failed roles is ineligible for a performance verdict.
    entry.checks.cleanConsole = errors;
    entry.checks.externalRequests = external;
    requireCheck(!errors.length, 'Console exceptions');
    requireCheck(!external.length, 'Runtime external request');
    entry.edits = await edits(target, fixture, manifest);
    entry.checks.copyIsolation = await copyAndIsolation(target, manifest);
    entry.checks.lifecycle = await lifecycle(target, manifest);
    requireCheck(
      entry.checks.lifecycle.stale && entry.checks.lifecycle.disposed,
      'Stale/disposed source applied',
    );
    await target.evaluate(
      `__labSource(${JSON.stringify(manifest.first)},${JSON.stringify(manifest.example)})`,
    );
    entry.checks.capture = await target.evaluate(`__labCapture(${JSON.stringify(manifest.first)})`);
    requireCheck(
      entry.checks.capture.bytes > 100 &&
        entry.checks.capture.src === manifest.first &&
        entry.checks.capture.docs,
      'Invalid docs capture',
    );
    entry.visual = await target.evaluate(`__labCorrectness(${JSON.stringify(manifest.first)},true)`);
    requireCheck(
      entry.visual.geometry && entry.visual.typography,
      'Capture code geometry or typography changed',
    );
    fs.writeFileSync(path.join(dir, 'settled-normal.png'), await target.screenshot());
    await target.evaluate(`(async()=>{const {doc,w}=__labFind(${JSON.stringify(manifest.first)});const style=doc.createElement('style');style.id='__lab-raster-colour';style.textContent='.wb-docs-code code span,.wb-docs-example-code code span{color:inherit!important}';doc.head.append(style);await new Promise(r=>w.requestAnimationFrame(()=>w.requestAnimationFrame(r)));return true;})()`);
    fs.writeFileSync(path.join(dir,'settled-neutral.png'),await target.screenshot());
    await target.evaluate(`(async()=>{const {doc,w}=__labFind(${JSON.stringify(manifest.first)});doc.getElementById('__lab-raster-colour').remove();await new Promise(r=>w.requestAnimationFrame(()=>w.requestAnimationFrame(r)));return true;})()`);
    entry.checks.exampleFailure = await exampleFailure(target, fixture, manifest);
    entry.checks.faults = await faults(target, variant, manifest, running.url);
    const preparations = running.logs.filter(log => log.event === 'runtime.prepared');
    entry.checks.runtimeCacheReused = preparations.length > 0 && preparations.every(log => log.details?.extracted === false);
    requireCheck(entry.checks.runtimeCacheReused, 'Sample unexpectedly extracted the packaged runtime');
    entry.status = 'passed';
    entry.memory = await target.send('Memory.getDOMCounters');
    entry.metrics = await target.send('Performance.getMetrics');
    fs.writeFileSync(path.join(dir, 'settled-fallback.png'), await target.screenshot());
  } catch (e) {
    entry.status = 'failed';
    entry.error = String(e.stack || e);
    if (target)
      try {
        const state = await target.send(
          'Runtime.evaluate',
          {
            expression: `({url:location.href,visibility:document.visibilityState,text:document.body?.innerText.slice(0,500),frames:[...document.querySelectorAll('iframe')].map(f=>({src:f.src,visibility:getComputedStyle(f).visibility}))})`,
            returnByValue: true,
          },
          { timeout: 2000 },
        );
        entry.failureState = state.result?.value;
      } catch (error) {
        entry.failureState = { unavailable: String(error) };
      }
    report.failures.push({ variant, shape, index, error: entry.error });
  } finally {
    if (memory) entry.processMemory = await memory.stop().catch((e) => ({ error: String(e) }));
    if (target) await target.close().catch((e) => report.cleanupFailures.push(String(e)));
    if (browser) {
      await browser.close().catch((e) => report.cleanupFailures.push(String(e)));
      browsers.delete(browser);
    }
    if (running) {
      entry.logs = running.logs;
      await running.close().catch((e) => report.cleanupFailures.push(String(e)));
    }
    entry.checks.fixtureRestored = Object.entries(manifest.hashes).every(
      ([name, digest]) =>
        fs.existsSync(path.join(fixture, name)) &&
        hash(fs.readFileSync(path.join(fixture, name))) === digest,
    );
    if (!entry.checks.fixtureRestored) {
      entry.status = 'failed';
      entry.error = (entry.error || '') + '\nFixture source did not restore exactly';
      report.failures.push({ variant, shape, index, error: 'Fixture restoration failed' });
    }
    entry.cacheBytes = {
      compiler: size(path.join(dir, 'tmp/canonic-workbench-rspack')),
      index: size(path.join(dir, 'tmp/canonic-workbench-index')),
      runtime: size(runtimeCache),
      runtimeScope: 'Shared immutable packaged runtime per run set; extraction recorded before samples',
      browserProfile: size(path.join(dir, 'profile')),
    };
    entry.correctness = {
      tokenRoles: entry.checks.tokenRoles,
      execution: entry.status === 'passed',
      eligible: false, // Group render parity has not been checked yet.
    };
    entry.loadAfter = os.loadavg();
    // Keep bulky complete traces per sample, avoiding a multi-hundred-MB trace
    // rewrite after every cell in the 480-sample sets. Final parity lives in
    // report.json; this supplemental snapshot precedes group comparison.
    entry.rawSample = path.relative(runRoot, path.join(dir, 'sample.json'));
    fs.writeFileSync(path.join(dir, 'sample.json'), JSON.stringify(entry, null, 2));
    entry.logs = { rawSample: entry.rawSample, field: 'logs' };
    if (entry.processMemory?.samples)
      entry.processMemory = { ...entry.processMemory, samples: undefined, rawSample: entry.rawSample };
    report.samples.push(entry);
    save();
    for (const generated of ['fixture', 'tmp', 'storage', 'profile'])
      try {
        fs.rmSync(path.join(dir, generated), { recursive: true, force: true });
      } catch (error) {
        report.cleanupFailures.push({
          sample: path.basename(dir),
          generated,
          error: String(error),
        });
      }
    console.log(
      JSON.stringify({
        variant,
        shape,
        index,
        cache: entry.cache,
        status: entry.status,
        error: entry.error?.split('\n')[0],
        first: entry.cells[0] && {
          visible: entry.cells[0].visibleMs,
          coloured: entry.cells[0].colouredMs,
        },
        report: raw,
      }),
    );
    if (mode === 'smoke' && entry.status === 'failed') stopping = true;
  }
}
async function main() {
  const base = overlay(path.join(runRoot, 'source'), '0', path.join(root, 'assets'), inputProduct);
  require('node:assert/strict').equal(hash(fs.readFileSync(path.join(base, 'server/bin', process.platform + '-' + process.arch, 'workbench-server'))), report.source.binary);
  for (const name of ['node_modules', 'capture/runtime']) {
    fs.unlinkSync(path.join(base, name));
    fs.cpSync(path.join(inputProduct, name), path.join(base, name), {
      recursive: true,
      verbatimSymlinks: true,
      mode: fs.constants.COPYFILE_FICLONE,
    });
  }
  const templates = {};
  for (const shape of ['Small', 'Overview', 'Code-heavy']) {
    const dir = path.join(runRoot, 'templates', shape);
    if (fixedInputs) {
      const input = fixedInputs.templates[shape];
      fs.cpSync(input.dir, dir, { recursive: true, mode: fs.constants.COPYFILE_FICLONE });
      requireCheck(Object.entries(input.manifest.hashes).every(([name, digest]) => hash(fs.readFileSync(path.join(dir, name))) === digest), 'Frozen fixture identity changed');
      templates[shape] = { dir, manifest: input.manifest };
    } else templates[shape] = { dir, manifest: generate(dir, shape, path.join(root, 'assets')) };
  }
  const snapshots = {};
  for (const variant of ['0', '1', '2', 'D'])
    snapshots[variant] = overlay(
      path.join(runRoot, 'product-' + variant),
      variant,
      path.join(root, 'assets'),
      base,
      labSource,
    );
  const runtimeStarted = performance.now();
  const runtimeEvents = [];
  await require(path.join(base, 'capture/electron-runtime.js')).prepare(
    runtimeCache, null, (event, details) => runtimeEvents.push({ event, ...details }),
  );
  report.runtimePreparation = { durationMs: performance.now() - runtimeStarted, events: runtimeEvents };
  // This is host admission, not a warmup: no server, browser, grammar or page
  // has run. Retain all observations, including inability to obtain a quiet host.
  if (mode !== 'smoke') {
    report.hostAdmission = [];
    const deadline = performance.now() + 600000;
    let quietSince;
    while (!stopping) {
      const now = performance.now(), load = os.loadavg();
      report.hostAdmission.push({ elapsedMs: now - runtimeStarted, load });
      if (load[0] <= os.cpus().length) quietSince ??= now;
      else quietSince = undefined;
      save();
      console.log(JSON.stringify({ admission: 'waiting for load <= logical CPUs for 60s', load, quietMs: quietSince === undefined ? 0 : now - quietSince }));
      if (quietSince !== undefined && now - quietSince >= 60000) break;
      if (now >= deadline) {
        report.hostQualification = 'Quiet-host admission not obtained within 10 minutes; collecting the authorized performance matrix with observed host load';
        break;
      }
      await delay(10000);
    }
  }
  save();
  const count = mode === 'smoke' ? 1 : 20;
  let state = 20261020 + (mode === 'set2' ? 1 : mode === 'confirmation' ? 2 : 0);
  report.orderSeed = state;
  const random = () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 4294967296;
  };
  for (let i = 0; i < count && !stopping; i++)
    for (const shape of process.argv[5]
      ? process.argv[5].split(',')
      : ['Small', 'Overview', 'Code-heavy'])
      for (const settled of [false, true]) {
        const allowed = process.argv[4] ? process.argv[4].split(',') : ['0', '1', '2', 'D'];
        const variants =
          mode === 'smoke'
            ? allowed
            : allowed
                .map((v) => ({ v, order: random() }))
                .sort((a, b) => a.order - b.order)
                .map((e) => e.v);
        for (const variant of variants) {
          if (stopping) break;
          await sample(variant, shape, i, snapshots[variant], settled, templates[shape]);
        }
        if (!stopping && variants.includes('0') && variants.length > 1) {
          const group = report.samples.filter(
            (sample) =>
              sample.index === i &&
              sample.shape === shape &&
              sample.cache === (settled ? 'fresh-profile-after-examples-settled' : 'cold-startup'),
          );
          const parity = await compareGroup(runRoot, group);
          report.parity ||= [];
          report.parity.push({ index: i, shape, settled, ...parity });
          save();
          // Classified raster findings remain failures in the evidence. Finish
          // the other cells rather than hiding their correctness or timings.
        }
      }
  report.finished = new Date().toISOString();
  save();
  console.log(
    JSON.stringify({
      report: raw,
      samples: report.samples.length,
      failures: report.failures.length,
    }),
  );
}
for (const signal of ['SIGINT', 'SIGTERM'])
  process.on(signal, () => {
    stopping = true;
    for (const child of children) child.stdin.end();
  });
main().catch((error) => {
  report.fatal = String(error.stack || error);
  save();
  for (const child of children) child.stdin.end();
  console.error(error);
  process.exitCode = 1;
});
