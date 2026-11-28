import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { createContext, runInContext } from 'node:vm';
import { clockSource } from './browser-cdp-helper.mjs';

const helper = await readFile(new URL('../src/browser/reading/clock.js', import.meta.url), 'utf8');
const install = await readFile(new URL('../src/browser/cdp/session/clock-install.js', import.meta.url), 'utf8');
const apply = await readFile(new URL('../src/browser/cdp/session/clock-apply.js', import.meta.url), 'utf8');
const driver = runInContext(await readFile(new URL('../src/browser/driver/bootstrap.js', import.meta.url), 'utf8'), createContext({}));
const epoch = 1700000000000;
function document(history = [], sdk = false) {
  // Native scheduling is captured before replacement. Only the helper's own
  // yields execute here; native progression is explicitly driven by tests.
  let monotonic = 0;
  const native = new Map();
  let next = 0;
  const context = createContext({
    setTimeout: (callback, delay) => { const id = ++next; if (!delay) queueMicrotask(callback); else native.set(id, {callback,at:monotonic+delay}); return id; },
    clearTimeout: id => native.delete(id), setInterval: () => ++next, clearInterval: () => {},
    requestAnimationFrame: () => ++next, cancelAnimationFrame: () => {},
    requestIdleCallback: () => ++next, cancelIdleCallback: () => {},
    performance: Object.assign(Object.create({getEntries:()=>['native'],getEntriesByType:()=>['native'],getEntriesByName:()=>['native'],mark:()=>{},measure:()=>{}}), { now: () => monotonic }), console,
  });
  runInContext('globalThis.Event = class Event {}; Object.defineProperty(Event.prototype,"timeStamp",{configurable:true,get(){return 1;}})', context);
  runInContext('globalThis.acmeNativeNow=Date.now;globalThis.acmeNativeTimeout=setTimeout', context);
  const library = `()=>{const module={exports:{}};${helper}\nreturn module.exports;}`;
  const bootstrap = entries => {
    if (!sdk) return runInContext(`(${install})('AcmeClock',${JSON.stringify(entries)},${library})`, context);
    const source = `(()=>{const module={};${helper}\nglobalThis.__pwClock = (module.exports.inject())(globalThis,"firefox");})()`;
    runInContext(driver.correctClockBootstrap(source), context);
    for (const entry of entries) runInContext(`__pwClock.controller.log(...${JSON.stringify(entry)})`, context);
    runInContext(`globalThis.AcmeClock={controller:__pwClock.controller,sequence:${entries.length}}`, context);
  };
  bootstrap(history);
  return {
    run: source => runInContext(source, context), bootstrap,
    act: (sequence, method, parameter) => runInContext(`(${apply})('AcmeClock',${sequence},${JSON.stringify(method)},${JSON.stringify(parameter ?? null)})`, context),
    advanceNative: elapsed => { monotonic += elapsed; for (const [id, {callback,at}] of [...native]) if (native.has(id)&&at<=monotonic) { native.delete(id); callback(); } },
  };
}

test('SDK clock replay cancels its startup wakeup and initializes the first time-origin read', async () => {
  const page = document([['install', 0, epoch], ['pauseAt', 0, epoch]], true);
  assert.equal(page.run('performance.timeOrigin'), epoch);
  page.run('globalThis.calls=0;setTimeout(()=>calls++,0)');
  page.advanceNative(200);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(page.run('calls'), 0);
  await page.act(3, 'runFor', 1);
  assert.equal(page.run('calls'), 1);
  await page.act(4, 'resume');
  page.run('setTimeout(()=>calls++,1)');
  page.advanceNative(200);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(page.run('calls'), 2);
});

test('SDK clock correction preserves ordinary scripts and refuses changed pinned boundaries', () => {
  assert.equal(driver.correctClockBootstrap('globalThis.acme=42'), 'globalThis.acme=42');
  assert.throws(() => driver.correctClockBootstrap('globalThis.__pwClock = (module.exports.inject())'), /CLOCK_BOOTSTRAP_UNSUPPORTED/);
});

