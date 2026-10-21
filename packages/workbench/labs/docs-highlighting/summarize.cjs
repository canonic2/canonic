// Recompute nearest-rank aggregates exclusively from retained raw reports.
const fs = require('node:fs');
const path = require('node:path');
function summarize(report) {
  const groups = new Map();
  const add = (sample, metric, value) => {
    if (typeof value !== 'number' || !Number.isFinite(value)) return;
    const key = [sample.variant, sample.shape, sample.cache, metric].join('|');
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(value);
  };
  const correctness = [];
  const resources = (cell) => {
    const combined = [...(cell.resources || [])];
    for (const work of cell.workers || [])
      for (const resource of work.resources || [])
        if (
          resource.start >= cell.startedAbs &&
          !combined.some(
            (existing) =>
              existing.name === resource.name && Math.abs(existing.start - resource.start) < 1,
          )
        )
          combined.push(resource);
    return combined;
  };
  const dom = (sample, name, cell) => {
    for (const phase of ['template-parse', 'code-insert'])
      add(
        sample,
        name + '.' + phase + 'Ms',
        (cell.domWork || []).filter((e) => e.phase === phase).reduce((n, e) => n + e.duration, 0),
      );
    add(
      sample,
      name + '.longTaskMaxMs',
      Math.max(0, ...(cell.longTasks || []).map((e) => e.duration)),
    );
  };
  const worker = (sample, name, cell, previous = []) => {
    if (!Array.isArray(cell.workers) || !['1', '2'].includes(sample.variant)) return;
    const earlier = new Set(previous.map(job => job.id));
    const jobs = cell.workers.filter(job => typeof job.startedAbs === 'number'
      ? job.startedAbs >= cell.startedAbs : !earlier.has(job.id));
    for (const phase of ['initializationMs', 'grammarInitializationMs', 'tokenizationMs', 'totalWorkerMs'])
      add(sample, name + '.' + phase, jobs.reduce((n, work) => n + (work[phase] || 0), 0));
  };
  for (const sample of report.samples) {
    correctness.push({
      variant: sample.variant,
      shape: sample.shape,
      cache: sample.cache,
      index: sample.index,
      status: sample.status,
      tokenRoles: sample.checks.tokenRoles,
      failedRoles: sample.checks.corpus?.flatMap((block) =>
        block.roles
          .filter((role) => !role.pass)
          .map((role) => ({ language: block.language, ...role })),
      ),
      error: sample.error,
      checks: Object.fromEntries(Object.entries(sample.checks).filter(([key]) => key !== 'corpus')),
    });
    // A later edit/capture correctness failure does not erase completed,
    // independently checked navigation timings. It still rejects adoption.
    const sequenceComplete = sample.status === 'passed' || sample.navigationMeasurementsComplete;
    for (const cell of sample.cells.filter(
      (c) => !['eviction-transit', 'corpus'].includes(c.action),
    )) {
      const observedVisible = typeof cell.startedAbs === 'number' &&
        cell.assets?.fonts === 'loaded' && cell.assets.images &&
        cell.assets.examples.every(example => example.status === 'ready');
      if (!sequenceComplete && !observedVisible) continue;
      add(sample, cell.action + '.visibleMs', cell.visibleMs);
      if (cell.blocks.length && sample.variant !== 'D' && cell.blocks.every(block => block.spans > 0)) {
        add(sample, cell.action + '.visibleAndColouredMs', cell.visibleAndColouredMs);
        add(sample, cell.action + '.colouredMs', cell.colouredMs);
      }
      add(
        sample,
        cell.action + '.longTaskMs',
        cell.longTasks.reduce((n, task) => n + task.duration, 0),
      );
      add(sample, cell.action + '.loaderMs', cell.loaderMs);
      if (cell.action !== 'retained')
        add(sample, cell.action + '.documentRequestMs', cell.document?.requestMs);
      add(
        sample,
        cell.action + '.bytesTransferred',
        resources(cell).reduce((n, resource) => n + resource.transferSize, 0),
      );
      add(
        sample,
        cell.action + '.bytesDecoded',
        resources(cell).reduce((n, resource) => n + resource.decodedBodySize, 0),
      );
      dom(sample, cell.action, cell);
      worker(sample, cell.action, cell, cell.action === 'retained' ? sample.sourceFirst?.workers : []);
      add(sample, cell.action + '.sharedBrowserHeap', cell.heap);
    }
    for (const key of ['sourceFirst', 'sourceReopen']) {
      if (!sequenceComplete && typeof sample[key]?.startedAbs !== 'number') continue;
      add(sample, key + '.readableMs', sample[key]?.readableMs);
      dom(sample, key, sample[key] || {});
      add(
        sample,
        key + '.bytesTransferred',
        resources(sample[key] || {}).reduce((n, r) => n + r.transferSize, 0),
      );
      add(
        sample,
        key + '.bytesDecoded',
        resources(sample[key] || {}).reduce((n, r) => n + r.decodedBodySize, 0),
      );
      if (sample.variant !== 'D' && sample[key]?.spans > 0)
        add(sample, key + '.colouredMs', sample[key]?.colouredMs);
      worker(sample, key, sample[key] || {}, key === 'sourceFirst'
        ? sample.cells.find(cell => cell.action === 'first')?.workers
        : sample.sourceFirst?.workers);
    }
    for (const [name, edit] of Object.entries(sample.edits || {}))
      for (const metric of ['visibleMs', 'colouredMs'])
        if (metric !== 'colouredMs' || sample.variant !== 'D')
          add(sample, 'edit.' + name + '.' + (metric === 'colouredMs' ? 'highlightSettledMs' : metric), edit[metric]);
    if (!sequenceComplete && !sample.memoryAfterReturns) continue;
    const memory = sample.memoryAfterReturns || sample.processMemory;
    add(sample, 'memory.peakServerRSS', memory?.peakServerRSS);
    add(sample, 'memory.peakBrowserRSS', memory?.peakBrowserRSS);
    add(sample, 'memory.settledServerRSS', memory?.settled?.serverRSS);
    add(sample, 'memory.settledBrowserRSS', memory?.settled?.browserRSS);
    if (typeof memory?.settled?.serverRSS === 'number' && typeof memory?.settled?.browserRSS === 'number')
      add(sample, 'memory.settledCombinedRSS', memory.settled.serverRSS + memory.settled.browserRSS);
    const cpu = new Map();
    for (const moment of memory?.samples || [{ processes: memory?.cpuObservations || [] }])
      for (const process of moment.processes) {
        const parts = process.cpu.split(':').map(Number);
        let seconds = 0;
        for (const part of parts) seconds = seconds * 60 + part;
        cpu.set(process.pid, Math.max(cpu.get(process.pid) || 0, seconds));
      }
    if (cpu.size)
      add(
        sample,
        'cpu.ownedProcessesSeconds',
        [...cpu.values()].reduce((a, b) => a + b, 0),
      );
    add(
      sample,
      'memory.workerHeap',
      sample.workerHeaps?.reduce((n, worker) => n + worker.usedSize, 0),
    );
    add(sample, 'cache.compilerBytes', sample.cacheBytes?.compiler);
    add(sample, 'cache.indexBytes', sample.cacheBytes?.index);
  }
  const metrics = [...groups.entries()].map(([key, values]) => {
    const sorted = values.slice().sort((a, b) => a - b);
    const rank = (p) => sorted[Math.ceil(sorted.length * p) - 1];
    const [variant, shape, cache, metric] = key.split('|');
    return {
      variant,
      shape,
      cache,
      metric,
      n: sorted.length,
      p50: rank(0.5),
      p95: rank(0.95),
      min: sorted[0],
      max: sorted.at(-1),
    };
  });
  return {
    mode: report.mode,
    planSha256: report.planSha256,
    started: report.started,
    finished: report.finished,
    incomplete: report.incomplete,
    interruption: report.interruption,
    hostQualification: report.hostQualification,
    samples: report.samples.length,
    failures: report.failures.length,
    cleanupFailures: report.cleanupFailures,
    metrics,
    correctness,
    qualification:
      report.mode === 'smoke'
        ? 'Smokes only; no performance verdict.'
        : 'Planned n=20; inspect actual n per cell. Incomplete sets and correctness failures cannot establish a performance verdict; nearest-rank tails remain exploratory.',
  };
}
if (require.main === module) {
  const file = path.resolve(process.argv[2]);
  const summary = summarize(JSON.parse(fs.readFileSync(file)));
  const out = path.join(path.dirname(file), 'summary.json');
  fs.writeFileSync(out, JSON.stringify(summary, null, 2));
  console.log(
    JSON.stringify({
      out,
      samples: summary.samples,
      failures: summary.failures,
      metrics: summary.metrics.length,
    }),
  );
}
module.exports = { summarize };
