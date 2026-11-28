// Read-only Rust source witnesses. This is a lexical inventory, never a compiler
// or an evaluator. Preserve source expressions rather than inventing schemas.
function tokens(source) {
  const result = [];
  for (let i = 0; i < source.length;) {
    const start = i;
    if (/\s/.test(source[i])) { i++; continue; }
    if (source.startsWith('//', i)) { const end = source.indexOf('\n', i); i = end < 0 ? source.length : end; continue; }
    if (source.startsWith('/*', i)) {
      let depth = 1; i += 2;
      while (depth && i < source.length) {
        if (source.startsWith('/*', i)) { depth++; i += 2; }
        else if (source.startsWith('*/', i)) { depth--; i += 2; }
        else i++;
      }
      if (depth) throw new Error('Unterminated Rust comment');
      continue;
    }
    const raw = source.slice(i).match(/^(?:br|r)(#*)"/);
    let value;
    if (raw) {
      const end = source.indexOf('"' + raw[1], i + raw[0].length);
      if (end < 0) throw new Error('Unterminated raw Rust string');
      value = source.slice(i + raw[0].length, end); i = end + 1 + raw[1].length;
    } else if (source[i] === '"' || (source[i] === "'" && /^'(?:\\.|[^'\\])'/u.test(source.slice(i)))) {
      const quote = source[i++];
      let closed = false;
      for (; i < source.length; i++) {
        if (source[i] === '\\') i++;
        else if (source[i] === quote) { i++; closed = true; break; }
      }
      if (!closed) throw new Error('Unterminated Rust string');
      if (quote === '"') value = source.slice(start + 1, i - 1);
    } else {
      const word = source.slice(i).match(/^\w+/)?.[0];
      i += word?.length ?? (source.startsWith('=>', i) ? 2 : 1);
    }
    result.push({ text: source.slice(start, i), value, start, end: i });
  }
  return result;
}

function endOf(list, start) {
  const stack = [];
  for (let i = start; i < list.length; i++) {
    const text = list[i].text;
    if ('([{'.includes(text) && text.length === 1) stack.push(text);
    else if (')]}'.includes(text) && text.length === 1) {
      if ('([{'.indexOf(stack.pop()) !== ')]}'.indexOf(text)) throw new Error('Unbalanced Rust source');
      if (!stack.length) return i;
    }
  }
  throw new Error('Unterminated Rust group');
}

function declarationStart(list, index) {
  let start = index;
  for (;;) {
    const previous = list[start - 1]?.text;
    if (['pub','async','unsafe','const','extern'].includes(previous)) { start--; continue; }
    if (previous === ']' || previous === ')') {
      const close = previous;
      const open = close === ']' ? '[' : '(';
      let depth = 1;
      let first = start - 2;
      for (; first >= 0; first--) {
        if (list[first].text === close) depth++;
        else if (list[first].text === open && --depth === 0) break;
      }
      const marker = list[first - 1]?.text;
      if ((open === '[' && marker === '#') || (open === '(' && marker === 'pub')) { start = first - 1; continue; }
    }
    return start;
  }
}

export function rustSurface(source, selected = /^(parse_|run_|main$|command_is_|build_provider_|attach_|incompatible_|should_send_)/) {
  const production = source.split(/#\[cfg\(test\)\]\s*mod tests\s*\{/)[0];
  const list = tokens(production);
  const functions = [];
  const branches = [];
  for (let i = 0; i < list.length; i++) {
    if (list[i].text !== 'fn' || !selected.test(list[i + 1]?.text ?? '')) continue;
    const name = list[i + 1].text;
    let open = i + 2;
    while (open < list.length && list[open].text !== '{') {
      if (list[open].text === '(' || list[open].text === '[') open = endOf(list, open);
      open++;
    }
    if (open >= list.length) throw new Error(`Missing function body: ${name}`);
    const end = endOf(list, open);
    const expression = production.slice(list[declarationStart(list,i)].start, list[end].end);
    const sourceLine = production.slice(0, list[i].start).split('\n').length;
    const flags = [...new Set(list.slice(open, end).filter(t => t.value?.match(/^--?[a-zA-Z][\w-]*$/)).map(t => t.value))].sort();
    functions.push({ name, sourceLine, expression, flags });
    for (let j = open + 1; j < end; j++) {
      if (list[j].text !== 'match') continue;
      let body = j + 1;
      while (body < end && list[body].text !== '{') {
        if (list[body].text === '(' || list[body].text === '[') body = endOf(list, body);
        body++;
      }
      if (body >= end) throw new Error(`Missing match body: ${name}`);
      const stop = endOf(list, body);
      for (let arm = body + 1; arm < stop;) {
        let arrow = arm;
        while (arrow < stop && list[arrow].text !== '=>') {
          if ('([{'.includes(list[arrow].text) && list[arrow].text.length === 1) arrow = endOf(list, arrow);
          arrow++;
        }
        if (arrow >= stop) throw new Error(`Unrecognized match arm: ${name}`);
        const pattern = list.slice(arm, arrow);
        let finish = arrow + 1;
        // Block arms need no comma; ordinary expressions do.
        if (list[finish]?.text === '{') finish = endOf(list, finish) + 1;
        else {
          while (finish < stop && list[finish].text !== ',') {
            if ('([{'.includes(list[finish].text) && list[finish].text.length === 1) finish = endOf(list, finish);
            finish++;
          }
        }
        const guard = pattern.findIndex(t => t.text === 'if');
        const literals = pattern.slice(0, guard < 0 ? pattern.length : guard).filter(t => t.value !== undefined).map(t => t.value);
        branches.push({ function: name, pattern: production.slice(list[arm].start, list[arrow].start).trim(), literals, start: list[arm].start, end: list[finish - 1].end, sourceLine: production.slice(0, list[arm].start).split('\n').length, expression: production.slice(list[arrow].end, list[finish - 1].end).trim() });
        arm = list[finish]?.text === ',' ? finish + 1 : finish;
      }
    }
  }
  if (!functions.length) throw new Error('No selected Rust function declarations');
  for (const branch of branches) {
    branch.ancestors = branches.filter(parent => parent.start < branch.start && parent.end >= branch.end).sort((a,b) => a.start - b.start).map(parent => parent.pattern);
  }
  return { functions, branches };
}

export function mcpProfiles(source, declarations) {
  const names = new Map(declarations.map(tool => [tool.constant, tool.name]));
  const profiles = [];
  for (const match of source.matchAll(/^const ([A-Z]+)_PROFILE_TOOLS: &\[&str\] = &\[([^]*?)\];/gm)) {
    const constants = match[2].replace(/\/\/[^\n]*/g, '').split(',').map(v => v.trim()).filter(Boolean);
    const tools = constants.map(value => {
      if (!names.has(value)) throw new Error(`Unknown profile tool ${value}`);
      return names.get(value);
    });
    profiles.push({ name: match[1].toLowerCase(), tools });
  }
  if (!profiles.some(profile => profile.name === 'core') || !source.includes('enabled_tools: None') || !source.includes('Self::from_profiles(vec![ToolProfile::Core])')) throw new Error('Changed MCP profile composition/default');
  profiles.push({ name: 'all', tools: [...names.values()] });
  // Some declared tools intentionally occur only in All (e.g. dblclick at
  // this pin). Do not silently assign those tools to the default Core set.
  return profiles;
}
