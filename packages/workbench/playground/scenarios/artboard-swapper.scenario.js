/** Many artboards, each shown as the cheapest representation that looks right:
 *  - every visible artboard draws its best cached PNG (WebGPU), or a placeholder until one exists;
 *  - missing resolutions come from a three-tier cache (GPU textures, encoded PNG sheets in memory,
 *    IndexedDB keyed by content hash), and only true misses are rendered;
 *  - a priority queue renders those in the background renderer pool, nearest the centre first,
 *    cheap low-resolution first, and cancels work for artboards that leave the view;
 *  - once artboards are large on screen, the few nearest the centre become live iframes. An iframe
 *    stays hidden over its PNG until it has loaded and painted, so the swap is not visible. */
const W = 640, H = 480, GAP = 80, ROW_GAP = 112;
const LEVELS = [0.125, 0.25, 0.5, 1, 2, 4];
const MiB = 1048576, GPU_BUDGET = 768 * MiB, MEMORY_BUDGET = 256 * MiB, DISK_BUDGET = 1024 * MiB;
const MARGIN = 0.25, POOL_EXTRA = 4, SHEET_PIXELS = 1_000_000, SHEET_MAX = 16;

function simple(i) {
  const hue = (i * 47) % 360, accent = (hue + 40) % 360;
  return `<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;width:${W}px;height:${H}px;overflow:hidden;font:15px/1.45 system-ui;color:#1c2230;background:linear-gradient(160deg,hsl(${hue} 70% 94%),#fff 60%)}
.bar{height:56px;background:hsl(${hue} 55% 35%);color:#fff;display:flex;align-items:center;padding:0 20px;font-weight:700;font-size:18px}
.card{margin:24px;padding:20px;border-radius:14px;background:#fff;box-shadow:0 8px 24px rgb(0 0 0/.12)}
h1{margin:0 0 6px;font-size:26px}.row{display:flex;gap:12px;margin-top:14px}.row i{flex:1;height:70px;border-radius:10px;background:hsl(${accent} 70% 80%)}
button{font:600 14px system-ui;color:#fff;background:hsl(${hue} 60% 45%);border:0;border-radius:8px;padding:9px 16px;cursor:pointer}
</style><div class="bar">Acme · Screen ${i + 1}</div><div class="card"><h1>Dashboard ${i + 1}</h1>
<p>Artboard ${i + 1}, rendered as a PNG by the background pool or shown live as an iframe.</p>
<div class="row"><i></i><i></i><i></i></div><p><button onclick="this.textContent='Clicked ✓'">Action</button></p></div>`;
}

const shader = `
struct View { camera: vec4f, size: vec4f };
@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var linearSampler: sampler;
struct Out { @builtin(position) position: vec4f, @location(0) uv: vec2f, @location(1) tint: vec4f };
@vertex fn vertex(@builtin(vertex_index) v: u32, @location(0) rect: vec4f, @location(1) tint: vec4f) -> Out {
  let c = array<vec2f,6>(vec2f(0,0), vec2f(1,0), vec2f(0,1), vec2f(0,1), vec2f(1,0), vec2f(1,1))[v];
  let screen = (rect.xy + c * rect.zw) * view.camera.z + view.camera.xy;
  var out: Out; out.position = vec4f(screen.x / view.size.x * 2 - 1, 1 - screen.y / view.size.y * 2, 0, 1); out.uv = c; out.tint = tint; return out;
}
@fragment fn fragment(in: Out) -> @location(0) vec4f { return textureSample(image, linearSampler, in.uv) * in.tint; }`;

/** IndexedDB tier: `sheets` holds encoded PNG sheets by id, `entries` maps `<content hash>:<scale>`
 *  to a sheet and the artboard's rectangle in it. Content hashes make stale entries impossible. */
