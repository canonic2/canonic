/* Implementations, through Workbench
   ----------------------------------
   A lens frames a page another server owns — a Storybook, a dev server. The
   workbench can't read into another origin's frame, and a team shouldn't have
   to install anything in its own pages so that it can. Instead each
   implementation origin gets a loopback proxy on its own port: every request
   and WebSocket passes through to the implementation unchanged, and HTML
   documents gain one script, the preview bridge, which streams the live DOM
   back to the workbench for capture.

   A port of its own, not a path under the workbench's: dev servers ask for
   absolute paths (/@vite/client, /sb-addons/…) that a prefix would break.

   Headers that would stop the page loading inside the workbench frame are
   dropped, and cookies are rewritten to belong to the proxy's origin.

   Lenses must not go back to framing an implementation's own address, or to
   serving it from the workbench's origin: specs/implementation-proxy.md
   records why, and the alternatives that were ruled out. */

var http = require('node:http');
var https = require('node:https');
var net = require('node:net');
var tls = require('node:tls');

var MAX_DOCUMENT = 32 * 1024 * 1024;
var HOP = ['connection', 'keep-alive', 'proxy-connection', 'transfer-encoding', 'upgrade'];

/* The bridge goes first in <head> so it is listening before the page's own
   scripts run. A document with no <head> or <html> gets it at the top. */
function inject(html, tag) {
  var at = /<head(\s[^>]*)?>/i.exec(html) || /<html(\s[^>]*)?>/i.exec(html);
  if (!at) return tag + html;
  var end = at.index + at[0].length;
  return html.slice(0, end) + tag + html.slice(end);
}

/* The page now lives at the proxy's origin over plain http. Domain and
   Secure would keep the browser from storing the cookie there at all. */
function cookie(value) {
  return String(value).split(';').filter(function (part, index) {
    return index === 0 || !/^\s*(domain|secure|samesite)\b/i.test(part);
  }).join(';');
}

/* Only a navigation gets the bridge: a fetch or XHR that happens to answer
   with HTML is data, not a document. */
function isDocument(req, headers) {
  if (!/^text\/html\b/i.test(String(headers['content-type'] || ''))) return false;
  if (req.method === 'HEAD') return false;
  var dest = req.headers['sec-fetch-dest'];
  return !dest || dest === 'document' || dest === 'iframe' || dest === 'frame';
}

function swapOrigin(value, from, to) {
  if (typeof value !== 'string' || value.indexOf(from) !== 0) return value;
  return to + value.slice(from.length);
}

/* `options.bridge()` answers the bridge script's address; it is asked per
   document because the workbench learns its port only once it listens. */
