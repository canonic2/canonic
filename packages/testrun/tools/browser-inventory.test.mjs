import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { bindingSchema, clientInventory, declarationInventory, generate, keyboardLayout, protocolInventory, schemaExpression, withoutComments } from './browser-inventory.mjs';

const keyboard = `// packages/playwright-core/src/server/usKeyboardLayout.ts
var init_layout = __esm({
  layout() {
    USKeyboardLayout = {
      "KeyA": { "keyCode": 65, "key": "a", "shiftKey": "A" },
      "Quote": { "keyCode": 222, "key": "'", "shiftKey": '"' },
      "ShiftLeft": { "keyCode": 16, "key": "Shift" }
    };
  }
});
// packages/playwright-core/src/server/input.ts
    aliases = /* @__PURE__ */ new Map([
      ["ShiftLeft", ["Shift"]]
    ]);
// next section
`;

test('keyboard data retains canonical codes, character variants and alias provenance without execution', () => {
  const parsed = keyboardLayout(keyboard);
  assert.equal(parsed.layout.KeyA.shiftKey, 'A');
  assert.equal(parsed.layout.Quote.shiftKey, '"');
  assert.deepEqual(parsed.aliases.ShiftLeft, ['Shift']);
  assert.equal(parsed.layoutSourceSha256.length, 64);
  assert.equal(parsed.sourceSha256.length, 64);
  for (const source of [keyboard.replace('65', 'process.exit()'), keyboard.replace('"KeyA"', '"__proto__"'),
    keyboard.replace('["ShiftLeft", ["Shift"]]', '["Unknown", ["Shift"]]'),
    keyboard.replace('"keyCode": 65', '"keyCode": -1'), '']) assert.throws(() => keyboardLayout(source));
});

const protocol = `// packages/protocol/src/validator.ts
var init_validator = __esm({
  "packages/protocol/src/validator.ts"() {
    scheme.RootInitializer = tOptional(tObject({}));
    scheme.RootInitializeParams = tObject({
      sdkLanguage: tEnum(["javascript", "python"]),
      options: tOptional(tObject({ timeout: tFloat }))
    });
  }
});
// packages/playwright-core/src/client/connection.ts
var client = {};
// packages/playwright-core/src/server/dispatchers/dispatcher.ts
var dispatcher = {};
`;

test('declarative schemas parse to data without executing expressions', () => {
  assert.deepEqual(JSON.parse(JSON.stringify(schemaExpression('tOptional(tObject({ child: tChannel(["Page"]), values: tArray(tType("Value")) }))'))), {
    kind: 'optional', value: { kind: 'object', value: { child: { kind: 'channel', value: ['Page'] },
      values: { kind: 'array', value: { kind: 'type', value: 'Value' } } } },
  });
  assert.deepEqual(schemaExpression('tChannel("*")'), { kind: 'channel', value: ['*'] });
  assert.deepEqual(schemaExpression('tEnum([1, "value"])'), { kind: 'enum', value: [1, 'value'] });
  for (const value of ['process.exit()', 'tAny; evil()', 'tUnknown(tString)', 'tObject({ x: tString, x: tInt })', 'tString tInt']) {
    assert.throws(() => schemaExpression(value));
  }
});

test('protocol inventory retains nested options and separates client helpers', () => {
  const records = protocolInventory(protocol);
  assert.deepEqual(records.map(record => record.name), ['RootInitializer', 'RootInitializeParams']);
  assert.equal(records[0].line, 4);
  assert.match(records[1].validator, /timeout: tFloat/);
  assert.match(records[1].validator, /"python"/);
  const clients = clientInventory(protocol);
  assert.equal(clients.length, 1);
  assert.equal(clients[0].name, 'packages/playwright-core/src/client/connection.ts');
  assert.equal(clients[0].sha256.length, 64);
});

test('protocol parser refuses missing sections, duplicates and unparsed assignments', () => {
  assert.throws(() => protocolInventory(''), /Missing/);
  assert.throws(() => protocolInventory(protocol.replace('RootInitializeParams', 'RootInitializer')), /Duplicate/);
  assert.throws(() => protocolInventory(protocol.replace('scheme.RootInitializeParams =', 'scheme.RootInitializeParams=')), /Unparsed/);
});

