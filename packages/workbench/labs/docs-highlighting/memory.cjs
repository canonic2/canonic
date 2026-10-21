const cp = require('node:child_process');
const { promisify } = require('node:util');
const execFile = promisify(cp.execFile);
function monitor(host, browser) {
  const samples = [],
    errors = [];
  let busy = false,
    closed = false;
  const read = async () => {
    if (busy || closed) return;
    busy = true;
    try {
      const { stdout } = await execFile('ps', ['-axo', 'pid=,ppid=,rss=,time=,comm=']);
      const rows = stdout
        .trim()
        .split('\n')
        .map((line) => {
          const match = line.trim().match(/^(\d+)\s+(\d+)\s+(\d+)\s+([\d:.]+)\s+(.+)$/);
          return (
            match && {
              pid: +match[1],
              ppid: +match[2],
              rss: +match[3] * 1024,
              cpu: match[4],
              command: match[5],
            }
          );
        })
        .filter(Boolean);
      const owned = (root) => {
        const ids = new Set([root]);
        let changed = true;
        while (changed) {
          changed = false;
          for (const row of rows)
            if (ids.has(row.ppid) && !ids.has(row.pid)) {
              ids.add(row.pid);
              changed = true;
            }
        }
        return rows.filter((row) => ids.has(row.pid));
      };
      const server = owned(host),
        chrome = owned(browser());
      samples.push({
        at: performance.now(),
        serverRSS: server.reduce((n, row) => n + row.rss, 0),
        browserRSS: chrome.reduce((n, row) => n + row.rss, 0),
        processes: [...server, ...chrome].map((row) => ({
          ...row,
          command: row.command.split('/').pop(),
        })),
      });
    } catch (error) {
      errors.push(String(error));
    } finally {
      busy = false;
    }
  };
  const timer = setInterval(() => void read().catch(() => {}), 200);
  void read().catch(() => {});
  const snapshot = async () => {
    while (busy) await new Promise((r) => setTimeout(r, 5));
    await read();
    return {
      errors,
      samples: samples.slice(),
      peakServerRSS: Math.max(0, ...samples.map((s) => s.serverRSS)),
      peakBrowserRSS: Math.max(0, ...samples.map((s) => s.browserRSS)),
      settled: samples.at(-1),
    };
  };
  return {
    snapshot,
    async stop() {
      clearInterval(timer);
      const value = await snapshot();
      closed = true;
      return value;
    },
  };
}
async function watchWorkers(browser, target) {
  browser.__labWorkers = new Map();
  target.on('Target.attachedToTarget', (event) => {
    if (event.targetInfo.type !== 'worker') return;
    const worker = { sessionId: event.sessionId, url: event.targetInfo.url };
    worker.enabled = browser.pipe
      .send('Runtime.enable', {}, event.sessionId, { timeout: 2000 })
      .then(
        () => true,
        (error) => {
          worker.error = String(error);
          return false;
        },
      );
    browser.__labWorkers.set(event.sessionId, worker);
  });
  target.on('Target.detachedFromTarget', (event) => browser.__labWorkers.delete(event.sessionId));
  await target.send('Target.setAutoAttach', {
    autoAttach: true,
    waitForDebuggerOnStart: false,
    flatten: true,
  });
}
async function workerHeaps(browser) {
  const results = [];
  for (const worker of browser.__labWorkers.values()) {
    if (!(await worker.enabled)) {
      results.push({ error: worker.error });
      continue;
    }
    try {
      results.push({
        url: worker.url
          ? new URL(worker.url, 'http://localhost').pathname
          : '(worker URL not yet published)',
        ...(await browser.pipe.send('Runtime.getHeapUsage', {}, worker.sessionId, {
          timeout: 2000,
        })),
      });
    } catch (error) {
      results.push({ error: String(error) });
    }
  }
  return results;
}
module.exports = { monitor, workerHeaps, watchWorkers };
