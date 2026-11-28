// Development-only inventory of a verified, pinned package; never executes its code.
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const digest = text => createHash('sha256').update(text).digest('hex');

// Preserve offsets/line numbers and quoted text while removing comments. The
// input is generated declarations or the declarative validator block, not JS
// with regular-expression literals. No eval, imports or package hooks are used.
export function withoutComments(source) {
  const chars = [...source];
  let quote;
  for (let i = 0; i < chars.length; i++) {
    if (quote) {
      if (chars[i] === '\\') i++;
      else if (chars[i] === quote) quote = undefined;
    } else if (['"', "'", '`'].includes(chars[i])) {
      quote = chars[i];
    } else if (chars[i] === '/' && ['/', '*'].includes(chars[i + 1])) {
      const block = chars[i + 1] === '*';
      chars[i++] = ' ';
      chars[i] = ' ';
      for (i++; i < chars.length; i++) {
        if (!block && chars[i] === '\n') break;
        if (block && chars[i] === '*' && chars[i + 1] === '/') {
          chars[i++] = ' ';
          chars[i] = ' ';
          break;
        }
        if (chars[i] !== '\n') chars[i] = ' ';
      }
    }
  }
  if (quote) throw new Error('Unterminated quoted declaration');
  return chars.join('');
}

export function protocolInventory(source) {
  const marker = '// packages/protocol/src/validator.ts\n';
  const start = source.indexOf(marker);
  if (start < 0) throw new Error('Missing pinned protocol validator');
  const end = source.indexOf('\n// ', start + marker.length);
  if (end < 0) throw new Error('Missing validator section boundary');
  const section = source.slice(start, end);
  const entries = [];
  const seen = new Set();
  const assignments = [...section.matchAll(/^    scheme\.(\w+) = ([\s\S]*?);\n/gm)];
  if (!assignments.length) throw new Error('Empty protocol inventory');
  for (const match of assignments) {
    if (seen.has(match[1])) throw new Error(`Duplicate validator ${match[1]}`);
    seen.add(match[1]);
    entries.push({
      kind: 'protocol-schema', name: match[1],
      source: 'lib/coreBundle.js',
      line: source.slice(0, start + match.index).split('\n').length,
      // Keep the complete validator expression, including nested options,
      // referenced types and enum values; this is data, never executed code.
      validator: match[2].trim(),
    });
  }
  const assignmentsCount = (section.match(/\bscheme\./g) ?? []).length;
  if (entries.length !== assignmentsCount) throw new Error('Unparsed protocol assignment');
  return entries;
}

export function declarationInventory(source, file) {
  const lines = withoutComments(source).split('\n');
  const entries = [];
  const declaration = /^(export )?(interface|type|const|namespace|class) ([\w$]+)/;
  for (let i = 0; i < lines.length; i++) {
    const match = lines[i].match(declaration);
    if (!match) continue;
    const name = match[3];
    entries.push({ kind: 'library-declaration', name, declaration: match[2],
      exported: Boolean(match[1]), source: file, line: i + 1, header: lines[i].trim(),
      ...(['type', 'const'].includes(match[2]) ? { signature: statement(lines, i) } : {}) });
    if (match[2] !== 'interface') continue;
    // The pinned generated format puts the interface's closing brace at column
    // zero, members at two spaces and nested options at deeper indentation.
    if (/\{\s*\}/.test(lines[i].slice(lines[i].lastIndexOf('{')))) continue;
    let end = i + 1;
    while (end < lines.length && !/^}/.test(lines[end])) end++;
    if (end === lines.length) throw new Error(`Unclosed interface ${name}`);
    const members = [];
    for (let j = i + 1; j < end; j++) {
      const member = lines[j].match(/^  (?:readonly )?([\w$]+|\[[^\]]+\])(?=[(<:?:])/);
      if (member) members.push({ at: j, name: member[1] });
      else if (/^  [^\s})]/.test(lines[j])) {
        throw new Error(`Unparsed ${file}:${j + 1} interface member`);
      }
    }
    for (let j = 0; j < members.length; j++) {
      const member = members[j];
      const signature = lines.slice(member.at, members[j + 1]?.at ?? end)
        .map(line => line.trimEnd()).join('\n').trim();
      if (!signature.endsWith(';')) throw new Error(`Unterminated ${name}.${member.name}`);
      const event = signature.match(/^\w+\(event: '([^']+)'/);
      entries.push({ kind: 'library-member', owner: name, name: member.name,
        ...(event ? { event: event[1] } : {}), source: file, line: member.at + 1, signature });
    }
    i = end;
  }
  if (!entries.length) throw new Error(`Empty declarations: ${file}`);
  return entries;
}