test('declarations preserve overloads, events, inherited interfaces and nested options', () => {
  const source = `/** Not a declaration: export interface Fake {} */
export interface Page<T = {}> extends Parent<T> {
  /** a method with fake text and delimiters: }; */
  click(options?: {
    delay?: number;
    url?: 'https://example.com/*quoted*/';
  }): Promise<void>;
  click(selector: string): Promise<void>;
  on(event: 'dialog', listener: (dialog: Dialog) => void): this;
  readonly keyboard: Keyboard;
  [Symbol.asyncDispose](): Promise<void>;
  send<T>(
    method: T
  ): Promise<void>;
}
export interface Child extends Page { }
export const browser: BrowserType;
export type Options = {
  token?: string;
  nested?: { delay: number };
}
`;
  const records = declarationInventory(source, 'types.d.ts');
  const members = records.filter(record => record.kind === 'library-member');
  assert.deepEqual(members.map(record => record.name), ['click', 'click', 'on', 'keyboard', '[Symbol.asyncDispose]', 'send']);
  assert.equal(members[2].event, 'dialog');
  assert.equal(members[0].line, 4);
  assert.match(members[0].signature, /delay\?: number/);
  assert.match(members[0].signature, /https:\/\/example.com\/\*quoted\*\//);
  assert.equal(records.filter(record => record.kind === 'library-declaration').length, 4);
  assert.match(records.find(record => record.name === 'Options').signature, /nested\?: \{ delay: number }/);
});

test('unrecognized members and incomplete declarations fail visibly', () => {
  assert.throws(() => declarationInventory('export interface A {\n  ???\n}', 'fixture'), /Unparsed/);
  assert.throws(() => declarationInventory('export interface A {\n  a(): void\n}', 'fixture'), /Unterminated/);
  assert.throws(() => declarationInventory('export interface A {\n', 'fixture'), /Unclosed/);
  assert.throws(() => declarationInventory('', 'fixture'), /Empty/);
});

test('comment removal preserves lines, escapes and quoted comment markers', () => {
  const source = `// comment\nconst value = 'https://example.com/\\\'/*safe*/'; /* block\nline */\n`;
  const clean = withoutComments(source);
  assert.equal(clean.split('\n').length, source.split('\n').length);
  assert.match(clean, /https:\/\/example.com/);
  assert.match(clean, /\/\*safe\*\//);
  assert.doesNotMatch(clean, /block|comment/);
});

test('generation is deterministic and rejects changed identity or pinned source', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'testrun-browser-inventory-'));
  try {
    await mkdir(join(directory, 'lib'));
    await mkdir(join(directory, 'types'));
    const files = {
      'lib/coreBundle.js': protocol,
      'types/types.d.ts': 'export interface Root {\n  initialize(): void;\n}\n',
      'types/structs.d.ts': 'export type Serializable = any;\n',
    };
    for (const [file, contents] of Object.entries(files)) await writeFile(join(directory, file), contents);
    const metadata = { name: 'playwright-core', version: '1.64.1', license: 'Apache-2.0' };
    await writeFile(join(directory, 'package.json'), JSON.stringify(metadata));
    const baseline = { date: '2026-10-22', playwrightCore: { version: '1.64.1',
      inspectedFilesSha256: Object.fromEntries(Object.entries(files)
        .map(([file, content]) => [file, createHash('sha256').update(content).digest('hex')])) } };
    const first = await generate(directory, baseline);
    assert.equal(first, await generate(directory, baseline));
    const header = JSON.parse(first.split('\n')[0]);
    assert.equal(header.defaultDisposition.maturity, 'unimplemented');
    assert.equal(header.defaultDisposition.testrunOperation, null);
    assert.equal(header.counts['protocol-schema'], 2);
    const generatedSchema = JSON.parse(bindingSchema(first));
    assert.equal(generatedSchema.schemas.RootInitializeParams.kind, 'object');
    await writeFile(join(directory, 'types/types.d.ts'), 'tampered');
    await assert.rejects(generate(directory, baseline), /hash mismatch/);
    await writeFile(join(directory, 'package.json'), JSON.stringify({ ...metadata, version: 'new' }));
    await assert.rejects(generate(directory, baseline), /identity/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
