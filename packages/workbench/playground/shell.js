/** Canvas playground shell: the permanent part. Owns the camera (Workbench's own geometry), the
 *  real dot grid, the HUD and each scenario's lifecycle. Scenarios are swapped and hot-reloaded
 *  without reloading the page; see README.md for the scenario contract. */
import { fit, zoom } from '../ui/src/modules/canvas/geometry.js';
import { createGrid } from '../ui/src/modules/canvas/grid.js';

const $ = id => document.getElementById(id);
const root = $('playground'), stage = $('stage'), gridSurface = $('grid'), world = $('world'), overlay = $('overlay');
const overlayContext = overlay.getContext('2d');
const store = {
  get(key, fallback) { try { const v = localStorage.getItem('playground:' + key); return v === null ? fallback : JSON.parse(v); } catch { return fallback; } },
  set(key, value) { try { localStorage.setItem('playground:' + key, JSON.stringify(value)); } catch { /* storage unavailable: per-session only */ } },
};

// Workbench's real canvas tokens, read from its stylesheet through a hidden `.wb` element.
const TOKENS = ['--wb-panel', '--wb-canvas', '--wb-float', '--wb-float-line', '--wb-raised', '--wb-hover', '--wb-selected', '--wb-line',
  '--wb-fg', '--wb-fg-2', '--wb-fg-3', '--wb-accent', '--wb-accent-strong', '--wb-accent-text', '--wb-warning'];
async function adoptTokens() {
  const link = $('tokens');
  if (!link.sheet) await new Promise(resolve => { link.addEventListener('load', resolve, { once: true }); link.addEventListener('error', resolve, { once: true }); });
  const style = getComputedStyle($('token-source'));
  for (const name of TOKENS) { const value = style.getPropertyValue(name).trim(); if (value) document.documentElement.style.setProperty(name, value); }
}
function rgba(cssColor) {
  const c = new OffscreenCanvas(1, 1).getContext('2d', { willReadFrequently: true });
  c.fillStyle = cssColor; c.fillRect(0, 0, 1, 1);
  return [...c.getImageData(0, 0, 1, 1).data].map(v => v / 255);
}

// --- Camera: Workbench geometry and input conventions -------------------------------------------
const camera = { x: 32, y: 32, scale: 1 };
const MIN_SCALE = 0.05;
function setCamera(next) {
  Object.assign(camera, next, { scale: Math.max(MIN_SCALE, next.scale ?? camera.scale) });
  const dpr = devicePixelRatio || 1;
  camera.x = Math.round(camera.x * dpr) / dpr; camera.y = Math.round(camera.y * dpr) / dpr;
  if (active) store.set('camera:' + active.url, { ...camera });
  world.style.transform = `translate(${camera.x}px, ${camera.y}px) scale(${camera.scale})`;
  requestFrame();
}
function zoomAround(scale, point = { x: stage.clientWidth / 2, y: stage.clientHeight / 2 }) { setCamera(zoom(camera, Math.max(MIN_SCALE, scale), point)); }
/** The stage area not covered by HUD panels, so Fit frames content where it is visible. */
function freeArea() {
  const panels = !root.classList.contains('hide-panels'), left = panels ? $('scenarios-panel').getBoundingClientRect().right : 0;
  const right = panels ? stage.clientWidth - $('inspector-panel').getBoundingClientRect().left : 0, top = 64, bottom = 76;
  return { x: left, y: top, width: Math.max(200, stage.clientWidth - left - right), height: Math.max(200, stage.clientHeight - top - bottom) };
}
function fitScenario() {
  const rects = active?.instance?.bounds?.() ?? [], area = freeArea();
  if (!rects.length) { setCamera({ x: area.x + 32, y: area.y + 32, scale: 1 }); return; }
  const fitted = fit(rects.map(r => ({ x: r.x, y: r.y, size: { width: r.width, height: r.height } })), area);
  setCamera({ ...fitted, x: fitted.x + area.x, y: fitted.y + area.y });
}

