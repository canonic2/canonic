// Extract pinned browser-only helpers as data; never import the package.
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');

export function injectedSource(bundle) {
  return generatedSource(bundle, 'injectedScript', 'injectedScript_exports', 'injected');
}
export function storageSource(bundle) {
  return generatedSource(bundle, 'storageScript', 'storageScript_exports', 'storage');
}
export function clockSource(bundle) {
  return generatedSource(bundle, 'clock', 'clock_exports', 'clock');
}
function correctedClockSource(source) {
  const patches = [
    ['    delay = Math.max(0, delay);', '    delay = Math.max(options.type === "Interval" ? 1 : 0, delay);'],
    ['        isPaused = true;\n        this._innerSetTime(asWallTime(param));',
      '        isPaused = true;\n        const toConsume = param - this._now.time;\n        if (toConsume < 0)\n          throw new Error("Cannot fast-forward to the past");\n        this._advanceNow(shiftTicks(this._now.ticks, toConsume));'],
  ];
  for (const [before, after] of patches) {
    if (source.split(before).length !== 2) throw new Error('Pinned clock correction boundary changed');
    source = source.replace(before, after);
  }
  return '// Generated from the pinned Playwright Clock; see tools/browser-provenance/cdp-clock.json.\n'
    + '// Testrun changes: minimum 1 ms intervals; pause replay preserves fixed Date and ticks.\n'
    + source;
}
function generatedSource(bundle, name, exports, label) {
  const marker = `// packages/playwright-core/src/generated/${name}Source.ts\n`;
  const start = bundle.indexOf(marker);
  const end = bundle.indexOf('\n// ', start + marker.length);
  if (start < 0 || end < 0) throw new Error(`Missing pinned ${label} source section`);
  const section = bundle.slice(start, end);
  const assignment = section.match(/^    source\d* = '/m);
  if (!assignment) throw new Error(`Missing pinned ${label} string`);
  const offset = assignment.index + assignment[0].length;
  let result = '';
  for (let at = offset; at < section.length; at++) {
    const char = section[at];
    if (char === "'") {
      if (!/^;\s*}\s*}\);\s*$/.test(section.slice(at + 1))) throw new Error(`Unexpected ${label} source suffix`);
      if (!result.includes(`module.exports = __toCommonJS(${exports});`)) throw new Error(`Unexpected ${label} module`);
      return result;
    }
    if (char !== '\\') { result += char; continue; }
    const escaped = section[++at];
    const escapes = { n:'\n', r:'\r', t:'\t', b:'\b', f:'\f', v:'\v', "'":"'", '"':'"', '\\':'\\' };
    if (Object.hasOwn(escapes, escaped)) result += escapes[escaped];
    else if (escaped === 'u' && /^[0-9a-fA-F]{4}$/.test(section.slice(at + 1, at + 5))) {
      result += String.fromCharCode(parseInt(section.slice(at + 1, at + 5), 16));
      at += 4;
    } else throw new Error(`Unsupported ${label} string escape`);
  }
  throw new Error(`Unterminated ${label} source`);
}

export function macEditingCommands(bundle) {
  const section=bundle.split('// packages/playwright-core/src/server/macEditingCommands.ts\n')[1]?.split('\n// ')[0];
  const literal=section?.match(/    macEditingCommands = (\{[\s\S]*?\n    \});/);
  if(!literal)throw new Error('Missing pinned Mac editing commands');
  const commands=JSON.parse(literal[1]);
  for(const value of Object.values(commands))if(!(typeof value==='string'||Array.isArray(value)&&value.every(command=>typeof command==='string')))throw new Error('Unexpected Mac editing command value');
  return JSON.stringify(commands,null,2)+'\n';
}

export function webSocketSource(bundle) {
  const marker='// packages/playwright-core/src/generated/webSocketMockSource.ts\n';
  const start=bundle.indexOf(marker);
  if(start<0)throw new Error('Missing pinned WebSocket source section');
  const section=bundle.slice(start+marker.length);
  const header=section.match(/^var (source\d*);\nvar init_webSocketMockSource = __esm\(\{\n  "packages\/playwright-core\/src\/generated\/webSocketMockSource\.ts"\(\) \{\n    "use strict";\n    \1 = `/);
  if(!header)throw new Error('Unexpected pinned WebSocket source header');
  let result='';
  for(let at=header[0].length;at<section.length;at++) {
    const char=section[at];
    if(char==='`') {
      if(!/^;\s*}\s*}\);(?:\s*\/\/|\s*$)/.test(section.slice(at+1)))throw new Error('Unexpected WebSocket source suffix');
      if(!result.includes('module.exports = __toCommonJS(webSocketMock_exports);')||!result.includes('function inject(globalThis)'))throw new Error('Unexpected WebSocket module');
      return result;
    }
    if(char==='$'&&section[at+1]==='{')throw new Error('Dynamic WebSocket source interpolation');
    if(char!=='\\'){result+=char;continue;}
    const escaped=section[++at];
    const escapes={n:'\n',r:'\r',t:'\t',b:'\b',f:'\f',v:'\v',"'":"'",'"':'"','\\':'\\','`':'`','$':'$'};
    if(Object.hasOwn(escapes,escaped))result+=escapes[escaped];
    else if(escaped==='u'&&/^[0-9a-fA-F]{4}$/.test(section.slice(at+1,at+5))){result+=String.fromCharCode(parseInt(section.slice(at+1,at+5),16));at+=4;}
    else if(escaped==='x'&&/^[0-9a-fA-F]{2}$/.test(section.slice(at+1,at+3))){result+=String.fromCharCode(parseInt(section.slice(at+1,at+3),16));at+=2;}
    else throw new Error('Unsupported WebSocket source escape');
  }
  throw new Error('Unterminated WebSocket source');
}

