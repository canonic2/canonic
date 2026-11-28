// Own only a synthetic browser server and a TLS relay for its exact endpoint.
const assert = require('node:assert/strict');
const {spawn} = require('node:child_process');
const {once} = require('node:events');
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const http = require('node:http');
const net = require('node:net');
const path = require('node:path');
const readline = require('node:readline');
const tls = require('node:tls');
const {setTimeout: delay} = require('node:timers/promises');

const [root, packagePath, application, engine, mode, certificates] = process.argv.slice(2);
const playwright = require(packagePath);
const {wsServer: WebSocketServer} = require(path.join(packagePath, 'lib/utilsBundle.js'));
const report = object => process.stdout.write(JSON.stringify(object) + '\n');
const marker = 'Acme borrowed browser';

async function chrome() {
  const profile = path.join(root, 'chrome');
  fs.mkdirSync(profile, {mode: 0o700});
  const child = spawn(application, [
    `--user-data-dir=${profile}`, '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
    '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
    '--use-mock-keychain', '--password-store=basic',
    ...(mode === 'headless' ? ['--headless=new'] : []), 'about:blank',
  ], {cwd: root, stdio: 'ignore'});
  const exited = once(child, 'exit');
  let socket;
  let id = 0;
  async function command(method, params = {}, sessionId) {
    const request = ++id;
    const received = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        socket.removeEventListener('message', reply);
        reject(Error(`CDP fixture timeout for ${method}`));
      }, 15000);
      function reply(event) {
        const value = JSON.parse(event.data);
        if (value.id !== request) return;
        clearTimeout(timer);
        socket.removeEventListener('message', reply);
        if (value.error) reject(Error(value.error.message));
        else resolve(value.result);
      }
      socket.addEventListener('message', reply);
      socket.send(JSON.stringify({id: request, method, params, sessionId}));
    });
    return received;
  }
  const stop = async () => {
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({id: ++id, method: 'Browser.close'}));
    } else if (child.exitCode === null) {
      child.kill('SIGTERM');
    }
    const force = setTimeout(() => child.kill('SIGKILL'), 10000);
    try { await exited; } finally { clearTimeout(force); socket?.close(); }
  };
  try {
    const deadline = Date.now() + 30000;
    let endpoint;
    while (!endpoint && Date.now() < deadline && child.exitCode === null) {
      try {
        const [port, wsPath] = (await fsp.readFile(path.join(profile, 'DevToolsActivePort'), 'utf8')).trim().split(/\r?\n/);
        if (port && wsPath) endpoint = `ws://127.0.0.1:${port}${wsPath}`;
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
      }
      if (!endpoint) await delay(25);
    }
    assert(endpoint, 'Owned Chrome did not publish its endpoint');
    socket = new WebSocket(endpoint);
    await new Promise((resolve, reject) => {
      socket.addEventListener('open', resolve, {once: true});
      socket.addEventListener('error', reject, {once: true});
    });
    const {browserContextId} = await command('Target.createBrowserContext');
    const foreign = await command('Target.createBrowserContext');
    const {targetId} = await command('Target.createTarget', {
      browserContextId, url: `data:text/html,<title>${marker}</title>`,
    });
    const markerDeadline = Date.now() + 5000;
    for (;;) {
      const {targetInfo} = await command('Target.getTargetInfo', {targetId});
      if (targetInfo.title === marker) break;
      assert(Date.now() < markerDeadline, 'Synthetic adopted tab did not finish navigation');
      await delay(25);
    }
    const foreignPage = await command('Target.createTarget', {browserContextId: foreign.browserContextId, url: 'about:blank'});
    const baseline = await command('Target.getBrowserContexts');
    const {targetInfos} = await command('Target.getTargets');
    const defaultContextId = baseline.defaultBrowserContextId ?? targetInfos.find(target =>
      target.type === 'page' && target.browserContextId && !baseline.browserContextIds.includes(target.browserContextId)
    )?.browserContextId;
    assert.equal(typeof defaultContextId, 'string', 'Fixture needs the exact default context identity');
    return {endpoint, contextId: browserContextId, defaultContextId, stop, async check() {
      const expected = baseline.browserContextIds.sort();
      const deadline = Date.now() + 5000;
      let contexts;
      do {
        contexts = (await command('Target.getBrowserContexts')).browserContextIds.sort();
        if (JSON.stringify(contexts) === JSON.stringify(expected)) break;
        await delay(25);
      } while (Date.now() < deadline);
      assert.deepEqual(contexts, expected);
      const {targetInfos} = await command('Target.getTargets');
      assert(targetInfos.some(target => target.targetId === targetId && target.title === marker));
      assert(targetInfos.some(target => target.targetId === foreignPage.targetId));
    }};
  } catch (error) {
    await stop();
    throw error;
  }
}