let spaceHeld = false, drag = null;
const zoomWheel = (event, point) => zoomAround(camera.scale * Math.exp(-event.deltaY * 0.01), point);
stage.addEventListener('wheel', event => {
  event.preventDefault();
  const r = stage.getBoundingClientRect(), point = { x: event.clientX - r.left, y: event.clientY - r.top };
  if (event.ctrlKey || event.metaKey) zoomWheel(event, point);
  else setCamera({ x: camera.x - event.deltaX, y: camera.y - event.deltaY });
}, { passive: false });
function setSpace(held) { spaceHeld = held; stage.classList.toggle('can-pan', held); }
stage.addEventListener('pointerdown', event => {
  // Middle button, Space+drag, or a drag that starts on empty canvas pans; scenario content keeps its own input.
  const onBackground = event.target === stage || event.target === gridSurface || event.target === world;
  if (!(event.button === 1 || spaceHeld || (event.button === 0 && onBackground))) return;
  event.preventDefault(); stage.setPointerCapture(event.pointerId); stage.classList.add('panning');
  drag = { id: event.pointerId, x: event.clientX, y: event.clientY, camera: { ...camera } };
});
stage.addEventListener('pointermove', event => { if (drag?.id === event.pointerId) setCamera({ x: drag.camera.x + event.clientX - drag.x, y: drag.camera.y + event.clientY - drag.y }); });
for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) stage.addEventListener(type, () => { drag = null; stage.classList.remove('panning'); });

// Duck-typed so elements from iframe documents (another realm) count too.
const editable = target => !!target && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName));
/** Canvas navigation keys: Space for panning and ⌘/Ctrl + `=` / `-` / `0`. Returns whether it handled the event. */
function navigationKey(event) {
  if (editable(event.target)) return false;
  const mod = event.metaKey || event.ctrlKey;
  if (event.code === 'Space') { if (!event.repeat) setSpace(true); event.preventDefault(); return true; }
  if (mod && event.key === '0') { event.preventDefault(); zoomAround(1); return true; }
  if (mod && (event.key === '=' || event.key === '+')) { event.preventDefault(); zoomAround(camera.scale * 1.25); return true; }
  if (mod && event.key === '-') { event.preventDefault(); zoomAround(camera.scale * 0.8); return true; }
  return false;
}
addEventListener('keydown', event => {
  if (editable(event.target) || navigationKey(event)) return;
  const mod = event.metaKey || event.ctrlKey;
  if (mod) return;
  else if (event.shiftKey && event.code === 'Digit1') fitScenario();
  else if (event.key === 'r' || event.key === 'R') void reload(event.shiftKey);
  else if (event.key === '[') step(-1);
  else if (event.key === ']') step(1);
  else if (event.key === 'Tab') { event.preventDefault(); root.classList.toggle('hide-panels'); store.set('panels', !root.classList.contains('hide-panels')); }
  else if (/^[1-9]$/.test(event.key) && !event.shiftKey) { const s = visibleScenarios()[Number(event.key) - 1]; if (s) void open(s.url); }
});
addEventListener('keyup', event => { if (event.code === 'Space') setSpace(false); });

// Scenario iframes receive their own input, so canvas navigation would stop at their edges.
// Same-origin iframes (srcdoc and pages this server serves) get listeners in their window that
// forward Space and ⌘/Ctrl + wheel to the canvas; their content keeps every other input. While
// Space is held, CSS turns off pointer events on the world so the drag reaches the stage.
// Cross-origin documents cannot be reached from here.
const spaceUp = event => { if (event.code === 'Space') setSpace(false); }, frameWheels = new WeakMap();
function bridge(frame) {
  let win; try { win = frame.contentWindow; if (!win?.document) return; } catch { return; }
  let wheel = frameWheels.get(frame);
  if (!wheel) frameWheels.set(frame, wheel = event => {
    if (!(event.ctrlKey || event.metaKey)) return; // plain scrolling stays with the content
    event.preventDefault();
    // Iframe client coordinates are in its own CSS pixels, before the world transform.
    const f = frame.getBoundingClientRect(), s = stage.getBoundingClientRect(), k = f.width / (frame.clientWidth || f.width);
    zoomWheel(event, { x: f.left + (frame.clientLeft + event.clientX) * k - s.left, y: f.top + (frame.clientTop + event.clientY) * k - s.top });
  });
  // The same listener objects each time, so a window that survives a load is not bridged twice.
  win.addEventListener('keydown', navigationKey, true);
  win.addEventListener('keyup', spaceUp, true);
  win.addEventListener('wheel', wheel, { capture: true, passive: false });
}
function watchFrame(frame) { frame.addEventListener('load', () => bridge(frame)); bridge(frame); }
new MutationObserver(records => {
  for (const record of records) for (const node of record.addedNodes) {
    if (node.nodeName === 'IFRAME') watchFrame(node);
    else if (node.querySelectorAll) node.querySelectorAll('iframe').forEach(watchFrame);
  }
}).observe(world, { childList: true, subtree: true });

