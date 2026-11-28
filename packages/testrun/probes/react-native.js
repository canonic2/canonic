// Optional app-owned instrumentation. Import/install explicitly; no React dependency.
'use strict';
const KEY = '__TESTRUN_DIAGNOSTICS_V1__';
const MAX_EVENTS = 64;
function text(value, max = 256) {
  if (typeof value !== 'string' || !value || value.length > max || /[\x00-\x1f\x7f]/.test(value)) {
    throw new TypeError('Expected bounded diagnostic text');
  }
  return value;
}
function fields(value = {}) {
  if (!value || typeof value !== 'object' || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) {
    throw new TypeError('Diagnostic fields must be a plain object');
  }
  const entries = Object.entries(Object.getOwnPropertyDescriptors(value));
  if (entries.length > 8) throw new RangeError('At most eight diagnostic fields');
  const copy = Object.create(null);
  for (const [key, descriptor] of entries) {
    text(key, 64);
    if (!('value' in descriptor)) throw new TypeError('Diagnostic getters are not read');
    const v = descriptor.value;
    if (v !== null && typeof v !== 'boolean' && !(typeof v === 'number' && Number.isFinite(v)) && !(typeof v === 'string' && v.length <= 128)) {
      throw new TypeError('Diagnostic fields must be bounded scalar values');
    }
    copy[key] = v;
  }
  return copy;
}
function installDiagnosticProbe(metadata, host = globalThis) {
  if (Object.getOwnPropertyDescriptor(host, KEY)) throw new Error('A diagnostic probe already owns this global');
  const allowed = ['appId', 'targetId', 'buildId', 'bundleHash', 'sourceMapHash', 'updateId', 'runtimeVersion', 'reactNativeVersion', 'hermesVersion', 'expoSdkVersion', 'projectId', 'buildMode', 'clientMode'];
  const descriptors = Object.getOwnPropertyDescriptors(metadata);
  const copied = Object.create(null);
  for (const [key, descriptor] of Object.entries(descriptors)) {
    if (!allowed.includes(key) || !('value' in descriptor)) throw new TypeError('Unknown metadata or getter');
    copied[key] = text(descriptor.value);
  }
  text(copied.appId); text(copied.targetId);
  // An installation epoch, not a cryptographic identity or a native process birth.
  copied.runtimeGeneration = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  const info = Object.freeze(copied);
  const events = [];
  let sequence = 0, dropped = 0;
  const now = () => host.performance && typeof host.performance.now === 'function' ? host.performance.now() : null;
  function record(kind, name, actionId, details) {
    const event = {kind, name: text(name, 128), actionId: actionId == null ? null : text(actionId, 128), timestamp: now(), fields: fields(details)};
    event.sequence = ++sequence;
    if (events.length === MAX_EVENTS) { events.shift(); dropped++; }
    events.push(event);
    return event.sequence;
  }
  const api = Object.freeze({
    mark(name, actionId, details) { return record('marker', name, actionId, details); },
    measure(name, start, end, actionId) {
      if (![start, end].every(v => typeof v === 'number' && Number.isFinite(v)) || end < start) throw new TypeError('Use an ordered same-clock interval');
      return record('measure', name, actionId, {start, end, durationMs: end - start});
    },
    onRender(id, phase, actualDuration, baseDuration, startTime, commitTime) {
      if (!['mount', 'update', 'nested-update'].includes(phase) || ![actualDuration, baseDuration, startTime, commitTime].every(v => typeof v === 'number' && Number.isFinite(v) && v >= 0)) throw new TypeError('Invalid React Profiler commit');
      return record('react_commit', id, null, {phase, actualDuration, baseDuration, startTime, commitTime});
    },
    snapshot() {
      return {schemaVersion: 1, metadata: {...info}, clock: 'producer performance.now milliseconds; null if absent', sequence, dropped, events: events.map(e => ({...e, fields: {...e.fields}})), completeness: false};
    },
    uninstall() {
      if (Object.getOwnPropertyDescriptor(host, KEY)?.value !== api) return false;
      delete host[KEY]; events.length = 0; return true;
    },
  });
  Object.defineProperty(host, KEY, {value: api, configurable: true, enumerable: false, writable: false});
  return api;
}
module.exports = {installDiagnosticProbe};
