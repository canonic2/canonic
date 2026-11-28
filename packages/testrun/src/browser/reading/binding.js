// Private bridge for explicitly registered page bindings. No host APIs are exposed.
(raw, name, codec) => {
  const key = Symbol.for(raw);
  if (globalThis[key]) return;
  if (Object.prototype.hasOwnProperty.call(globalThis, name)) throw Error('Binding name exists');
  const send = globalThis[raw];
  if (typeof send !== 'function') throw Error('Binding transport unavailable');
  const pending = new Map();
  let active = true, sequence = 0;
  const fn = (...args) => {
    if (!active) return Promise.reject(Error('Testrun binding closed'));
    if (pending.size >= 64) return Promise.reject(Error('Testrun binding capacity'));
    let encoded;
    if (args.length > 64) return Promise.reject(Error('Testrun binding argument bounds'));
    try { encoded = args.map(value => codec.encodeBinding(value)); } catch (error) { return Promise.reject(error); }
    const ticket = ++sequence;
    const payload = JSON.stringify({ticket, args: encoded});
    if (new TextEncoder().encode(payload).length > 1048576) return Promise.reject(Error('Testrun binding argument bounds'));
    return new Promise((resolve, reject) => {
      pending.set(ticket, {resolve, reject});
      try { send(payload); } catch (error) { pending.delete(ticket); reject(error); }
    });
  };
  const owner = {
    settle(ticket, value, reject) {
      const callback = pending.get(ticket);
      if (!callback) return false;
      pending.delete(ticket);
      if (reject === true) callback.reject(Error('Testrun binding rejected or expired'));
      else { try { const decoded = codec.decode(value, []); if (reject === 'value') callback.reject(decoded); else callback.resolve(decoded); } catch (error) { callback.reject(error); } }
      return true;
    },
    close() {
      active = false;
      for (const callback of pending.values()) callback.reject(Error('Testrun binding closed'));
      pending.clear();
      if (globalThis[name] === fn) delete globalThis[name];
      if (globalThis[raw] === send) delete globalThis[raw];
      delete globalThis[key];
    }
  };
  Object.defineProperty(globalThis, key, {value: owner, configurable: true});
  Object.defineProperty(globalThis, name, {value: fn, writable: true, configurable: true});
}