// --- Frame loop: on demand, or continuous while a scenario animates ----------------------------
let device, format, gpuContext, grid, colors, frameHandle = 0, animating = false, lastFrame = 0;
const frameTimes = [], presented = [];
function requestFrame() { if (!frameHandle) frameHandle = requestAnimationFrame(frame); }
function resize() {
  const dpr = devicePixelRatio || 1, w = stage.clientWidth, h = stage.clientHeight;
  const pw = Math.max(1, Math.round(w * dpr)), ph = Math.max(1, Math.round(h * dpr));
  for (const surface of [gridSurface, overlay]) if (surface.width !== pw || surface.height !== ph) { surface.width = pw; surface.height = ph; }
  return { width: w, height: h, dpr };
}
function frame(time) {
  frameHandle = 0;
  const began = performance.now(), size = resize();
  world.style.transform = `translate(${camera.x}px, ${camera.y}px) scale(${camera.scale})`;
  const info = { camera: { ...camera }, ...size, time, overlay: overlayContext };
  overlayContext.setTransform(1, 0, 0, 1, 0, 0); overlayContext.clearRect(0, 0, overlay.width, overlay.height);
  if (device) {
    // The grid and the scenario are separate submissions, so an invalid scenario command buffer
    // (a WGSL or validation error) cannot take the grid down with it.
    const view = gpuContext.getCurrentTexture().createView();
    const encoder = device.createCommandEncoder(), pass = encoder.beginRenderPass({ colorAttachments: [{ view, clearValue: colors.background, loadOp: 'clear', storeOp: 'store' }] });
    grid.draw(pass, camera, size.dpr, colors.background, colors.line);
    pass.end(); device.queue.submit([encoder.finish()]);
    if (active?.instance?.render) {
      const scene = device.createCommandEncoder(), scenePass = scene.beginRenderPass({ colorAttachments: [{ view, loadOp: 'load', storeOp: 'store' }] });
      guard(() => active.instance.render(scenePass, info));
      scenePass.end(); device.queue.submit([scene.finish()]);
    }
  }
  guard(() => active?.instance?.frame?.(info));
  const ms = performance.now() - began;
  frameTimes.push(ms); if (frameTimes.length > 120) frameTimes.shift();
  lastFrame = ms;
  if (animating) requestFrame();
}
function guard(fn) { try { return fn(); } catch (error) { showError(error); } }

// Display FPS (presented animation frames) runs independently of scenario frames.
(function tick(now) { presented.push(now); requestAnimationFrame(tick); })(performance.now());
setInterval(() => {
  const now = performance.now(); while (presented.length && now - presented[0] > 1000) presented.shift();
  const fps = presented.length, el = $('stat-fps'); el.textContent = String(fps); el.className = fps >= 55 ? 'good' : fps >= 30 ? 'warn' : 'bad';
  $('stat-frame').textContent = lastFrame ? lastFrame.toFixed(1) + 'ms' : '—';
  $('stat-zoom').textContent = Math.round(camera.scale * 100) + '%'; $('stat-dpr').textContent = String(devicePixelRatio);
  $('zoom-label').value = Math.round(camera.scale * 100) + '%'; $('zoom').value = String(Math.log2(camera.scale));
  const spark = $('spark'), c = spark.getContext('2d'), max = Math.max(16.7, ...frameTimes);
  c.clearRect(0, 0, spark.width, spark.height);
  c.strokeStyle = 'rgb(255 255 255 / 0.12)'; c.beginPath(); c.moveTo(0, spark.height - 16.7 / max * spark.height); c.lineTo(spark.width, spark.height - 16.7 / max * spark.height); c.stroke();
  c.strokeStyle = getComputedStyle(root).getPropertyValue('--pg-accent'); c.beginPath();
  frameTimes.forEach((t, i) => { const x = i / 119 * spark.width, y = spark.height - t / max * spark.height; i ? c.lineTo(x, y) : c.moveTo(x, y); }); c.stroke();
}, 250);

