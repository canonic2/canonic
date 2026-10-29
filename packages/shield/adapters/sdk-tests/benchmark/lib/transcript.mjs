// What a host delivered to its model and what the model called, read from the host's own transcript.

export const strings = (value, out = []) => {
  if (typeof value === 'string') out.push(value);
  else if (Array.isArray(value)) for (const item of value) strings(item, out);
  else if (value && typeof value === 'object') for (const item of Object.values(value)) strings(item, out);
  return out;
};

/** The balanced argument text after each `tools.<name>(` in code-mode JavaScript. */
export function codeModeCalls(source) {
  const calls = [];
  for (const match of source.matchAll(/tools\.([A-Za-z0-9_]+)\(/g)) {
    let depth = 1;
    let quote = null;
    let end = match.index + match[0].length;
    for (; end < source.length && depth; end++) {
      const char = source[end];
      if (quote) { if (char === '\\') end++; else if (char === quote) quote = null; }
      else if (char === '"' || char === "'" || char === '`') quote = char;
      else if (char === '(') depth++;
      else if (char === ')') depth--;
    }
    const text = source.slice(match.index + match[0].length, end - 1).trim();
    let input = { source: text };
    try {
      // Object literals here are JSON apart from unquoted keys.
      input = JSON.parse(text.replace(/([{,]\s*)([A-Za-z_][A-Za-z0-9_]*)\s*:/g, '$1"$2":'));
    } catch {}
    calls.push({ name: match[1], input });
  }
  return calls;
}

export function modelView(host, lines) {
  const visible = [];
  const calls = [];
  lines.forEach((line, index) => {
    if (host === 'claude') {
      if (line.type === 'assistant') {
        for (const block of Array.isArray(line.message?.content) ? line.message.content : []) {
          if (block.type === 'tool_use') calls.push({ index, name: block.name, input: block.input });
        }
      } else if (line.type === 'user' || line.type === 'attachment' || line.type === 'system') {
        visible.push({ index, kind: line.type, text: strings(line.message?.content ?? line.attachment ?? line.content).join('\n') });
      }
    } else if (line.type === 'response_item') {
      const payload = line.payload ?? {};
      if (payload.type === 'function_call' || payload.type === 'custom_tool_call') {
        let input = payload.arguments ?? payload.input;
        try { input = JSON.parse(input); } catch {}
        // Code mode runs tools from JavaScript; each inner call is its own model request,
        // and the outer script only contains them.
        const inner = typeof input === 'string' ? codeModeCalls(input) : [];
        calls.push({ index, name: [payload.namespace, payload.name].filter(Boolean).join('.'), input, container: inner.length > 0 });
        for (const call of inner) calls.push({ index, ...call });
      } else if (payload.type !== 'reasoning' && !(payload.type === 'message' && payload.role === 'assistant')) {
        visible.push({ index, kind: payload.type, text: strings(payload.output ?? payload.content).join('\n') });
      }
    }
  });
  return { visible, calls };
}