test('clock extraction refuses changed boundaries and matches the generated helper digest', async () => {
  const manifest = JSON.parse(await readFile(new URL('./browser-provenance/cdp-clock.json', import.meta.url)));
  assert.equal(createHash('sha256').update(helper).digest('hex'), manifest.helperSha256);
  const fixture = '// packages/playwright-core/src/generated/clockSource.ts\nvar source;\nvar init_clockSource = __esm({\n  "clock"() {\n    source = \'module.exports = __toCommonJS(clock_exports);\\n\';\n  }\n});\n\n// next\n';
  assert.equal(clockSource(fixture), 'module.exports = __toCommonJS(clock_exports);\n');
  assert.throws(() => clockSource(''), /Missing pinned/);
  assert.throws(() => clockSource(fixture.replace('clock_exports', 'foreign_exports')), /Unexpected clock module/);
});

test('clock controls Date, interval catch-up, microtasks and synthetic animation frames independently of host time', async () => {
  const page = document([['install', 0, epoch], ['pauseAt', 0, epoch]]);
  assert.equal(page.run('Date.now()'), epoch);
  assert.equal(page.run('+new Date()'), epoch);
  assert.equal(page.run('Date.parse(Date())'), epoch);
  page.run('globalThis.ticks=0;globalThis.frames=[];globalThis.idles=0;globalThis.order=[];setInterval(()=>{ticks++;Promise.resolve().then(()=>order.push(ticks))},1000);requestAnimationFrame(t=>frames.push(t));requestIdleCallback(()=>idles++);');
  await page.act(3, 'runFor', 2000);
  assert.equal(page.run('ticks'), 2);
  assert.equal(page.run('order.join(",")'), '1,2');
  assert.equal(page.run('frames.join(",")'), '16');
  assert.equal(page.run('idles'), 1);
  assert.notEqual(page.run('acmeNativeNow()'), epoch);
  assert.equal(page.run('performance.now()'), 2000);
  assert.equal(page.run('new Event().timeStamp'), 2000);
  assert.equal(page.run('performance.getEntries().length'), 0);
  assert.equal(page.run('performance.mark("Acme").name'), 'Acme');
  assert.equal(page.run('performance.measure("Acme").duration'), 50);
  await page.act(4, 'fastForward', 86400000);
  assert.equal(page.run('ticks'), 3);
  await page.act(5, 'setFixedTime', epoch);
  await page.act(6, 'runFor', 1000);
  assert.equal(page.run('ticks'), 4);
  assert.equal(page.run('Date.now()'), epoch);
  await page.act(7, 'setSystemTime', epoch + 10000);
  assert.equal(page.run('Date.now()'), epoch + 10000);
  assert.equal(page.run('ticks'), 4);
  await page.act(8, 'resume');
  page.advanceNative(50);
  assert.equal(page.run('Date.now()'), epoch + 10050);
  const foreign = document([['install', 0, 42], ['pauseAt', 0, 42]]);
  assert.equal(foreign.run('Date.now()'), 42);
});

test('future documents reconstruct ordered history without firing old callbacks or duplicating an applied action', async () => {
  const history = [['install', 0, epoch], ['pauseAt', 0, epoch], ['runFor', 0, 2000]];
  const page = document(history);
  assert.equal(page.run('Date.now()'), epoch + 2000);
  page.run('globalThis.ticks=0;setTimeout(()=>ticks++,10)');
  page.bootstrap([...history, ['runFor', 0, 10]]);
  assert.equal(page.run('ticks'), 0);
  await page.act(4, 'runFor', 10);
  await page.act(4, 'runFor', 10);
  assert.equal(page.run('ticks'), 1);
  assert.equal(page.run('Date.now()'), epoch + 2010);
  await assert.rejects(page.act(6, 'runFor', 10), /CLOCK_PENDING/);
});

test('callback exceptions keep their one-dispatch fence and cancelled timers never fire', async () => {
  const page = document([['install', 0, epoch], ['pauseAt', 0, epoch]]);
  page.run('globalThis.calls=0;const id=setTimeout(()=>calls+=100,1);clearTimeout(id);const frame=requestAnimationFrame(()=>calls+=1000);cancelAnimationFrame(frame);const idle=requestIdleCallback(()=>calls+=10000);cancelIdleCallback(idle);setTimeout(()=>{calls++;throw Error("Acme callback")},1)');
  await assert.rejects(page.act(3, 'runFor', 16), /Acme callback/);
  await page.act(3, 'runFor', 16);
  assert.equal(page.run('calls'), 1);
});