// --- HUD helpers -----------------------------------------------------------------------------
function toast(text) { const t = $('toast'); t.hidden = true; void t.offsetWidth; t.textContent = text; t.hidden = false; }
// The first error is usually the cause; later ones (such as invalid command buffers) follow from it.
let firstError = null, laterErrors = 0;
function showError(error) {
  // GPU errors are not Error instances: they carry only a message.
  const text = error?.stack ?? (error?.message ? `${error.constructor?.name ?? 'Error'}: ${error.message}` : String(error));
  if ($('error').hidden) { firstError = text; laterErrors = 0; } else laterErrors++;
  $('error-text').textContent = firstError + (laterErrors ? `\n\n…and ${laterErrors} later error${laterErrors > 1 ? 's' : ''} (latest: ${text.split('\n')[0]})` : '');
  $('error').hidden = false;
  setStatus('error', 'Error');
}
$('error-close').onclick = () => { $('error').hidden = true; };
function setStatus(state, text) { const s = $('scenario-status'); s.dataset.state = state; s.textContent = text; }

// --- Scenarios --------------------------------------------------------------------------------
let list = [], active = null, generation = 0, host = { owned: false, paintRecord: false };
const meta = new Map();
function visibleScenarios() { const q = $('search').value.trim().toLowerCase(); return list.filter(s => !q || (meta.get(s.url)?.title ?? s.file).toLowerCase().includes(q) || s.file.toLowerCase().includes(q)); }
function renderList() {
  const shown = visibleScenarios(), ol = $('scenario-list');
  $('scenario-count').textContent = String(list.length);
  ol.replaceChildren(...shown.map((s, i) => {
    const li = document.createElement('li'), b = document.createElement('button');
    b.type = 'button'; b.setAttribute('aria-current', String(active?.url === s.url)); b.title = s.file;
    b.innerHTML = `<span class="index">${String(i + 1).padStart(2, '0')}</span><span class="name"></span><span class="source"></span>`;
    b.querySelector('.name').textContent = meta.get(s.url)?.title ?? s.file;
    const src = b.querySelector('.source'); src.textContent = s.source === 'built-in' ? 'core' : s.source; src.classList.toggle('local', s.source !== 'built-in');
    b.onclick = () => void open(s.url); li.append(b); return li;
  }));
}
$('search').addEventListener('input', renderList);
function step(delta) {
  const shown = visibleScenarios(); if (!shown.length) return;
  const at = shown.findIndex(s => s.url === active?.url);
  void open(shown[(at + delta + shown.length) % shown.length].url);
}
async function load(url, version) { return (await import(`${url}?v=${version}`)).default; }
async function refreshList() {
  list = await (await fetch('/api/scenarios')).json();
  await Promise.all(list.map(async s => { if (!meta.has(s.url)) { try { const m = await load(s.url, 0); meta.set(s.url, { title: m?.title ?? s.file, description: m?.description ?? '' }); } catch { meta.set(s.url, { title: s.file + ' (failed to load)', description: '' }); } } }));
  renderList();
}

