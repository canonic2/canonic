// A bounded document-local weak registry; references never mutate page attributes.
(() => {
  const key = Symbol.for('canonic.testrun.elements.v1');
  const state = globalThis[key] ||= new Map();
  return {
    queryAll(root, token) {
      const element = state.get(token)?.deref();
      if (!element?.isConnected) throw new Error('canonic:STALE_REFERENCE');
      let node = element;
      while (node && node !== root) node = node.parentNode || node.host;
      return node === root ? [element] : [];
    },
    query(root, token) { return this.queryAll(root, token)[0] || null; },
    retain(element, frame) {
      for (const [token, weak] of state) if (!weak.deref()?.isConnected) state.delete(token);
      while (state.size >= 512) state.delete(state.keys().next().value);
      const bytes=crypto.getRandomValues(new Uint8Array(16));
      bytes[6]=(bytes[6]&15)|64;bytes[8]=(bytes[8]&63)|128;
      const hex=Array.from(bytes,byte=>byte.toString(16).padStart(2,'0')).join('');
      const id=`${hex.slice(0,8)}-${hex.slice(8,12)}-${hex.slice(12,16)}-${hex.slice(16,20)}-${hex.slice(20)}`;
      state.set(`${frame.id}.${frame.generation}.${id}`, new WeakRef(element));
      return {kind:'element', frame, id};
    },
  };
})()
