// Owned bounded loopback wire facts shared by independent pinned SDK references.
const http = require('node:http');
const net = require('node:net');
const basic = (username, password) => 'Basic ' + Buffer.from(`${username}:${password}`).toString('base64');
async function listen(server) {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const sockets = new Set();
  server.on('connection', socket => {sockets.add(socket);socket.on('close', () => sockets.delete(socket));});
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    port: server.address().port,
    close: async () => {
      for (const socket of sockets) socket.destroy();
      await new Promise(resolve => server.close(resolve));
    },
  };
}
async function origin() {
  const requests = [];
  const server = http.createServer((request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    const headers = request.headers;
    if (url.pathname !== '/favicon.ico') requests.push({
      path: url.pathname,
      authorization: headers.authorization || '',
      proxyAuthorization: headers['proxy-authorization'] || '',
      custom: headers['x-acme'] || '',
    });
    if (url.pathname === '/auth' && !headers.authorization) {
      response.writeHead(401, {'WWW-Authenticate':'Basic realm="Acme origin"'});
      response.end('Acme origin challenge');
    } else {
      response.writeHead(200, {'Content-Type':'text/html','Connection':'close'});
      response.end('<!doctype html><title>Acme proxy</title><button>Acme</button>');
    }
  });
  return {...await listen(server), requests};
}
async function proxy(port, authorization) {
  const requests = [];
  const server = http.createServer();
  const selected = url => url.hostname === '127.0.0.1' && Number(url.port) === port;
  server.on('request', (request, response) => {
    let url;
    try {url = new URL(request.url);} catch {response.writeHead(400);response.end();return;}
    if (!selected(url)) {response.writeHead(403);response.end();return;}
    const supplied = request.headers['proxy-authorization'] || '';
    if (url.pathname !== '/favicon.ico') requests.push({kind:'request',path:url.pathname,authorization:supplied});
    if (authorization && supplied !== authorization) {
      response.writeHead(407, {'Proxy-Authenticate':'Basic realm="Acme proxy"','Connection':'close'});
      response.end('Acme proxy challenge');
      return;
    }
    const headers = {...request.headers};
    delete headers['proxy-authorization'];
    delete headers['proxy-connection'];
    headers.connection = 'close';
    const forwarded = http.request(url, {method:request.method,headers}, source => {
      response.writeHead(source.statusCode, source.headers);
      source.pipe(response);
    });
    forwarded.on('error', () => {response.writeHead(502);response.end();});
    request.pipe(forwarded);
  });
  server.on('connect', (request, socket, head) => {
    let target;
    try {target = new URL('http://' + request.url);} catch {socket.destroy();return;}
    if (!selected(target)) {socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');return;}
    const supplied = request.headers['proxy-authorization'] || '';
    requests.push({kind:'connect',authorization:supplied});
    if (authorization && supplied !== authorization) {
      socket.end('HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm="Acme proxy"\r\nConnection: close\r\n\r\n');
      return;
    }
    const upstream = net.connect(port, '127.0.0.1', () => {
      socket.write('HTTP/1.1 200 Connection established\r\n\r\n');
      if (head.length) upstream.write(head);
      socket.pipe(upstream);upstream.pipe(socket);
    });
    upstream.on('error', () => socket.destroy());
    socket.on('error', () => upstream.destroy());
    socket.on('close', () => upstream.destroy());
  });
  return {...await listen(server),requests};
}
module.exports = {origin, proxy, basic};