/** Builds the per-scenario context: layers, controls, readouts, host actions and cleanup. */
function context(url, signal) {
  const controls = $('controls'), readouts = $('readouts'), rows = new Map();
  controls.replaceChildren(); readouts.replaceChildren();
  // A scenario replaced while its async setup runs keeps working on detached elements.
  const add = el => { if (!signal.aborted) controls.append(el); return el; };
  const host_ = async (route, body, timeout = 120000, cancel = null) => {
    // A stalled host action fails instead of leaving the scenario waiting forever; aborting the scenario
    // or the caller's own signal cancels it (the server then drops a queued render).
    const res = await fetch(route, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body ?? {}),
      signal: AbortSignal.any([signal, AbortSignal.timeout(timeout), ...(cancel ? [cancel] : [])]) });
    if (!res.ok) throw Error((await res.json().catch(() => ({}))).error ?? res.statusText);
    return res;
  };
  return {
    url, signal, world, stage,
    get camera() { return { ...camera }; },
    get dpr() { return devicePixelRatio || 1; },
    get gpu() { return device ? { device, format } : null; },
    requestFrame, animate(on) { animating = !!on; requestFrame(); },
    setCamera, fit: fitScenario,
    screenToWorld(x, y) { return { x: (x - camera.x) / camera.scale, y: (y - camera.y) / camera.scale }; },
    worldToScreen(x, y) { return { x: x * camera.scale + camera.x, y: y * camera.scale + camera.y }; },
    controls: {
      button(label, onClick) { const b = add(document.createElement('button')); b.type = 'button'; b.textContent = label; b.onclick = () => guard(onClick); return b; },
      toggle(label, value, onChange) {
        const row = add(document.createElement('label')); row.className = 'row'; row.textContent = label;
        const box = document.createElement('input'); box.type = 'checkbox'; box.checked = value; box.onchange = () => guard(() => onChange(box.checked)); row.append(box); return box;
      },
      select(label, options, value, onChange) {
        const row = add(document.createElement('label')); row.textContent = label; const sel = document.createElement('select');
        for (const o of options) { const opt = document.createElement('option'); opt.value = opt.textContent = o; sel.append(opt); }
        sel.value = value; sel.onchange = () => guard(() => onChange(sel.value)); row.append(sel); return sel;
      },
      segmented(label, options, value, onChange) {
        const row = add(document.createElement('div')); row.setAttribute('role', 'group'); row.setAttribute('aria-label', label);
        const cap = document.createElement('label'); cap.textContent = label; const group = document.createElement('div'); group.className = 'segmented';
        const set = v => { for (const b of group.children) b.setAttribute('aria-pressed', String(b.dataset.value === v)); };
        for (const o of options) { const b = document.createElement('button'); b.type = 'button'; b.dataset.value = b.textContent = o; b.onclick = () => { set(o); guard(() => onChange(o)); }; group.append(b); }
        set(value); row.append(cap, group); return { set };
      },
      slider(label, { min = 0, max = 1, step = 0.01, value = 0 }, onChange) {
        const row = add(document.createElement('label')); row.textContent = label; const input = document.createElement('input');
        Object.assign(input, { type: 'range', min, max, step, value }); input.oninput = () => guard(() => onChange(Number(input.value))); row.append(input); return input;
      },
    },
    stats: {
      set(key, value) {
        if (signal.aborted) return;
        let row = rows.get(key);
        if (!row) { const dt = document.createElement('dt'), dd = document.createElement('dd'); dt.textContent = key; readouts.append(dt, dd); row = dd; rows.set(key, dd); }
        row.textContent = typeof value === 'number' ? (Number.isInteger(value) ? String(value) : value.toFixed(2)) : String(value);
      },
      clear() { readouts.replaceChildren(); rows.clear(); },
    },
    host: {
      get owned() { return host.owned; }, get paintRecord() { return host.paintRecord; },
      /** Forces Chrome to produce a new frame (commits pending paint). */
      async frame() { await host_('/api/frame'); },
      /** Native screenshot of a CSS-pixel rectangle at `scale` × CSS pixels; resolves to a PNG Blob. */
      async capture(rect, scale = 0) {
        // The HUD floats over the stage; hide it so the capture holds only canvas content.
        root.classList.add('capturing');
        try { await host_('/api/frame'); return (await host_('/api/capture', { ...rect, scale })).blob(); }
        finally { root.classList.remove('capturing'); }
      },
      /** Number of background renderer pages working in parallel. */
      get renderers() { return host.renderers ?? 1; },
      /** Server-side queue counters: renderers, queued, running, done, cancelled, discarded, failed. */
      async renderStats() { return (await fetch('/api/render-stats')).json(); },
      /** Renders an HTML document laid out at width × height in the background renderer pool, without
       *  touching the visible window: the whole page at `scale` device pixels per CSS pixel, or only
       *  `clip` ({x, y, width, height} in CSS pixels) magnified by `scale`. Aborting `signal` cancels
       *  the job. Resolves to `{png: Blob, ms, phases}`; `phases` has the server's queue, load,
       *  ready and screenshot milliseconds. */
      async render({ html, width, height, scale, clip, signal: cancel }) {
        const res = await host_('/api/render', { html, width, height, scale, clip }, 30000, cancel);
        return { png: await res.blob(), ms: Number(res.headers.get('x-render-ms')), phases: JSON.parse(res.headers.get('x-render-phases') ?? 'null') };
      },
      /** Chrome's serialized paint record for every layer, plus the layer tree. Needs --paint-record. */
      async paintRecord() { return (await host_('/api/paint-record')).json(); },
    },
  };
}
async function open(url, { keepCamera = false } = {}) {
  const version = ++generation;
  await close();
  const entry = list.find(s => s.url === url); if (!entry) return;
  active = { url, instance: null, abort: new AbortController() };
  store.set('scenario', url); renderList(); setStatus('loading', 'Loading');
  $('error').hidden = true;
  try {
    const scenario = await load(url, version);
    if (version !== generation) return;
    meta.set(url, { title: scenario?.title ?? entry.file, description: scenario?.description ?? '' });
    $('scene-title').textContent = meta.get(url).title; $('scenario-description').textContent = meta.get(url).description || entry.file;
    renderList();
    const current = active, ctx = context(url, current.abort.signal);
    const instance = (await scenario.setup?.(ctx)) ?? {};
    // Replaced while setting up: release what this setup created.
    if (version !== generation) { try { await instance.dispose?.(); } catch (error) { showError(error); } return; }
    current.instance = instance;
    if (!keepCamera) { const saved = store.get('camera:' + url, null); if (saved) setCamera(saved); else fitScenario(); }
    setStatus('running', 'Running'); requestFrame();
  } catch (error) { if (version === generation) showError(error); }
}
async function close() {
  if (!active) return;
  const current = active; active = null; animating = false;
  current.abort.abort();
  try { await current.instance?.dispose?.(); } catch (error) { showError(error); }
  world.replaceChildren(); $('controls').replaceChildren(); $('readouts').replaceChildren();
  setStatus('idle', 'Idle'); requestFrame();
}
async function reload(full = false) {
  if (full) { location.reload(); return; }
  if (!active) return;
  const url = active.url; await open(url, { keepCamera: true }); toast('Reloaded ' + (meta.get(url)?.title ?? url));
}

