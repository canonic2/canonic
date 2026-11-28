// Project a retained result without invoking the original developer callback.
(value, frame, references, codec) => {
  const annotations = [], elements = new Map(), seen = new Map();
  let nodes = 0;
  const visit = (value, path, depth) => {
    if (++nodes > 4096 || depth > 32) throw Error('canonic:VALUE_BOUNDS');
    if (value instanceof HTMLElement) {
      if (value.ownerDocument !== document || !value.isConnected)
        throw Error('canonic:DEVELOPER_TOOL_ELEMENT_SCOPE');
      if (!elements.has(value)) {
        if (annotations.length >= 32) throw Error('canonic:VALUE_BOUNDS');
        const index = annotations.length;
        const locator = references.retain(value, frame);
        elements.set(value, index);
        annotations.push({index, path, locator});
      }
      return {kind:'element_annotation', index:elements.get(value)};
    }
    if (!value || typeof value !== 'object') return value;
    if (seen.has(value)) return seen.get(value);
    if (!Array.isArray(value) && Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null)
      return value;
    const result = Array.isArray(value) ? [] : Object.create(null);
    seen.set(value, result);
    const copy = key => {
      if (key.length > 1024) throw Error('canonic:VALUE_BOUNDS');
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      const projected = descriptor && 'value' in descriptor
        ? visit(descriptor.value, [...path,key], depth+1)
        : {kind:'accessor', get:!!descriptor?.get, set:!!descriptor?.set};
      Object.defineProperty(result,key,{value:projected,enumerable:true,writable:true,configurable:true});
    };
    if (Array.isArray(value)) {
      if (value.length > 4096) throw Error('canonic:VALUE_BOUNDS');
      for (let i=0;i<value.length;i++) {
        if (Object.prototype.hasOwnProperty.call(value,i)) copy(String(i));
        else { if (++nodes>4096) throw Error('canonic:VALUE_BOUNDS'); result.push(undefined); }
      }
    } else {
      let keys=0;
      for (const key in value) if (Object.prototype.hasOwnProperty.call(value,key)) {
        if (++keys>4096) throw Error('canonic:VALUE_BOUNDS');
        copy(key);
      }
    }
    return result;
  };
  const projected = codec.encode(visit(value,[],0),32768);
  const text=JSON.stringify({value:projected,annotations,complete:true,projection:'own-enumerable-data-properties',symbolsIncluded:false});
  if (new TextEncoder().encode(text).length > 60000) throw Error('canonic:VALUE_BOUNDS');
  return text;
}