test('first current-document actions execute rather than reconstructing a future-document snapshot', async () => {
  const page = document();
  await page.act(1, 'install', epoch);
  assert.equal(page.run('Date.now()'), epoch);
  await page.act(2, 'setFixedTime', epoch);
  await page.act(3, 'pauseAt', epoch);
  await assert.rejects(page.act(4, 'pauseAt', epoch - 1), /past/);
});

test('zero-delay intervals let both advance actions settle while remaining active', async () => {
  for (const method of ['runFor', 'fastForward']) {
    const page = document([['install', 0, epoch], ['pauseAt', 0, epoch]]);
    // Bound the old same-tick loop without relying on a wall-time timeout that
    // an endless microtask queue could starve.
    page.run('globalThis.calls=[];globalThis.zeroCalls=0;globalThis.interval=setInterval(()=>{const tick=performance.now();calls.push(tick);if(tick===0&&++zeroCalls===3)clearInterval(interval)},0)');
    await page.act(3, method, 0);
    assert.equal(page.run('calls.length'), 0, method);
    await page.act(4, method, 5);
    assert.equal(page.run('calls.length'), method === 'runFor' ? 5 : 1, method);
    assert.equal(page.run('AcmeClock.controller.countTimers()'), 1, method);
    page.run('clearInterval(interval)');
  }
});

test('a paused future document initializes timeOrigin before its first script and schedules no native wakeup', async () => {
  const page = document([['install', 0, epoch], ['pauseAt', 0, epoch]]);
  page.run('globalThis.firstOrigin=performance.timeOrigin;globalThis.calls=0;setTimeout(()=>calls++,0)');
  page.advanceNative(100);
  await Promise.resolve();
  assert.equal(page.run('JSON.stringify([firstOrigin,calls])'), JSON.stringify([epoch, 0]));
  await page.act(3, 'runFor', 0);
  assert.equal(page.run('calls'), 1);
});

test('pause history preserves the same fixed Date, elapsed ticks and origin as live dispatch', async () => {
  const history = [['install', 0, epoch], ['setFixedTime', 0, epoch]];
  const live = document(history);
  await live.act(3, 'pauseAt', epoch + 100);
  await live.act(4, 'runFor', 10);
  const read = 'JSON.stringify([Date.now(),performance.now(),performance.timeOrigin])';
  assert.equal(live.run(read), JSON.stringify([epoch, 110, epoch]));
  const future = document([...history, ['pauseAt', 0, epoch + 100], ['runFor', 0, 10]]);
  assert.equal(future.run(read), live.run(read));
});

test('overlapping startup registrations keep cached timer APIs virtual until the new action applies', async () => {
  const history = [['install', 0, epoch], ['setFixedTime', 0, epoch], ['pauseAt', 0, epoch]];
  const page = document(history);
  page.bootstrap([...history, ['runFor', 0, 10]]);
  page.run('globalThis.started=Date.now();globalThis.calls=0;globalThis.cachedTimeout=setTimeout;cachedTimeout(()=>calls++,0)');
  page.advanceNative(100);
  assert.equal(page.run('started'), epoch);
  assert.equal(page.run('calls'), 0);
  await page.act(4, 'runFor', 10);
  assert.equal(page.run('calls'), 1);
  assert.equal(page.run('performance.now()'), 10);
});

test('the installed comparison scenario tolerates different real elapsed time before pause', async () => {
  const steps = JSON.parse(await readFile(new URL('../tests/fixtures/browser-clock-scenario.json', import.meta.url)));
  const exercise = async elapsed => {
    const page = document(), history = [], report = [];
    for (const step of steps) {
      if (step.action) {
        const {kind,timeMs,durationMs} = step.action;
        if (kind === 'pause_at') page.advanceNative(elapsed);
        const method = kind.replace(/_([a-z])/g, (_,letter) => letter.toUpperCase());
        const parameter = timeMs ?? durationMs ?? null;
        await page.act(history.length + 1, method, parameter);
        history.push([method, 0, parameter]);
      } else {
        const selected = step.future ? document(history) : page;
        const value = selected.run(`(${step.future ?? step.evaluate})()`);
        if (value && typeof value.then === 'function') selected.advanceNative(10);
        report.push(JSON.parse(await value));
      }
    }
    return report;
  };
  const cold = await exercise(0), delayed = await exercise(120);
  assert.deepEqual(delayed, cold);
  assert.equal(cold[1].performance, 2000);
  assert.deepEqual(cold[1].frames, {count:1,aligned:true});
});