$('zoom-out').onclick = () => zoomAround(camera.scale * 0.8);
$('zoom-in').onclick = () => zoomAround(camera.scale * 1.25);
$('zoom').oninput = () => zoomAround(2 ** Number($('zoom').value));
$('fit').onclick = fitScenario;
$('actual').onclick = () => zoomAround(1);
$('reload').onclick = () => void reload();
new ResizeObserver(requestFrame).observe(stage);
matchMedia(`(resolution: ${devicePixelRatio}dppx)`).addEventListener('change', requestFrame);

// Hot reload: the server reports changed files; reload the active scenario when its folder changes.
const events = new EventSource('/api/events');
events.onmessage = ({ data }) => {
  const { url } = JSON.parse(data);
  void refreshList();
  if (active && url.slice(0, url.lastIndexOf('/')) === active.url.slice(0, active.url.lastIndexOf('/'))) void reload();
};

// --- Start ------------------------------------------------------------------------------------
await adoptTokens();
const tokens = getComputedStyle(document.documentElement);
colors = { background: rgba(tokens.getPropertyValue('--wb-canvas') || '#161616'), line: rgba(tokens.getPropertyValue('--wb-line') || '#383838') };
if (store.get('panels', true) === false) root.classList.add('hide-panels');
try {
  if (!navigator.gpu) throw Error('The playground needs WebGPU (Chrome or Electron).');
  const adapter = await navigator.gpu.requestAdapter(); if (!adapter) throw Error('No WebGPU adapter is available.');
  device = await adapter.requestDevice({ requiredLimits: { maxTextureDimension2D: adapter.limits.maxTextureDimension2D } });
  device.lost.then(info => showError(Error('GPU device lost: ' + info.message)));
  device.addEventListener('uncapturederror', event => showError(event.error));
  format = navigator.gpu.getPreferredCanvasFormat();
  gpuContext = gridSurface.getContext('webgpu'); gpuContext.configure({ device, format, alphaMode: 'opaque', colorSpace: 'srgb' });
  grid = createGrid(device, format);
  const info = adapter.info ?? {};
  host = await (await fetch('/api/host')).json();
  $('host').innerHTML = `<b>●</b> WebGPU ${[info.vendor, info.architecture].filter(Boolean).join(' ') || 'adapter'} · ${host.owned ? 'owned Chrome' : 'external browser'}${host.paintRecord ? ' · paint records on' : ''}`;
} catch (error) { showError(error); $('host').textContent = 'WebGPU unavailable'; }
await refreshList();
const last = store.get('scenario', null);
await open(list.some(s => s.url === last) ? last : list[0]?.url);
requestFrame();