async function main() {
  const [packageRoot, mode] = process.argv.slice(2);
  if (!packageRoot || mode && mode !== '--check') throw new Error('Supply PLAYWRIGHT_CORE [--check]');
  const baseline = JSON.parse(await readFile(resolve(root, 'specs/browser/upstream-baseline.json'), 'utf8'));
  const bundle = await readFile(resolve(packageRoot, 'lib/coreBundle.js'));
  if (digest(bundle) !== baseline.playwrightCore.inspectedFilesSha256['lib/coreBundle.js']) throw new Error('Pinned Playwright bundle changed');
  const source = injectedSource(bundle.toString('utf8'));
  const editing=macEditingCommands(bundle.toString('utf8'));
  const websocket=webSocketSource(bundle.toString('utf8'));
  const storage=storageSource(bundle.toString('utf8'));
  const upstreamClock=clockSource(bundle.toString('utf8'));
  const clock=correctedClockSource(upstreamClock);
  const clockManifest=JSON.stringify({version:baseline.playwrightCore.version,license:'Apache-2.0',copyright:'Microsoft Corporation',
    licenseFile:'third-party/browser/upstream-notices/LICENSE',notice:'third-party/browser/upstream-notices/NOTICE',
    source:'lib/coreBundle.js#packages/playwright-core/src/generated/clockSource.ts',sourceSha256:digest(bundle),
    upstreamHelperSha256:digest(upstreamClock),corrections:['minimum-interval-delay-1ms','pause-replay-preserves-fixed-time-and-ticks'],
    helper:'src/browser/reading/clock.js',helperSha256:digest(clock)},null,2)+'\n';
  const storageManifest=JSON.stringify({version:baseline.playwrightCore.version,license:'Apache-2.0',copyright:'Microsoft Corporation',
    licenseFile:'third-party/browser/upstream-notices/LICENSE',notice:'third-party/browser/upstream-notices/NOTICE',
    source:'lib/coreBundle.js#packages/playwright-core/src/generated/storageScriptSource.ts',sourceSha256:digest(bundle),
    helper:'src/browser/reading/storage.js',helperSha256:digest(storage)},null,2)+'\n';
  const websocketManifest=JSON.stringify({version:baseline.playwrightCore.version,license:'Apache-2.0',copyright:'Microsoft Corporation',
    licenseFile:'third-party/browser/upstream-notices/LICENSE',notice:'third-party/browser/upstream-notices/NOTICE',
    source:'lib/coreBundle.js#packages/playwright-core/src/generated/webSocketMockSource.ts',sourceSha256:digest(bundle),
    helper:'src/browser/reading/websocket.js',helperSha256:digest(websocket)},null,2)+'\n';
  const manifest = JSON.stringify({ version:baseline.playwrightCore.version, license:'Apache-2.0', copyright:'Microsoft Corporation',
    licenseFile:'third-party/browser/upstream-notices/LICENSE', notice:'third-party/browser/upstream-notices/NOTICE',
    source:'lib/coreBundle.js#packages/playwright-core/src/generated/injectedScriptSource.ts', sourceSha256:digest(bundle), helper:'src/browser/reading/selector.js', helperSha256:digest(source),
    adapter:'src/browser/reading/factory.js',adapterSha256:digest(await readFile(resolve(root,'src/browser/reading/factory.js'))),
    macEditingCommands:'src/browser/cdp/mac-editing-commands.json',macEditingCommandsSha256:digest(editing) }, null, 2) + '\n';
  for (const [file, value] of [['src/browser/reading/selector.js', source], ['src/browser/cdp/mac-editing-commands.json',editing], ['tools/browser-provenance/cdp-selector.json', manifest],['src/browser/reading/websocket.js',websocket],['tools/browser-provenance/cdp-websocket.json',websocketManifest],['src/browser/reading/storage.js',storage],['tools/browser-provenance/cdp-storage.json',storageManifest],['src/browser/reading/clock.js',clock],['tools/browser-provenance/cdp-clock.json',clockManifest]]) {
    if (mode === '--check') {
      if (await readFile(resolve(root, file), 'utf8') !== value) throw new Error(`Injected helper drift: ${file}`);
    } else await writeFile(resolve(root, file), value);
  }
  process.stdout.write(`Verified Playwright ${baseline.playwrightCore.version} browser selector, WebSocket, storage and clock helpers\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { process.stderr.write(error.message + '\n'); process.exitCode = 1; });
}