function statement(lines, start) {
  const text = lines.slice(start).join('\n');
  let nesting = 0;
  let quote;
  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (quote) {
      if (char === '\\') i++;
      else if (char === quote) quote = undefined;
    } else if (['"', "'", '`'].includes(char)) quote = char;
    else if ('{[('.includes(char)) nesting++;
    else if ('}])'.includes(char)) {
      nesting--;
      if (char === '}' && nesting === 0 && /^\s*(?:export |interface |type |$)/.test(text.slice(i + 1))) {
        return text.slice(0, i + 1).trim();
      }
    }
    else if (char === ';' && nesting === 0) return text.slice(0, i + 1).trim();
    if (nesting < 0) break;
  }
  throw new Error(`Unterminated declaration at line ${start + 1}`);
}

export function clientInventory(source) {
  const sections = [...source.matchAll(/^\/\/ (packages\/[^\n]+)\n/gm)];
  return sections.flatMap((match, i) => {
    if (!match[1].startsWith('packages/playwright-core/src/client/')) return [];
    const body = source.slice(match.index, sections[i + 1]?.index ?? source.length);
    return [{ kind: 'client-helper', name: match[1], source: 'lib/coreBundle.js',
      line: source.slice(0, match.index).split('\n').length, sha256: digest(body) }];
  });
}

