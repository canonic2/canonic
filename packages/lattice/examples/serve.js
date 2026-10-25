/** @file Local-only static host for runnable examples and unchanged runtime source. */
import http from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
/**
 * Start a loopback example host; callers own closing the returned server.
 * @param {number} [port] Port; zero requests an ephemeral port.
 * @returns {Promise<http.Server>} Listening server.
 */
export function serveExamples(port = 4177) {
  const server = http.createServer(async (request, response) => {
    try {
      if (request.method !== 'GET' && request.method !== 'HEAD') { response.writeHead(405).end(); return; }
      let name = decodeURIComponent(new URL(request.url ?? '/', 'http://example.com').pathname);
      if (name === '/') name = '/examples/tasks/';
      if (name.endsWith('/')) name += 'index.html';
      const file = path.resolve(root, `.${name}`);
      if (!file.startsWith(root + path.sep) || (!name.startsWith('/examples/') && !name.startsWith('/src/'))) { response.writeHead(404).end(); return; }
      const body = await readFile(file), type = name.endsWith('.html') ? 'text/html' : name.endsWith('.js') ? 'text/javascript' : 'text/plain';
      response.writeHead(200, { 'Content-Type': `${type}; charset=utf-8`, 'Cache-Control': 'no-store' });
      response.end(request.method === 'HEAD' ? undefined : body);
    } catch { response.writeHead(404).end(); }
  });
  return new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', () => resolve(server)); });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const server = await serveExamples();
  console.log('Lattice examples: http://127.0.0.1:4177/examples/tasks/');
  process.once('SIGINT', () => server.close());
  process.once('SIGTERM', () => server.close());
}