function create(options) {
  options = options || {};
  var proxies = new Map();
  var opened = new Map();
  var closed = false;

  function open(targetOrigin) {
    var target = new URL(targetOrigin);
    var secure = target.protocol === 'https:';
    var client = secure ? https : http;
    var targetPort = Number(target.port) || (secure ? 443 : 80);
    var sockets = new Set();
    var self = { target: targetOrigin, origin: null, server: null };

    function upstreamHeaders(headers) {
      var out = Object.assign({}, headers);
      HOP.forEach(function (name) { delete out[name]; });
      out.host = target.host;
      if (out.origin) out.origin = swapOrigin(out.origin, self.origin, targetOrigin);
      if (out.referer) out.referer = swapOrigin(out.referer, self.origin, targetOrigin);
      /* Documents are rewritten, so ask for them uncompressed. */
      delete out['accept-encoding'];
      return out;
    }

    function downstreamHeaders(headers) {
      var out = Object.assign({}, headers);
      HOP.forEach(function (name) { delete out[name]; });
      delete out['content-security-policy'];
      delete out['content-security-policy-report-only'];
      delete out['x-frame-options'];
      delete out['cross-origin-opener-policy'];
      delete out['cross-origin-embedder-policy'];
      delete out['cross-origin-resource-policy'];
      if (out.location) out.location = swapOrigin(out.location, targetOrigin, self.origin);
      if (out['set-cookie']) out['set-cookie'] = [].concat(out['set-cookie']).map(cookie);
      return out;
    }

    function failed(res, error) {
      if (res.headersSent) { res.destroy(); return; }
      res.writeHead(502, { 'Content-Type': 'text/plain; charset=utf-8' });
      res.end(targetOrigin + ' isn’t answering — is it running? (' + String(error.message || error) + ')');
    }

    var server = http.createServer(function (req, res) {
      var outgoing = client.request({
        protocol: target.protocol,
        hostname: target.hostname,
        port: targetPort,
        method: req.method,
        path: req.url,
        headers: upstreamHeaders(req.headers),
        /* No pooled sockets: closing the proxy leaves nothing behind. */
        agent: false,
        rejectUnauthorized: false,
      }, function (incoming) {
        var headers = downstreamHeaders(incoming.headers);
        if (!isDocument(req, incoming.headers)) {
          res.writeHead(incoming.statusCode, incoming.statusMessage, headers);
          incoming.pipe(res);
          return;
        }
        var chunks = [];
        var size = 0;
        incoming.on('data', function (chunk) {
          size += chunk.length;
          if (size > MAX_DOCUMENT) { incoming.destroy(new Error('document too large to rewrite')); return; }
          chunks.push(chunk);
        });
        incoming.on('error', function (error) { failed(res, error); });
        incoming.on('end', function () {
          var tag = '<script src="' + options.bridge() + '"></script>';
          var body = Buffer.from(inject(Buffer.concat(chunks).toString('utf8'), tag), 'utf8');
          /* The rewritten document names this session's workbench port; a
             cached copy from another session would name a stale one. */
          delete headers.etag;
          delete headers['last-modified'];
          delete headers['content-encoding'];
          headers['content-length'] = body.length;
          headers['cache-control'] = 'no-store';
          res.writeHead(incoming.statusCode, incoming.statusMessage, headers);
          res.end(body);
        });
      });
      outgoing.on('error', function (error) { failed(res, error); });
      res.on('close', function () { if (!res.writableFinished) outgoing.destroy(); });
      req.pipe(outgoing);
    });

    /* Dev servers push reloads and their own channels over WebSockets. Pass
       the upgrade through as raw bytes once the request line is rewritten. */
    server.on('upgrade', function (req, socket, head) {
      var upstream = secure
        ? tls.connect({ host: target.hostname, port: targetPort, servername: target.hostname, rejectUnauthorized: false })
        : net.connect({ host: target.hostname, port: targetPort });
      function end() { socket.destroy(); upstream.destroy(); }
      upstream.on('error', end);
      socket.on('error', end);
      upstream.on('close', end);
      socket.on('close', end);
      upstream.once(secure ? 'secureConnect' : 'connect', function () {
        var headers = Object.assign({}, req.headers, { host: target.host });
        if (headers.origin) headers.origin = swapOrigin(headers.origin, self.origin, targetOrigin);
        var lines = [req.method + ' ' + req.url + ' HTTP/1.1'];
        Object.keys(headers).forEach(function (name) {
          [].concat(headers[name]).forEach(function (value) { lines.push(name + ': ' + value); });
        });
        upstream.write(lines.join('\r\n') + '\r\n\r\n');
        if (head && head.length) upstream.write(head);
        upstream.pipe(socket);
        socket.pipe(upstream);
      });
    });

    server.on('connection', function (socket) {
      sockets.add(socket);
      socket.on('close', function () { sockets.delete(socket); });
    });

    self.server = server;
    self.close = function () {
      return new Promise(function (resolve) {
        server.close(function () { resolve(); });
        sockets.forEach(function (socket) { socket.destroy(); });
      });
    };
    return new Promise(function (resolve, reject) {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', function () {
        server.removeListener('error', reject);
        self.origin = 'http://127.0.0.1:' + server.address().port;
        resolve(self);
      });
    });
  }

  function proxyFor(address) {
    if (closed) return Promise.reject(new Error('Implementation proxies are closed'));
    var origin = new URL(address).origin;
    if (!proxies.has(origin)) {
      var opening = open(origin);
      proxies.set(origin, opening);
      opening.then(function (proxy) { opened.set(origin, proxy); },
        function () { if (proxies.get(origin) === opening) proxies.delete(origin); });
    }
    return proxies.get(origin);
  }

  return {
    /* The same address, at its proxy. */
    address: function (address) {
      return proxyFor(address).then(function (proxy) {
        return swapOrigin(address, proxy.target, proxy.origin);
      });
    },
    /* The proxy origins open so far, for checking a capture's address. */
    origins: function () {
      var out = [];
      opened.forEach(function (proxy) { out.push(proxy.origin); });
      return out;
    },
    close: function () {
      closed = true;
      var all = [];
      proxies.forEach(function (opening) {
        all.push(opening.then(function (proxy) { return proxy.close(); }, function () {}));
      });
      proxies.clear();
      opened.clear();
      return Promise.all(all);
    },
  };
}

module.exports = { create: create, inject: inject, cookie: cookie };
