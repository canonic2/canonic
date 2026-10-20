/* The preview compiler of one fixture project, for tests: a Workbench server
   started through the host with previews on, asked over HTTP for portable
   builds, and the binary's `list` and `build` commands. A fixture without a
   workbench.yaml gets one naming its preview settings. */
const cp = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const host = require('./src/modules/host/index.ts');

const product = path.resolve(__dirname, '../..');

function json(port, route, timeout) {
  return new Promise((resolve, reject) => {
    const request = require('node:http').get({ host: '127.0.0.1', port, path: route, agent: false, timeout: timeout || 300000 }, response => {
      const chunks = [];
      response.on('data', chunk => chunks.push(chunk));
      response.on('end', () => {
        let body;
        try { body = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch (error) { reject(error); return; }
        if (response.statusCode !== 200) reject(new Error(body && body.error || 'The preview compiler answered ' + response.statusCode));
        else resolve(body);
      });
    });
    request.on('timeout', () => request.destroy(new Error('The preview compiler did not answer.')));
    request.on('error', reject);
  });
}

/** A portable build as tests read it: outputs by name, local files absolute. */
function built(root, answer) {
  return { ...answer, outputs: new Map(answer.outputs.map(file => [file.path, Buffer.from(file.data, 'base64')])),
    ...(answer.localFiles ? { localFiles: answer.localFiles.map(file => path.join(root, file)) } : {}) };
}

function manifest(root, options) {
  const file = path.join(root, 'workbench.yaml');
  if (!fs.existsSync(file)) fs.writeFileSync(file, 'name: Acme\n' + (options ? 'previews: ' + JSON.stringify(options) + '\n' : ''));
}

/** Starts the compiler for `root` and lists its pages; `space` names a
    space of an existing workbench.yaml instead of the fixture's own. */
async function open(root, options, space) {
  if (!space) manifest(root, options);
  const running = await host.start({ root, config: space, isTrusted: true, capture: { close() {} } });
  try {
    await json(running.port, '/_workbench/config');
  } catch (error) { await running.close(); throw error; }
  const base = '/_workbench/previews';
  return {
    port: running.port,
    config: running.config,
    async compile(file) {
      return built(root, await json(running.port, base + '/portable/preview?file=' + encodeURIComponent(file)));
    },
    async compileDocs(request) {
      return built(root, await json(running.port, base + '/portable/docs?request=' + encodeURIComponent(JSON.stringify(request))));
    },
    close: running.close,
  };
}

/** What the page index finds in `root`: `{ previews, docs, errors }`. */
function list(root) {
  return JSON.parse(cp.execFileSync(host.binaryPath(), ['list', root, '--node', process.execPath, '--product', product], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }));
}

/** The portable browser build of `root`, written to `destination`. */
function build(root, destination) {
  return cp.spawnSync(host.binaryPath(), ['build', root, destination, '--node', process.execPath, '--product', product], { encoding: 'utf8' });
}

module.exports = { open, list, build, built };