async function play() {
  const type = playwright[engine];
  const options = {host: '127.0.0.1', headless: mode === 'headless', timeout: 30000};
  if (process.platform === 'win32' && engine === 'firefox' && options.headless) {
    options.firefoxUserPrefs = {
      'gfx.webrender.software': true,
      'gfx.webrender.software.d3d11': false,
      'gfx.webrender.compositor': false,
      'layers.gpu-process.enabled': false,
    };
  }
  const server = await type.launchServer(options);
  let browser;
  try {
    browser = await type.connect(server.wsEndpoint(), {timeout: 15000});
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.setContent(`<title>${marker}</title>`);
    return {endpoint: server.wsEndpoint(), async check() {
      assert.equal(await page.title(), marker);
      assert.equal(context.pages().length, 1);
    }, async stop() {
      try { await context.close(); } finally { await server.close(); }
    }};
  } catch (error) {
    await server.close();
    throw error;
  }
}

async function relay(endpoint) {
  const upstream = new URL(endpoint);
  const clients = new Set();
  const server = tls.createServer({
    cert: fs.readFileSync(path.join(certificates, 'server.pem')),
    key: fs.readFileSync(path.join(certificates, 'server-key.pem')),
  }, client => {
    clients.add(client);
    let socket;
    let header = Buffer.alloc(0);
    const release = () => { clients.delete(client); client.destroy(); socket?.destroy(); };
    client.on('close', release);
    client.on('error', release);
    client.setTimeout(15000, release);
    function upgrade(chunk) {
      header = Buffer.concat([header, chunk]);
      if (header.length > 8192) return release();
      if (header.indexOf('\r\n\r\n') < 0) return;
      client.removeListener('data', upgrade);
      client.setTimeout(0);
      if (!/\r\nAuthorization: Bearer acme-session-token\r\n/i.test(header.toString('ascii'))) {
        client.end('HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n');
        return;
      }
      socket = net.connect(Number(upstream.port), '127.0.0.1');
      socket.on('close', release);
      socket.on('error', release);
      socket.write(header);
      client.pipe(socket).pipe(client);
    }
    client.on('data', upgrade);
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return {endpoint: `wss://localhost:${server.address().port}${upstream.pathname}`, stop() {
    for (const client of clients) client.destroy();
    return new Promise(resolve => server.close(resolve));
  }};
}

async function ownedPageWitness() {
  const socketStates = new Map();
  const server = http.createServer((request, response) => {
    response.writeHead(200, {'Content-Type': 'text/html'});
    response.end('<!doctype html><title>Acme attachment ownership</title>');
  });
  const sockets = new WebSocketServer({server});
  sockets.on('connection', (socket, request) => {
    const name = request.url.slice(1);
    if (!/^(?:(?:cli|mcp)-)?(?:loopback|tls)$/.test(name) || socketStates.has(name)) {
      socket.close(1008);
      return;
    }
    socketStates.set(name, 'open');
    socket.on('error', () => {});
    socket.on('close', () => socketStates.set(name, 'closed'));
    socket.send('Acme witness open');
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    async check(name) {
      assert(socketStates.has(name), `Owned page never opened ${name} witness`);
      const deadline = Date.now() + 5000;
      while (socketStates.get(name) === 'open' && Date.now() < deadline) await delay(25);
      assert.equal(socketStates.get(name), 'closed', `Owned page socket survived disconnect: ${name}`);
    },
    async stop() {
      for (const peer of sockets.clients) peer.terminate();
      await new Promise(resolve => sockets.close(resolve));
      await new Promise(resolve => server.close(resolve));
    },
  };
}

(async () => {
  const input = readline.createInterface({input: process.stdin});
  const donor = await (engine === 'chrome' ? chrome() : play());
  let proxy;
  let witness;
  try {
    proxy = await relay(donor.endpoint);
    witness = await ownedPageWitness();
    await donor.check();
    report({
      endpoint: donor.endpoint, tlsEndpoint: proxy.endpoint, witnessOrigin: witness.origin,
      contextId: donor.contextId, defaultContextId: donor.defaultContextId,
    });
    for await (const line of input) {
      if (line === 'stop') break;
      const [command, name] = line.split(' ');
      assert.equal(command, 'check');
      if (name) await witness.check(name);
      await donor.check();
      report({intact: true, ...(name ? {ownedPageReleased: name} : {})});
    }
  } finally {
    input.close();
    try { await witness?.stop(); }
    finally {
      try { await proxy?.stop(); } finally { await donor.stop(); }
    }
  }
})().then(() => process.exit(0)).catch(error => { console.error(error); process.exit(1); });