const idb = request => new Promise((resolve, reject) => { request.onsuccess = () => resolve(request.result); request.onerror = () => reject(request.error); });
async function openDisk() {
  const request = indexedDB.open('playground-artboard-swapper', 1);
  request.onupgradeneeded = () => { request.result.createObjectStore('sheets'); request.result.createObjectStore('entries'); };
  const db = await idb(request), sizes = new Map();
  await new Promise((resolve, reject) => {
    const cursor = db.transaction('sheets').objectStore('sheets').openCursor();
    cursor.onsuccess = () => { const c = cursor.result; if (!c) return resolve(); sizes.set(c.key, { size: c.value.size, time: c.value.time }); c.continue(); };
    cursor.onerror = () => reject(cursor.error);
  });
  return { db, sizes };
}

export default {
  title: 'Artboard swapper',
  description: 'Hundreds of artboards in ten heavy themes (or simple pages). Each shows its best cached PNG at once (or a placeholder); missing resolutions come from memory or IndexedDB caches, or render in the background pool and are cancelled when the artboard leaves view; the few artboards nearest the centre swap to live iframes once they are large on screen.',
  async setup(ctx) {
    const gpu = ctx.gpu; if (!gpu) throw Error('WebGPU is unavailable.');
    // Imported with this module's version, so saving the themes reloads them with the scenario.
    const { wild } = await import(`./assets/wild.js?v=${new URL(import.meta.url).searchParams.get('v') ?? ''}`);
    let content = 'wild';
    const html = i => content === 'wild' ? wild(i, W, H) : simple(i);
    const { device, format } = gpu;
    const module = device.createShaderModule({ code: shader });
    const compiled = (await module.getCompilationInfo()).messages.filter(m => m.type === 'error');
    if (compiled.length) throw Error('WGSL: ' + compiled.map(m => `${m.lineNum}:${m.linePos} ${m.message}`).join('\n'));
    const viewLayout = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: GPUShaderStage.VERTEX, buffer: {} }] });
    const imageLayout = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: GPUShaderStage.FRAGMENT, texture: {} }, { binding: 1, visibility: GPUShaderStage.FRAGMENT, sampler: {} }] });
    const pipeline = device.createRenderPipeline({ layout: device.createPipelineLayout({ bindGroupLayouts: [viewLayout, imageLayout] }),
      vertex: { module, entryPoint: 'vertex', buffers: [{ arrayStride: 32, stepMode: 'instance', attributes: [{ shaderLocation: 0, offset: 0, format: 'float32x4' }, { shaderLocation: 1, offset: 16, format: 'float32x4' }] }] },
      fragment: { module, entryPoint: 'fragment', targets: [{ format, blend: { color: { srcFactor: 'src-alpha', dstFactor: 'one-minus-src-alpha' }, alpha: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha' } } }] },
      primitive: { topology: 'triangle-list' } });
    const viewBuffer = device.createBuffer({ size: 32, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
    const viewGroup = device.createBindGroup({ layout: viewLayout, entries: [{ binding: 0, resource: { buffer: viewBuffer } }] });
    const sampler = device.createSampler({ magFilter: 'linear', minFilter: 'linear' });
    const white = device.createTexture({ size: [1, 1], format: 'rgba8unorm', usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST });
    device.queue.writeTexture({ texture: white }, new Uint8Array([255, 255, 255, 255]), {}, [1, 1]);
    const whiteGroup = device.createBindGroup({ layout: imageLayout, entries: [{ binding: 0, resource: white.createView() }, { binding: 1, resource: sampler }] });
    let instances = null;

    let count = 1000, columns = 0, maxLive = 5, liveEnter = 320, lowFirst = true, showKind = true, useDisk = true;
    const textures = new Map(), have = [], jobs = new Map(), batches = new Set(), frames = new Map(), latencies = [];
    let bytes = 0, clock = 0, cancelled = 0, failed = 0, server = {}, recent = [];
    const filled = { memory: 0, disk: 0, render: 0 };
    const timings = []; globalThis.swapperTimings = timings; // per-batch phase timings, read by checks
    const key = (i, level) => i * 8 + level;
    const boardAt = i => ({ x: (i % columns) * (W + GAP), y: Math.floor(i / columns) * (H + ROW_GAP) });
    const desired = () => { const need = ctx.camera.scale * ctx.dpr; const k = LEVELS.findIndex(l => l >= need * 0.999); return k < 0 ? LEVELS.length - 1 : k; };
    /** Best cached level for an artboard: the smallest at or above `want`, else the largest below. */
    function best(i, want) {
      const levels = have[i]; if (!levels) return -1;
      for (let l = want; l < LEVELS.length; l++) if (levels & (1 << l)) return l;
      for (let l = want - 1; l >= 0; l--) if (levels & (1 << l)) return l;
      return -1;
    }
    /** Artboard indices intersecting the view, expanded by `margin` of its size on each side. */
    function inView(margin = 0) {
      const w = ctx.stage.clientWidth, h = ctx.stage.clientHeight;
      const a = ctx.screenToWorld(-w * margin, -h * margin), b = ctx.screenToWorld(w * (1 + margin), h * (1 + margin));
      const c0 = Math.max(0, Math.floor(a.x / (W + GAP))), c1 = Math.min(columns - 1, Math.floor(b.x / (W + GAP)));
      const rows = Math.ceil(count / columns), r0 = Math.max(0, Math.floor(a.y / (H + ROW_GAP))), r1 = Math.min(rows - 1, Math.floor(b.y / (H + ROW_GAP)));
      const out = [];
      for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) { const i = r * columns + c; if (i < count) out.push(i); }
      return out;
    }
    function distanceToCentre(i) {
      const p = boardAt(i), s = ctx.worldToScreen(p.x + W / 2, p.y + H / 2);
      return Math.hypot(s.x - ctx.stage.clientWidth / 2, s.y - ctx.stage.clientHeight / 2);
    }

    // --- Content hashes: cache keys follow the artboard's HTML, not its position ---------------
    const hashes = []; let hashing = Promise.resolve();
    async function hashAll() {
      const encoder = new TextEncoder();
      const list = await Promise.all(Array.from({ length: count }, async (_, i) => {
        const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', encoder.encode(html(i))));
        return Array.from(digest.subarray(0, 12), b => b.toString(16).padStart(2, '0')).join('');
      }));
      hashes.length = 0; hashes.push(...list);
    }
    const cacheKey = (i, level) => hashes[i] && `${hashes[i]}:${LEVELS[level]}`;

    // --- Tier 1: GPU textures. Largest and off-screen go first; the 0.125× base is never evicted --
    function store(i, level, bitmap) {
      const texture = device.createTexture({ size: [bitmap.width, bitmap.height], format: 'rgba8unorm', usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST | GPUTextureUsage.RENDER_ATTACHMENT });
      device.queue.copyExternalImageToTexture({ source: bitmap }, { texture }, [bitmap.width, bitmap.height]);
      const size = bitmap.width * bitmap.height * 4; bitmap.close();
      const group = device.createBindGroup({ layout: imageLayout, entries: [{ binding: 0, resource: texture.createView() }, { binding: 1, resource: sampler }] });
      drop(key(i, level));
      textures.set(key(i, level), { i, level, texture, group, size, used: ++clock }); bytes += size;
      have[i] = (have[i] ?? 0) | (1 << level);
      if (bytes > GPU_BUDGET) evict(new Set(inView(MARGIN)));
    }
    function drop(k) { const t = textures.get(k); if (!t) return; t.texture.destroy(); bytes -= t.size; textures.delete(k); have[t.i] &= ~(1 << t.level); }
    function evict(keep) {
      const want = desired();
      const order = [...textures].filter(([, t]) => t.level > 0)
        .sort(([, a], [, b]) => (keep.has(a.i) - keep.has(b.i)) || (b.size - a.size) || (a.used - b.used));
      for (const [k, t] of order) { if (bytes <= GPU_BUDGET * 0.85) break; if (!keep.has(t.i) || t.level > want) drop(k); }
    }

    // --- Tier 2: encoded PNG sheets in memory (LRU) ------------------------------------------
    const sheets = new Map(), entries = new Map(); let sheetBytes = 0;
    function remember(id, blob, items) {
      if (!sheets.has(id)) { sheets.set(id, { blob, used: ++clock }); sheetBytes += blob.size; }
      for (const item of items) { const k = cacheKey(item.i, item.level); if (k) entries.set(k, { sheet: id, x: item.x, y: item.y, width: item.width, height: item.height }); }
      if (sheetBytes <= MEMORY_BUDGET) return;
      for (const [sid, s] of [...sheets].sort((a, b) => a[1].used - b[1].used)) { if (sheetBytes <= MEMORY_BUDGET * 0.85) break; sheets.delete(sid); sheetBytes -= s.blob.size; }
    }
    function fromMemory(job) {
      const k = cacheKey(job.i, job.level), e = k && entries.get(k); if (!e) return null;
      const s = sheets.get(e.sheet); if (!s) { entries.delete(k); return null; }
      s.used = ++clock; return { ...e, blob: s.blob };
    }

    // --- Tier 3: IndexedDB, surviving reloads -------------------------------------------------
    let disk = null, diskBytes = 0, diskErrors = 0;
    try { disk = await openDisk(); for (const { size } of disk.sizes.values()) diskBytes += size; } catch { disk = null; }
    function persist(id, blob, items) {
      if (!disk || !useDisk) return;
      try {
        const tx = disk.db.transaction(['sheets', 'entries'], 'readwrite'), sheetStore = tx.objectStore('sheets'), entryStore = tx.objectStore('entries');
        sheetStore.put({ blob, size: blob.size, time: Date.now() }, id); disk.sizes.set(id, { size: blob.size, time: Date.now() }); diskBytes += blob.size;
        for (const item of items) { const k = cacheKey(item.i, item.level); if (k) entryStore.put({ sheet: id, x: item.x, y: item.y, width: item.width, height: item.height }, k); }
        // Over budget: delete the oldest sheets; entries pointing at them read as misses.
        if (diskBytes > DISK_BUDGET) for (const [sid, s] of [...disk.sizes].sort((a, b) => a[1].time - b[1].time)) {
          if (diskBytes <= DISK_BUDGET * 0.85) break; sheetStore.delete(sid); disk.sizes.delete(sid); diskBytes -= s.size;
        }
        tx.onerror = () => diskErrors++;
      } catch { diskErrors++; }
    }
    function fromDisk(list) {
      return new Promise(resolve => {
        const found = [];
        if (!disk || !useDisk || !list.length) return resolve(found);
        try {
          const tx = disk.db.transaction(['entries', 'sheets']), entryStore = tx.objectStore('entries'), sheetStore = tx.objectStore('sheets'), reads = new Map();
          list.forEach((job, n) => {
            const k = cacheKey(job.i, job.level); if (!k) return;
            const request = entryStore.get(k);
            request.onsuccess = () => {
              const e = request.result; if (!e) return;
              if (!reads.has(e.sheet)) reads.set(e.sheet, idb(sheetStore.get(e.sheet)).catch(() => null));
              reads.get(e.sheet).then(s => { if (!s) return; remember(e.sheet, s.blob, []); entries.set(k, e); found[n] = { ...e, blob: s.blob }; });
            };
          });
          tx.oncomplete = () => resolve(found); tx.onerror = tx.onabort = () => { diskErrors++; resolve(found); };
        } catch { diskErrors++; resolve(found); }
      });
    }

    // --- Filling from tiers 2 and 3: one decode per sheet, then crops -------------------------
    const restoring = new Map();
    function restore(job, entry, source) {
      job.state = 'restoring';
      if (!restoring.has(entry.sheet)) { restoring.set(entry.sheet, { blob: entry.blob, items: [] }); if (restoring.size === 1) queueMicrotask(flushRestores); }
      restoring.get(entry.sheet).items.push({ job, entry, source });
    }
    function flushRestores() {
      const groups = [...restoring.values()]; restoring.clear();
      for (const group of groups) void (async () => {
        let bitmap = null;
        try {
          bitmap = await createImageBitmap(group.blob);
          for (const { job, entry, source } of group.items) {
            const k = key(job.i, job.level); if (jobs.get(k) !== job || ctx.signal.aborted) continue;
            store(job.i, job.level, await createImageBitmap(bitmap, entry.x, entry.y, entry.width, entry.height));
            filled[source]++; jobs.delete(k); recent.push(performance.now());
          }
        } catch { for (const { job } of group.items) if (jobs.get(key(job.i, job.level)) === job) job.state = 'queued'; }
        finally { bitmap?.close(); if (!ctx.signal.aborted) ctx.requestFrame(); }
      })();
    }
    async function lookup(list) {
      await hashing;
      const misses = [];
      for (const job of list) { const entry = fromMemory(job); if (entry) restore(job, entry, 'memory'); else misses.push(job); }
      const found = await fromDisk(misses);
      misses.forEach((job, n) => { if (jobs.get(key(job.i, job.level)) !== job) return; if (found[n]) restore(job, found[n], 'disk'); else job.state = 'queued'; });
      if (!ctx.signal.aborted) ctx.requestFrame();
    }

    // --- Render queue: priority = distance to centre; low-resolution first; cancellable -------
    function schedule() {
      const visible = inView(MARGIN), want = desired(), wanted = new Map();
      for (const i of visible) {
        const priority = distanceToCentre(i);
        // Unseen artboards first get the 0.125× base: sheets of 16 render in one pass.
        if (lowFirst && want > 0 && !have[i]) wanted.set(key(i, 0), { i, level: 0, priority });
        if (!(have[i] & (1 << want))) wanted.set(key(i, want), { i, level: want, priority: priority + (lowFirst && !have[i] ? 1e6 : 0) });
      }
      // Forget work nobody wants any more (artboard left the view, zoom changed).
      for (const [k, job] of jobs) if (!wanted.has(k)) { jobs.delete(k); if (job.state === 'queued' || job.state === 'running') cancelled++; }
      const fresh = [];
      for (const [k, w] of wanted) { const job = jobs.get(k); if (job) job.priority = w.priority; else { const j = { ...w, state: 'lookup' }; jobs.set(k, j); fresh.push(j); } }
      if (fresh.length) void lookup(fresh);
      // A batch whose members were all cancelled stops; one with any member still wanted completes.
      for (const batch of batches) if (batch.members.every(job => jobs.get(key(job.i, job.level)) !== job)) { batch.controller.abort(); batches.delete(batch); }
      // One request per renderer: nothing waits in the browser's connection pool or the server's
      // FIFO, so this queue's order and cancellation decide what renders next.
      const limit = ctx.host.renderers;
      if (batches.size >= limit) return;
      const queued = [...jobs.values()].filter(j => j.state === 'queued').sort((a, b) => a.priority - b.priority);
      for (const lead of queued) {
        if (batches.size >= limit) break;
        if (lead.state !== 'queued') continue;
        // Small resolutions share one render, up to about a megapixel per sheet.
        const scale = LEVELS[lead.level], most = Math.max(1, Math.min(SHEET_MAX, Math.floor(SHEET_PIXELS / (W * H * scale * scale)))), members = [lead];
        for (const job of queued) { if (members.length >= most) break; if (job !== lead && job.state === 'queued' && job.level === lead.level) members.push(job); }
        void run(members);
      }
    }
    const escape = s => s.replaceAll('&', '&amp;').replaceAll('"', '&quot;');
    async function run(members) {
      const level = members[0].level, scale = LEVELS[level], side = Math.ceil(Math.sqrt(members.length));
      const cols = Math.min(side, members.length), rows = Math.ceil(members.length / cols);
      const batch = { members, controller: new AbortController() }; batches.add(batch);
      for (const job of members) job.state = 'running';
      const sheet = members.length === 1 ? html(members[0].i)
        : `<!doctype html><style>html,body{margin:0}body{display:grid;grid-template-columns:repeat(${cols},${W}px);grid-auto-rows:${H}px}iframe{border:0;width:${W}px;height:${H}px;display:block}</style>`
          + members.map(job => `<iframe srcdoc="${escape(html(job.i))}"></iframe>`).join('');
      const began = performance.now();
      try {
        const { png, ms, phases } = await ctx.host.render({ html: sheet, width: cols * W, height: rows * H, scale, signal: batch.controller.signal });
        const received = performance.now();
        // Every member goes into tiers 2 and 3, including ones cancelled meanwhile: the work is done.
        const cw = Math.round(W * scale), ch = Math.round(H * scale), id = crypto.randomUUID();
        const items = members.map((job, n) => ({ i: job.i, level, x: (n % cols) * cw, y: Math.floor(n / cols) * ch, width: cw, height: ch }));
        await hashing; remember(id, png, items); persist(id, png, items);
        const bitmap = await createImageBitmap(png); // decoded off the main thread
        const decoded = performance.now();
        for (const [n, job] of members.entries()) {
          if (jobs.get(key(job.i, job.level)) !== job || ctx.signal.aborted) continue; // cancelled meanwhile
          const item = items[n];
          store(job.i, level, await createImageBitmap(bitmap, item.x, item.y, item.width, item.height));
          filled.render++; recent.push(performance.now());
        }
        bitmap.close();
        timings.push({ level, members: members.length, bytes: png.size, total: performance.now() - began, server: ms, transfer: received - began - ms, decode: decoded - received, upload: performance.now() - decoded, ...phases });
        if (timings.length > 1000) timings.shift();
        ctx.stats.set('last render', `${(performance.now() - began).toFixed(0)} ms · ${members.length} artboard${members.length > 1 ? 's' : ''}`);
      } catch (error) { if (error.name !== 'AbortError' && !String(error.message).includes('cancelled') && !ctx.signal.aborted) failed++; }
      finally {
        batches.delete(batch);
        for (const job of members) { const k = key(job.i, job.level); if (jobs.get(k) === job) jobs.delete(k); }
        if (!ctx.signal.aborted) { schedule(); ctx.requestFrame(); }
      }
    }

    // --- Live iframes: nearest the centre once large on screen; hidden until painted ------------
    function updateLive() {
      const onScreen = W * ctx.camera.scale, live = new Set();
      if (maxLive > 0 && onScreen >= liveEnter * 0.85) {
        const candidates = inView(0).map(i => ({ i, d: distanceToCentre(i) })).sort((a, b) => a.d - b.d);
        for (const { i } of candidates) {
          if (live.size >= maxLive) break;
          // Hysteresis: already-live artboards stay live down to 85% of the threshold.
          if (onScreen >= liveEnter || frames.get(i)?.wanted) live.add(i);
        }
      }
      const now = performance.now();
      for (const [i, f] of frames) if (!live.has(i) && f.wanted) { f.wanted = false; f.el.style.visibility = 'hidden'; }
      for (const i of live) {
        let f = frames.get(i);
        if (!f) {
          const el = document.createElement('iframe'), p = boardAt(i);
          Object.assign(el.style, { position: 'absolute', left: p.x + 'px', top: p.y + 'px', width: W + 'px', height: H + 'px', border: '0', visibility: 'hidden', background: 'transparent' });
          f = { el, ready: false, wanted: false, since: now, cold: true };
          el.addEventListener('load', () => requestAnimationFrame(() => requestAnimationFrame(() => {
            f.ready = true; if (f.wanted) reveal(f);
          })), { signal: ctx.signal });
          el.srcdoc = html(i); ctx.world.append(el); frames.set(i, f);
        }
        f.used = now;
        if (!f.wanted) { f.wanted = true; f.since = now; if (f.ready) reveal(f); }
      }
      // Keep a few warm iframes beyond the live set; drop the least recently used.
      const extra = [...frames].filter(([, f]) => !f.wanted).sort((a, b) => a[1].used - b[1].used);
      while (frames.size > maxLive + POOL_EXTRA && extra.length) { const [i, f] = extra.shift(); f.el.remove(); frames.delete(i); }
    }
    function reveal(f) {
      f.el.style.visibility = 'visible';
      latencies.push({ ms: performance.now() - f.since, cold: f.cold }); f.cold = false;
      if (latencies.length > 40) latencies.shift();
    }

    function dropTextures() { for (const k of [...textures.keys()]) drop(k); have.length = 0; }
    function layout() {
      for (const batch of batches) batch.controller.abort(); batches.clear(); jobs.clear();
      dropTextures();
      for (const f of frames.values()) f.el.remove(); frames.clear();
      columns = Math.max(1, Math.ceil(Math.sqrt(count * (H + ROW_GAP) / (W + GAP))));
      instances?.destroy(); instances = device.createBuffer({ size: Math.max(32, count * 32), usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
      hashing = hashAll();
      ctx.requestFrame();
    }
    async function clearAll() {
      dropTextures(); sheets.clear(); entries.clear(); sheetBytes = 0;
      if (disk) { try { const tx = disk.db.transaction(['sheets', 'entries'], 'readwrite'); tx.objectStore('sheets').clear(); tx.objectStore('entries').clear(); } catch { diskErrors++; } disk.sizes.clear(); diskBytes = 0; }
      ctx.requestFrame();
    }
    ctx.controls.segmented('Content', ['wild', 'simple'], content, v => { content = v; layout(); });
    ctx.controls.slider('Artboards (log)', { min: 0, max: Math.log10(2000), step: 0.01, value: Math.log10(count) }, v => { count = Math.max(1, Math.round(10 ** v)); layout(); ctx.fit(); });
    ctx.controls.segmented('Max live iframes', ['0', '1', '3', '5', '8'], String(maxLive), v => { maxLive = Number(v); ctx.requestFrame(); });
    ctx.controls.slider('Go live at width (px)', { min: 120, max: 900, step: 10, value: liveEnter }, v => { liveEnter = v; ctx.requestFrame(); });
    ctx.controls.toggle('Low-resolution first', lowFirst, v => { lowFirst = v; ctx.requestFrame(); });
    ctx.controls.toggle('Disk cache (IndexedDB)', useDisk, v => { useDisk = v; ctx.requestFrame(); });
    ctx.controls.toggle('Outline representation', showKind, v => { showKind = v; ctx.requestFrame(); });
    ctx.controls.button('Drop GPU textures', () => { dropTextures(); ctx.requestFrame(); });
    ctx.controls.button('Clear all caches', () => void clearAll());
    layout();
    const poll = setInterval(async () => { try { server = await ctx.host.renderStats(); } catch { /* server gone */ } }, 1000);

    const kindColor = { live: [61, 220, 151], exact: [76, 188, 255], low: [255, 181, 71], none: [140, 140, 140] };
    return {
      bounds: () => { const rows = Math.ceil(count / columns); return [{ x: 0, y: 0, width: columns * (W + GAP) - GAP, height: rows * (H + ROW_GAP) - ROW_GAP }]; },
      render(pass, { camera, width, height, dpr }) {
        updateLive(); schedule();
        const visible = inView(0), want = desired(), data = new Float32Array(visible.length * 8), groups = [];
        visible.forEach((i, n) => {
          const p = boardAt(i), l = best(i, want), t = l >= 0 ? textures.get(key(i, l)) : null;
          if (t) t.used = ++clock;
          data.set(t ? [p.x, p.y, W, H, 1, 1, 1, 1] : [p.x, p.y, W, H, 0.22, 0.23, 0.26, 1], n * 8); groups.push(t?.group ?? whiteGroup);
        });
        if (visible.length) device.queue.writeBuffer(instances, 0, data);
        device.queue.writeBuffer(viewBuffer, 0, new Float32Array([camera.x * dpr, camera.y * dpr, camera.scale * dpr, 0, width * dpr, height * dpr, 0, 0]));
        pass.setPipeline(pipeline); pass.setBindGroup(0, viewGroup); pass.setVertexBuffer(0, instances);
        for (let n = 0; n < visible.length; n++) { pass.setBindGroup(1, groups[n]); pass.draw(6, 1, 0, n); }
        this.visible = visible; this.want = want;
      },
      frame({ overlay, dpr }) {
        const visible = this.visible ?? [], want = this.want ?? desired(), kinds = { live: 0, exact: 0, low: 0, none: 0 };
        overlay.setTransform(dpr, 0, 0, dpr, 0, 0);
        const s = ctx.camera.scale, labels = W * s >= 150;
        overlay.font = '600 11px ui-monospace, SF Mono, Menlo, monospace'; overlay.textBaseline = 'middle';
        for (const i of visible) {
          const f = frames.get(i), l = best(i, want);
          const kind = f?.wanted && f.ready ? 'live' : l >= want ? 'exact' : l >= 0 ? 'low' : 'none'; kinds[kind]++;
          if (!showKind) continue;
          const p = boardAt(i), a = ctx.worldToScreen(p.x, p.y), [r, g, b] = kindColor[kind];
          overlay.strokeStyle = `rgb(${r} ${g} ${b} / 0.9)`; overlay.lineWidth = W * s > 40 ? 2 : 1; overlay.strokeRect(a.x - 1, a.y - 1, W * s + 2, H * s + 2);
          if (labels) { overlay.fillStyle = `rgb(${r} ${g} ${b})`; overlay.fillText(`#${i + 1}  ${kind === 'live' ? 'IFRAME' : l >= 0 ? `PNG ${LEVELS[l]}×${kind === 'low' ? ' (upgrading)' : ''}` : 'placeholder'}`, a.x, a.y - 12); }
        }
        const now = performance.now(); recent = recent.filter(t => now - t < 2000);
        const sorted = latencies.map(x => x.ms).sort((a, b) => a - b), warm = latencies.filter(x => !x.cold).map(x => x.ms).sort((a, b) => a - b);
        const states = { lookup: 0, restoring: 0, queued: 0, running: 0 }; for (const job of jobs.values()) states[job.state]++;
        ctx.stats.set('artboards', count);
        ctx.stats.set('visible', visible.length);
        ctx.stats.set('wanted PNG level', LEVELS[want] + '×');
        ctx.stats.set('shown: iframe / exact / low / none', `${kinds.live} / ${kinds.exact} / ${kinds.low} / ${kinds.none}`);
        ctx.stats.set('iframes (live / warm)', `${[...frames.values()].filter(f => f.wanted).length} / ${frames.size}`);
        ctx.stats.set('queue (queued / running / batches)', `${states.queued} / ${states.running} / ${batches.size}`);
        ctx.stats.set('cache lookups (pending / decoding)', `${states.lookup} / ${states.restoring}`);
        ctx.stats.set('server (queued / running)', `${server.queued ?? '—'} / ${server.running ?? '—'} of ${server.renderers ?? ctx.host.renderers}`);
        ctx.stats.set('fills / s', (recent.length / 2).toFixed(1));
        ctx.stats.set('filled from memory / disk / render', `${filled.memory} / ${filled.disk} / ${filled.render}`);
        ctx.stats.set('cancelled / failed', `${cancelled} / ${failed}`);
        ctx.stats.set('GPU textures', `${textures.size} · ${(bytes / MiB).toFixed(0)} MiB`);
        ctx.stats.set('memory cache', `${sheets.size} sheets · ${(sheetBytes / MiB).toFixed(1)} MiB`);
        ctx.stats.set('disk cache', disk ? `${disk.sizes.size} sheets · ${(diskBytes / MiB).toFixed(1)} MiB${diskErrors ? ` · ${diskErrors} errors` : ''}` : 'unavailable');
        ctx.stats.set('swap latency (warm / all p50)', `${warm.length ? warm[warm.length >> 1].toFixed(0) + ' ms' : '—'} / ${sorted.length ? sorted[sorted.length >> 1].toFixed(0) + ' ms' : '—'}`);
        if (jobs.size || batches.size || [...frames.values()].some(f => f.wanted && !f.ready)) ctx.requestFrame();
      },
      dispose() {
        clearInterval(poll);
        for (const batch of batches) batch.controller.abort();
        dropTextures();
        instances?.destroy(); viewBuffer.destroy(); white.destroy();
        disk?.db.close();
      },
    };
  },
};
