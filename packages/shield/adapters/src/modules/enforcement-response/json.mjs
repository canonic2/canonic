// Check wire properties that JSON.parse would otherwise discard or leave unbounded.
export function parseJSON(bytes) {
  // Preserve a BOM so JSON.parse rejects it rather than silently stripping it.
  const source = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
  const stack = [];
  for (let i = 0; i < source.length; i++) {
    const char = source[i];
    if (char === '"') {
      const start = i++;
      for (; i < source.length && source[i] !== '"'; i++) {
        if (source[i] === '\\') i++;
      }
      const value = JSON.parse(source.slice(start, i + 1));
      if (!value.isWellFormed()) throw Error('invalid JSON Unicode string');
      let next = i + 1;
      while (next < source.length && ' \t\r\n'.includes(source[next])) next++;
      if (source[next] === ':') {
        const frame = stack.at(-1);
        if (!frame?.keys || frame.keys.has(value)) throw Error('invalid JSON object key');
        frame.keys.add(value);
      }
    } else if (char === '{' || char === '[') {
      if (stack.length === 128) throw Error('JSON nesting limit');
      stack.push({ close: char === '{' ? '}' : ']', keys: char === '{' ? new Set() : null });
    } else if (char === '}' || char === ']') {
      if (stack.pop()?.close !== char) throw Error('invalid JSON container');
    }
  }
  // The scan owns duplicate keys and depth; the native parser owns JSON grammar.
  return JSON.parse(source);
}
