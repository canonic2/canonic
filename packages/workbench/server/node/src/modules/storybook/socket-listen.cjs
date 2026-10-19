/* Loaded with --require into a Storybook that Workbench runs. Every TCP
   listen in the process goes to a local socket in WORKBENCH_STORYBOOK_SOCKETS
   instead, so the Storybook takes no port and only Workbench reaches it.
   Storybook still sees the port it was given, which is its proxy's, so the
   addresses it writes into its pages lead back to the proxy. The HTTP server
   that answers requests records its socket in the `server` file there. */
'use strict';
const net = require('node:net');
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');

const dir = process.env.WORKBENCH_STORYBOOK_SOCKETS;
const pipe = process.platform === 'win32';
let count = 0;
const listen = net.Server.prototype.listen;

function tcp(first) {
  if (typeof first === 'number') return true;
  if (typeof first === 'string') return /^\d+$/.test(first);
  return !!first && typeof first === 'object' && first.path === undefined && first.fd === undefined && first.handle === undefined;
}

net.Server.prototype.listen = function (...args) {
  const first = args[0];
  if (!dir || !tcp(first)) return listen.apply(this, args);
  const port = Number(typeof first === 'object' ? first.port : first) || 0;
  count += 1;
  const socket = pipe ? '\\\\.\\pipe\\' + path.basename(dir) + '-' + count : path.join(dir, count + '.sock');
  if (!pipe) { try { fs.unlinkSync(socket); } catch { /* not there yet */ } }
  const address = this.address;
  this.address = function () {
    const real = address.call(this);
    return typeof real === 'string' ? { address: '127.0.0.1', family: 'IPv4', port } : real;
  };
  if (this instanceof http.Server) {
    this.once('listening', () => { fs.writeFileSync(path.join(dir, 'server'), socket); });
  }
  return listen.call(this, socket, args.find((arg) => typeof arg === 'function'));
};