// Parse only the pinned declarative validator grammar. This deliberately does
// not evaluate JavaScript or import any part of the package being inspected.
export function schemaExpression(source) {
  const tokens = [];
  const pattern = /\s+|"(?:\\.|[^"\\])*"|[A-Za-z_]\w*|-?\d+(?:\.\d+)?|[()[\]{},:]/gy;
  let offset = 0;
  while (offset < source.length) {
    pattern.lastIndex = offset;
    const token = pattern.exec(source);
    if (!token) throw new Error('Unrecognized validator token');
    offset = pattern.lastIndex;
    if (token[0].trim()) tokens.push(token[0]);
  }
  let at = 0;
  const take = expected => {
    const token = tokens[at++];
    if (expected && token !== expected) throw new Error(`Expected validator token ${expected}`);
    return token;
  };
  const literal = () => {
    const token = take();
    if (!token || !/^(?:"|-?\d)/.test(token)) throw new Error('Invalid validator literal');
    return JSON.parse(token);
  };
  const array = () => {
    take('[');
    const values = [];
    while (tokens[at] !== ']') {
      values.push(literal());
      if (tokens[at] !== ']') take(',');
    }
    take(']');
    return values;
  };
  const parse = (depth = 0) => {
    if (depth > 64) throw new Error('Validator expression is too deep');
    const primitive = { tFloat: 'float', tInt: 'int', tBoolean: 'boolean',
      tString: 'string', tBinary: 'binary', tAny: 'any' };
    const name = take();
    if (primitive[name]) return { kind: primitive[name] };
    take('(');
    let result;
    if (['tOptional', 'tArray'].includes(name)) {
      result = { kind: name === 'tArray' ? 'array' : 'optional', value: parse(depth + 1) };
    } else if (name === 'tObject') {
      take('{');
      const fields = Object.create(null);
      while (tokens[at] !== '}') {
        const key = tokens[at]?.startsWith('"') ? literal() : take();
        if (typeof key !== 'string' || Object.hasOwn(fields, key)) throw new Error('Invalid validator field');
        take(':');
        fields[key] = parse(depth + 1);
        if (tokens[at] !== '}') take(',');
      }
      take('}');
      result = { kind: 'object', value: fields };
    } else if (name === 'tEnum') result = { kind: 'enum', value: array() };
    else if (name === 'tChannel') {
      result = { kind: 'channel', value: tokens[at] === '[' ? array() : [literal()] };
    } else if (name === 'tType') result = { kind: 'type', value: literal() };
    else throw new Error(`Unknown validator primitive ${name}`);
    take(')');
    return result;
  };
  const result = parse();
  if (at !== tokens.length) throw new Error('Trailing validator tokens');
  return result;
}

export function bindingSchema(inventory) {
  const records = inventory.trimEnd().split('\n').map(line => JSON.parse(line));
  const schemas = Object.fromEntries(records.filter(record => record.kind === 'protocol-schema')
    .map(record => [record.name, schemaExpression(record.validator)]));
  return JSON.stringify({ version: records[0].version, license: records[0].provenance.license,
    copyright: records[0].provenance.copyright,
    sourceSha256: records[0].provenance.filesSha256['lib/coreBundle.js'],
    sourceFilesSha256: records[0].provenance.filesSha256, schemas }) + '\n';
}

// The pinned key table and aliases are declarative literals. Keep their
// bytes/provenance; never import or execute the browser package to obtain them.
function keyboardJson(source) {
  const tokens = /\s+|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|-?\d+|[{}\[\],:]/gy;
  let at = 0, result = '';
  while (at < source.length) {
    tokens.lastIndex = at;
    const match = tokens.exec(source);
    if (!match) throw new Error('Non-declarative keyboard token');
    at = tokens.lastIndex;
    const token = match[0];
    if (!token.startsWith("'")) { result += token; continue; }
    let value = '"';
    for (let index = 1; index < token.length - 1; index++) {
      const char = token[index];
      if (char === '\\') {
        const escaped = token[++index];
        if (escaped === "'") value += "'";
        else if ('"\\/bfnrtu'.includes(escaped)) value += '\\' + escaped;
        else throw new Error('Unsupported keyboard string escape');
      } else value += char === '"' ? '\\"' : char;
    }
    result += JSON.stringify(JSON.parse(value + '"'));
  }
  return JSON.parse(result);
}

export function keyboardLayout(source) {
  const start = source.indexOf('// packages/playwright-core/src/server/usKeyboardLayout.ts\n');
  const end = source.indexOf('// packages/playwright-core/src/server/input.ts\n', start + 1);
  if (start < 0 || end < 0) throw new Error('Missing pinned keyboard layout section');
  const section = source.slice(start, end);
  const layouts = [...withoutComments(section).matchAll(/^    USKeyboardLayout = (\{[\s\S]*?\n    \});$/gm)];
  const inputsEnd = source.indexOf('\n// ', end + 1);
  const inputs = withoutComments(source.slice(end, inputsEnd < 0 ? undefined : inputsEnd));
  const aliases = [...inputs.matchAll(/aliases =\s*new Map\((\[[\s\S]*?\n    \])\);/g)];
  if (layouts.length !== 1 || aliases.length !== 1) throw new Error('Unparsed or duplicate pinned keyboard data');
  const layout = keyboardJson(layouts[0][1]);
  const aliasEntries = keyboardJson(aliases[0][1]);
  const allowed = new Set(['key', 'keyCode', 'keyCodeWithoutLocation', 'shiftKey', 'shiftKeyCode', 'text', 'location']);
  for (const [code, definition] of Object.entries(layout)) {
    if (!/^[A-Za-z][A-Za-z0-9]*$/.test(code) || !definition || typeof definition !== 'object' || Array.isArray(definition)
      || typeof definition.key !== 'string' || definition.key.length > 64
      || Object.keys(definition).some(field => !allowed.has(field))
      || Object.entries(definition).some(([field, value]) => ['key', 'shiftKey', 'text'].includes(field)
        ? typeof value !== 'string' || value.length > 64 : !Number.isInteger(value) || value < 0 || value > 255)) {
      throw new Error('Invalid pinned keyboard definition');
    }
  }
  if (!Object.keys(layout).length || Object.keys(layout).length > 256 || !Array.isArray(aliasEntries) || aliasEntries.length > 32)
    throw new Error('Pinned keyboard bounds');
  const seen = new Set();
  for (const entry of aliasEntries) {
    if (!Array.isArray(entry) || entry.length !== 2 || !Object.hasOwn(layout, entry[0]) || !Array.isArray(entry[1]) || !entry[1].length
      || entry[1].some(alias => typeof alias !== 'string' || !alias.length || alias.length > 64 || seen.has(alias)))
      throw new Error('Invalid pinned keyboard aliases');
    for (const alias of entry[1]) seen.add(alias);
  }
  return { sourceSha256: digest(source), layoutSourceSha256: digest(section),
    aliasesSourceSha256: digest(aliases[0][1]), layout, aliases: Object.fromEntries(aliasEntries) };
}

export async function generate(packageRoot, baseline) {
  const metadata = JSON.parse(await readFile(resolve(packageRoot, 'package.json'), 'utf8'));
  if (metadata.name !== 'playwright-core' || metadata.version !== baseline.playwrightCore.version) {
    throw new Error('Package identity differs from the reviewed development baseline');
  }
  const inputs = {};
  for (const [file, expected] of Object.entries(baseline.playwrightCore.inspectedFilesSha256)) {
    const bytes = await readFile(resolve(packageRoot, file));
    if (digest(bytes) !== expected) throw new Error(`Pinned source hash mismatch: ${file}`);
    inputs[file] = bytes.toString('utf8');
  }
  const structs = inputs['types/structs.d.ts'];
  if (structs === undefined) throw new Error('Missing structs source pin');
  const records = [
    ...protocolInventory(inputs['lib/coreBundle.js']),
    ...declarationInventory(inputs['types/types.d.ts'], 'types/types.d.ts'),
    ...declarationInventory(structs, 'types/structs.d.ts'),
    ...clientInventory(inputs['lib/coreBundle.js']),
  ];
  const counts = {};
  for (const record of records) counts[record.kind] = (counts[record.kind] ?? 0) + 1;
  const header = { kind: 'inventory-metadata', schemaVersion: 1,
    version: metadata.version, date: baseline.date,
    provenance: { license: metadata.license, copyright: 'Microsoft Corporation',
      notice: 'upstream-notices/NOTICE', licenseFile: 'upstream-notices/LICENSE',
      filesSha256: { ...baseline.playwrightCore.inspectedFilesSha256,
        'types/structs.d.ts': digest(structs) } },
    counts,
    defaultDisposition: { maturity: 'unimplemented', rustBinding: null,
      testrunOperation: null, restrictions: 'Not assessed; no capability claim',
      testEvidence: [], gapOwner: 'browser binding and workflows',
      reason: 'Awaiting per-member semantic review and implementation' },
    overrides: 'upstream-baseline.json#inventory',
    remaining: ['Per-member semantic mapping, engine/connection restrictions and tests',
      'Client-side implementation review; helper hashes are an enumeration, not a port',
      'Native CDP protocol inventory and generated selectors/injected helpers'],
  };
  return [header, ...records].map(record => JSON.stringify(record)).join('\n') + '\n';
}

async function main() {
  const [packageRoot, mode] = process.argv.slice(2);
  if (!packageRoot || (mode && mode !== '--check')) {
    throw new Error('Usage: browser-inventory.mjs PLAYWRIGHT_CORE [--check]');
  }
  const baseline = JSON.parse(await readFile(resolve(root, 'specs/browser/upstream-baseline.json'), 'utf8'));
  const output = await generate(packageRoot, baseline);
  const destination = resolve(root, 'specs/browser/upstream-inventory.jsonl');
  const schema = bindingSchema(output);
  const schemaDestination = resolve(root, 'src/browser/protocol-schema.json');
  const header = JSON.parse(output.split('\n')[0]);
  const keys = keyboardLayout(await readFile(resolve(packageRoot, 'lib/coreBundle.js'), 'utf8'));
  if (keys.sourceSha256 !== header.provenance.filesSha256['lib/coreBundle.js']) throw new Error('Keyboard source changed during inventory');
  const keyboard = JSON.stringify({ version: header.version, license: header.provenance.license,
    copyright: header.provenance.copyright, ...keys }) + '\n';
  const keyboardDestination = resolve(root, 'src/browser/keyboard-layout.json');
  if (mode === '--check') {
    if (await readFile(destination, 'utf8') !== output) throw new Error('Upstream inventory drift');
    if (await readFile(schemaDestination, 'utf8') !== schema) throw new Error('Binding schema drift');
    if (await readFile(keyboardDestination, 'utf8') !== keyboard) throw new Error('Keyboard layout drift');
  } else {
    await writeFile(destination, output);
    await writeFile(schemaDestination, schema);
    await writeFile(keyboardDestination, keyboard);
    // Retain required attribution with the derived declaration/schema data.
    for (const file of ['LICENSE', 'NOTICE']) {
      const contents = await readFile(resolve(packageRoot, file));
      await writeFile(resolve(root, 'third-party/browser/upstream-notices', file), contents);
    }
  }
  process.stdout.write(JSON.stringify(JSON.parse(output.split('\n')[0]).counts) + '\n');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { process.stderr.write(error.message + '\n'); process.exitCode = 1; });
}
