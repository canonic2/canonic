// Diagnostic-only additive listeners. Pinned driver bytes, arguments and child stdio are unchanged.
const cp = require('node:child_process');
const fs = require('node:fs');
const destination = process.env.TESTRUN_CHILD_OBSERVATIONS;
const started = performance.now();
const children = [];
let records = 0;
function record(event, fields = {}) {
  if (++records > 512) return;
  fs.appendFileSync(destination, JSON.stringify({event,elapsedMs:performance.now()-started,...fields})+'\n', {mode:0o600});
}
function snapshot() {
  return children.map(child => ({pid:child.pid,exitCode:child.exitCode,signalCode:child.signalCode,killed:child.killed,
    streams:child.stdio.map((stream,index) => stream ? {index,readable:stream.readable,writable:stream.writable,destroyed:stream.destroyed,closed:stream.closed,readableEnded:stream.readableEnded,writableEnded:stream.writableEnded,readableLength:stream.readableLength,writableLength:stream.writableLength} : null)}));
}
const spawn = cp.spawn;
cp.spawn = function(...args) {
  const child = Reflect.apply(spawn,this,args);
  if (children.length >= 32) throw Error('Acme diagnostic child bound');
  children.push(child);
  record('spawn',{pid:child.pid});
  child.once('exit',(code,signal) => record('exit',{pid:child.pid,code,signal,children:snapshot()}));
  child.once('close',(code,signal) => record('close',{pid:child.pid,code,signal,children:snapshot()}));
  child.stdio.forEach((stream,index) => {
    if (!stream) return;
    stream.once('end',() => record('stream-end',{pid:child.pid,index}));
    stream.once('finish',() => record('stream-finish',{pid:child.pid,index}));
    stream.once('close',() => record('stream-close',{pid:child.pid,index}));
  });
  return child;
};
setInterval(() => record('snapshot',{children:snapshot(),handles:process._getActiveHandles().slice(0,64).map(handle => ({type:handle.constructor?.name,fd:handle.fd,hasRef:typeof handle.hasRef === 'function' ? handle.hasRef() : undefined}))}),500).unref();
process.once('exit',code => record('driver-exit',{code,children:snapshot()}));
