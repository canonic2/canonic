/** Ten deliberately heavy artboard themes for rendering experiments: layered gradients, blend modes,
 *  filters, backdrop blur, clip paths, masks, many system font families, several scripts, emoji and
 *  generated SVG images. Everything is inline, so pages render offline and deterministically: the
 *  same index always produces the same document. Every page has one `<button>` that reacts to clicks. */

const FONTS = {
  futura: "Futura, 'Century Gothic', 'Avenir Next', sans-serif",
  helvetica: "'Helvetica Neue', Helvetica, Arial, sans-serif",
  didot: "Didot, 'Bodoni 72', 'Bodoni MT', Georgia, serif",
  script: "'Snell Roundhand', 'Brush Script MT', 'Segoe Script', cursive",
  avenir: "'Avenir Next', Avenir, 'Segoe UI', sans-serif",
  mono: "Menlo, 'SF Mono', Consolas, monospace",
  marker: "'Marker Felt', 'Comic Sans MS', cursive",
  chalk: "'Chalkboard SE', 'Comic Sans MS', cursive",
  copperplate: "Copperplate, 'Copperplate Gothic Light', serif",
  optima: "Optima, Candara, sans-serif",
  papyrus: "Papyrus, fantasy",
  impact: "Impact, 'Arial Black', sans-serif",
  typewriter: "'American Typewriter', 'Courier New', monospace",
  baskerville: "Baskerville, 'Baskerville Old Face', Garamond, serif",
  gill: "'Gill Sans', 'Gill Sans MT', sans-serif",
  noteworthy: "Noteworthy, 'Segoe Print', cursive",
  trebuchet: "'Trebuchet MS', sans-serif",
  rockwell: "Rockwell, 'Rockwell Nova', 'Courier New', serif",
  japanese: "'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Yu Gothic', sans-serif",
};

export const THEMES = ['Synthwave', 'Swiss poster', 'Editorial', 'Glass dashboard', 'Terminal', 'Candy', 'Art deco', 'Brutalist board', 'Botanical', 'Cyberpunk'];

/** Deterministic random numbers in [0, 1) for a seed (mulberry32). */
function random(seed) {
  return () => { seed = seed + 0x6d2b79f5 | 0; let t = Math.imul(seed ^ seed >>> 15, 1 | seed); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; };
}
const pick = (r, list) => list[Math.floor(r() * list.length)];
const between = (r, a, b) => a + r() * (b - a);
/** An SVG document as a data URL, safe inside single-quoted CSS `url()` and double-quoted attributes. */
const svg = (w, h, body) => 'data:image/svg+xml,' + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${body}</svg>`).replaceAll("'", '%27');

/** A generated "photo": sky gradient, sun, three mountain ridges and film grain. */
function landscape(r, w, h, hue) {
  const ridges = [0, 1, 2].map(k => {
    const base = h * (0.5 + k * 0.14); let d = `M0 ${h} L0 ${base.toFixed(0)}`;
    for (let x = 0; x <= w; x += w / 10) d += ` L${x.toFixed(0)} ${(base - r() * h * (0.3 - k * 0.07)).toFixed(0)}`;
    return `<path d="${d} L${w} ${h}Z" fill="hsl(${hue + 150 + k * 15} ${30 + k * 15}% ${55 - k * 16}%)"/>`;
  }).join('');
  return svg(w, h, `<defs><linearGradient id="sky" x2="0" y2="1"><stop offset="0" stop-color="hsl(${hue + 200} 70% 55%)"/><stop offset=".7" stop-color="hsl(${hue + 20} 95% 75%)"/></linearGradient>
<filter id="grain"><feTurbulence type="fractalNoise" baseFrequency=".85" numOctaves="2" seed="${Math.floor(r() * 99)}"/><feColorMatrix type="saturate" values="0"/></filter>
<radialGradient id="glow"><stop offset="0" stop-color="#fff8d0"/><stop offset=".4" stop-color="hsl(${hue + 40} 100% 75%)"/><stop offset="1" stop-color="hsl(${hue + 40} 100% 75%)" stop-opacity="0"/></radialGradient></defs>
<rect width="${w}" height="${h}" fill="url(#sky)"/><circle cx="${(w * between(r, 0.2, 0.8)).toFixed(0)}" cy="${(h * 0.35).toFixed(0)}" r="${(h * 0.3).toFixed(0)}" fill="url(#glow)"/>${ridges}
<rect width="${w}" height="${h}" filter="url(#grain)" opacity=".28" style="mix-blend-mode:overlay"/>`);
}
/** A line-and-area chart. */
function chart(r, w, h, color, points = 14) {
  const ys = Array.from({ length: points }, () => h * between(r, 0.15, 0.85));
  const line = ys.map((y, k) => `${k ? 'L' : 'M'}${(k * w / (points - 1)).toFixed(1)} ${y.toFixed(1)}`).join(' ');
  return svg(w, h, `<defs><linearGradient id="a" x2="0" y2="1"><stop offset="0" stop-color="${color}" stop-opacity=".55"/><stop offset="1" stop-color="${color}" stop-opacity="0"/></linearGradient></defs>
<path d="${line} L${w} ${h} L0 ${h}Z" fill="url(#a)"/><path d="${line}" fill="none" stroke="${color}" stroke-width="2.5" stroke-linejoin="round" stroke-linecap="round"/>`);
}
/** A round avatar with a gradient and an emoji. */
function avatar(r, size, emoji) {
  const hue = Math.floor(r() * 360);
  return svg(size, size, `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${hue} 90% 65%)"/><stop offset="1" stop-color="hsl(${hue + 70} 90% 45%)"/></linearGradient></defs>
<circle cx="${size / 2}" cy="${size / 2}" r="${size / 2}" fill="url(#g)"/><text x="50%" y="54%" dominant-baseline="middle" text-anchor="middle" font-size="${size * 0.55}">${emoji}</text>`);
}
/** A smooth closed blob through `n` jittered points. */
function blobPath(r, cx, cy, radius, n = 9) {
  const p = Array.from({ length: n }, (_, k) => { const a = k / n * Math.PI * 2, d = radius * between(r, 0.7, 1.15); return [cx + Math.cos(a) * d, cy + Math.sin(a) * d]; });
  const mid = k => { const a = p[k % n], b = p[(k + 1) % n]; return [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]; };
  let d = `M${mid(0).map(v => v.toFixed(1)).join(' ')}`;
  for (let k = 1; k <= n; k++) d += ` Q${p[k % n].map(v => v.toFixed(1)).join(' ')} ${mid(k).map(v => v.toFixed(1)).join(' ')}`;
  return d + 'Z';
}
const leaf = (r, hue) => svg(120, 120, `<path d="M60 112 C10 80 8 30 60 6 C112 30 110 80 60 112Z" fill="hsl(${hue} 45% ${between(r, 30, 48).toFixed(0)}%)"/><path d="M60 110 L60 14 M60 44 L36 30 M60 62 L84 46 M60 80 L38 66 M60 96 L80 84" stroke="hsl(${hue} 40% 82%)" stroke-width="2.5" fill="none" stroke-linecap="round"/>`);

const button = (css = '') => `<button style="${css}" onclick="this.textContent='Clicked ✓'">Action</button>`;
const SPARK = '✦';

const themes = [
  // 0 Synthwave: striped sun, perspective grid floor, chrome text.
  (r, n, hue) => ({
    css: `body{background:linear-gradient(#0b0221,#2b0b4f 40%,#ff2a6d 64%,#0b0221 64.3%);font-family:${FONTS.futura};color:#fff}
.sun{position:absolute;left:50%;top:44px;width:230px;height:230px;margin-left:-115px;border-radius:50%;background:linear-gradient(#ffe259,#ff2a6d 75%);-webkit-mask:linear-gradient(#000 45%,transparent 45%),repeating-linear-gradient(#000 0 11px,transparent 11px 17px);filter:drop-shadow(0 0 40px #ff2a6d)}
.floor{position:absolute;left:-60%;right:-60%;bottom:-30px;height:230px;background:repeating-linear-gradient(90deg,transparent 0 56px,#05d9e8 56px 59px),repeating-linear-gradient(transparent 0 30px,#05d9e8 30px 33px),linear-gradient(#1a0536,#0b0221);transform:perspective(260px) rotateX(62deg);transform-origin:bottom;box-shadow:0 -10px 50px #05d9e8}
h1{position:absolute;top:226px;width:100%;margin:0;text-align:center;font:italic 900 70px/1 ${FONTS.futura};letter-spacing:3px;background:linear-gradient(#fff 15%,#8fd3ff 47%,#1b1035 50%,#ff7edb 53%,#fff 88%);-webkit-background-clip:text;background-clip:text;color:transparent;-webkit-text-stroke:1.5px #ff7edb;filter:drop-shadow(0 5px 0 #2b0b4f) drop-shadow(0 0 18px #ff2a6d)}
.sub{position:absolute;top:300px;width:100%;text-align:center;font:38px ${FONTS.script};color:#fdf;text-shadow:0 0 6px #ff2a6d,0 0 18px #ff2a6d;transform:rotate(-5deg)}
.palm{position:absolute;bottom:14px;font-size:64px;filter:drop-shadow(0 0 10px #05d9e8) hue-rotate(${Math.floor(r() * 60)}deg)}
.stars{position:absolute;inset:0 0 55% 0;background-image:radial-gradient(1.5px 1.5px at 20% 30%,#fff,transparent),radial-gradient(1px 1px at 70% 20%,#fff,transparent),radial-gradient(2px 2px at 85% 45%,#ffe259,transparent),radial-gradient(1px 1px at 40% 60%,#fff,transparent);background-size:160px 120px}
button{position:absolute;right:22px;top:20px;font:700 13px ${FONTS.futura};letter-spacing:2px;color:#0b0221;background:linear-gradient(90deg,#05d9e8,#ff7edb);border:0;border-radius:999px;padding:9px 18px;box-shadow:0 0 18px #ff7edb}`,
    body: `<div class="stars"></div><div class="sun"></div><div class="floor"></div><h1>${pick(r, ['NEON', 'OUTRUN', 'TURBO', 'LASER', 'MIAMI'])} ${n}</h1>
<div class="sub">${pick(r, ['Midnight Drive', 'Electric Dreams', 'Sunset Boulevard', 'Night Call'])}</div><div class="palm" style="left:18px">🌴</div><div class="palm" style="right:18px">🌴🕶️</div>${button()}`,
  }),
  // 1 Swiss poster: giant numerals, multiply blends, vertical type, columns.
  (r, n, hue) => ({
    css: `body{background:#f2efe6 repeating-linear-gradient(90deg,transparent 0 79px,rgb(0 0 0/.06) 79px 80px);font-family:${FONTS.helvetica};color:#111}
.num{position:absolute;right:-24px;top:-70px;font:900 380px/1 ${FONTS.helvetica};letter-spacing:-26px;color:hsl(${hue} 85% 52%);mix-blend-mode:multiply}
.bar{position:absolute;left:36px;top:0;width:96px;height:100%;background:#111}
.disc{position:absolute;left:${Math.floor(between(r, 150, 260))}px;top:${Math.floor(between(r, 170, 230))}px;width:230px;height:230px;border-radius:50%;background:#e30613;mix-blend-mode:multiply}
.ring{position:absolute;left:300px;top:150px;width:170px;height:170px;border-radius:50%;border:22px solid #1d3cff;mix-blend-mode:multiply}
.v{position:absolute;left:62px;bottom:26px;writing-mode:vertical-rl;transform:rotate(180deg);color:#f2efe6;font:700 30px ${FONTS.helvetica};letter-spacing:8px;text-transform:uppercase}
.t{position:absolute;left:162px;top:34px;width:290px;font:800 36px/0.95 ${FONTS.helvetica};text-transform:uppercase;letter-spacing:-1.5px}
.small{position:absolute;left:162px;bottom:24px;width:330px;font:400 9.5px/1.35 ${FONTS.helvetica};columns:3;column-gap:12px;hyphens:auto}
button{position:absolute;right:20px;bottom:20px;font:700 12px ${FONTS.helvetica};text-transform:uppercase;letter-spacing:2px;background:#111;color:#f2efe6;border:0;padding:10px 16px}`,
    body: `<div class="bar"></div><div class="num">${String(n % 100).padStart(2, '0')}</div><div class="disc"></div><div class="ring"></div>
<div class="v">Kunsthalle</div><div class="t">${pick(r, ['Konstruktive Grafik', 'Neue Typografie', 'Form und Farbe', 'Raster Systeme'])}<br>Zürich ${1950 + n % 70}</div>
<div class="small">Die Ausstellung zeigt Plakate, Bücher und Zeichen aus fünf Jahrzehnten. Ordnung entsteht durch das Raster; Spannung durch Asymmetrie. Öffnungszeiten Di–So 10–18 Uhr. Eintritt frei für Studierende. Führungen jeden Donnerstag um 18.30 Uhr in deutscher, französischer und italienischer Sprache. ➜ ${n}</div>${button()}`,
  }),
  // 2 Editorial: masthead, generated photo, drop cap, justified columns, script pull quote.
  (r, n, hue) => ({
    css: `body{background:#faf6ef;color:#1d1a16;font:11.5px/1.45 ${FONTS.baskerville};padding:20px 26px;box-sizing:border-box}
.mast{display:flex;justify-content:space-between;align-items:end;border-bottom:3px double #1d1a16;padding-bottom:4px}
.mast b{font:400 50px/0.9 ${FONTS.didot};letter-spacing:-1px}.mast small{font:italic 11px ${FONTS.baskerville};text-align:right}
.grid{display:grid;grid-template-columns:1.25fr 1fr;gap:18px;margin-top:12px}
img{width:100%;height:200px;object-fit:cover;display:block;filter:contrast(1.08) saturate(.9)}
figcaption{font:italic 9.5px ${FONTS.baskerville};color:#7a6f60;margin-top:3px}
.kicker{font:700 9px ${FONTS.gill};letter-spacing:3px;text-transform:uppercase;color:hsl(${hue} 60% 38%)}
h2{font:italic 700 25px/1.02 ${FONTS.didot};margin:3px 0 7px}
.copy{columns:2;column-gap:12px;text-align:justify;hyphens:auto}
.copy p{margin:0 0 6px}.copy p:first-child::first-letter{float:left;font:400 52px/.8 ${FONTS.didot};padding:3px 5px 0 0;color:hsl(${hue} 60% 38%)}
blockquote{margin:8px 0 0;font:28px/1.05 ${FONTS.script};color:hsl(${hue} 55% 35%);text-align:center}
.jp{font:12px ${FONTS.japanese};letter-spacing:2px;color:#7a6f60;text-align:center}
button{font:italic 12px ${FONTS.baskerville};background:none;border:1px solid #1d1a16;padding:5px 12px;margin-top:6px}`,
    body: `<div class="mast"><b>The ${pick(r, ['Meridian', 'Atelier', 'Quarterly', 'Observer'])}</b><small>No. ${n + 1}<br>Autumn Edition · €${(between(r, 8, 20)).toFixed(2)}</small></div>
<div class="grid"><figure style="margin:0"><img src="${landscape(r, 420, 260, hue)}" alt=""><figcaption>Above: the valley at dusk, photographed for this issue.</figcaption><blockquote>“Beauty is in the details”</blockquote><div class="jp">「美は細部に宿る」 · « La beauté sera convulsive »</div></figure>
<div><div class="kicker">Essay · ${pick(r, ['Architecture', 'Travel', 'Design', 'Culture'])}</div><h2>${pick(r, ['The Quiet Geometry of Mountain Towns', 'Notes on Light and Distance', 'A Season Among the Glassmakers'])}</h2>
<div class="copy"><p>Long before the road was paved, travellers measured the valley by its bells. Each village kept its own pitch, and a careful listener could tell, by sound alone, how far the next warm kitchen lay.</p><p>Today the bells are electric and the kitchens franchised, yet something of the old arithmetic survives — in the spacing of windows, in the patience of stairs.</p><p>We walked it in eleven days.</p></div>${button()}</div></div>`,
  }),
  // 3 Glass dashboard: mesh gradient, backdrop blur cards, charts, avatars, KPIs.
  (r, n, hue) => {
    const kpi = (label, value, delta, color) => `<div class="glass kpi"><small>${label}</small><b>${value}</b><span style="color:${delta > 0 ? '#7dffb2' : '#ff8fa3'}">${delta > 0 ? '▲' : '▼'} ${Math.abs(delta).toFixed(1)}%</span><img src="${chart(r, 140, 34, color, 10)}" alt=""></div>`;
    return {
      css: `body{font:12px ${FONTS.avenir};color:#fff;padding:16px;box-sizing:border-box;background:radial-gradient(circle at 15% 20%,hsl(${hue} 95% 60%),transparent 38%),radial-gradient(circle at 85% 25%,hsl(${hue + 70} 95% 60%),transparent 42%),radial-gradient(circle at 55% 95%,hsl(${hue + 200} 95% 55%),transparent 50%),#0d0f1a}
.glass{background:rgb(255 255 255/.12);border:1px solid rgb(255 255 255/.28);border-radius:16px;backdrop-filter:blur(18px) saturate(170%);box-shadow:0 10px 30px rgb(0 0 0/.25),inset 0 1px 0 rgb(255 255 255/.35)}
header{display:flex;align-items:center;gap:10px;padding:10px 14px}header img{width:34px;height:34px}header h1{font:600 18px ${FONTS.avenir};margin:0;flex:1}
.row{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;margin-top:12px}
.kpi{padding:10px 12px;display:grid;gap:2px}.kpi small{opacity:.75;text-transform:uppercase;letter-spacing:1px;font-size:9px}.kpi b{font:700 26px ${FONTS.avenir}}.kpi img{width:100%;height:34px}
.main{display:grid;grid-template-columns:1.6fr 1fr;gap:12px;margin-top:12px}.chart{padding:12px}.chart img{width:100%;height:150px;display:block}
ul{list-style:none;margin:0;padding:10px 12px}li{display:flex;align-items:center;gap:8px;padding:5px 0;border-bottom:1px solid rgb(255 255 255/.15)}li img{width:22px;height:22px}li span{margin-left:auto}
button{font:600 12px ${FONTS.avenir};color:#0d0f1a;background:linear-gradient(135deg,#fff,#cde);border:0;border-radius:10px;padding:8px 14px}`,
      body: `<header class="glass"><img src="${avatar(r, 68, pick(r, ['🦊', '🐼', '🐸', '🦉', '🐙']))}" alt=""><h1>Acme Analytics · Board ${n + 1} ✨</h1>${button()}</header>
<div class="row">${kpi('Revenue', '$' + (between(r, 10, 99)).toFixed(1) + 'k', between(r, -9, 14), '#7dffb2')}${kpi('Users', Math.floor(between(r, 1000, 9999)).toLocaleString('en'), between(r, -9, 14), '#9ecbff')}${kpi('Latency', Math.floor(between(r, 40, 300)) + ' ms', between(r, -9, 14), '#ffb3e6')}</div>
<div class="main"><div class="glass chart"><img src="${chart(r, 340, 150, '#ffffff')}" alt=""></div><ul class="glass">${['Deploy finished ✅', 'Disk 91% ⚠️', 'Traffic spike 🔥', 'New signup 🎉', 'Backup ok 💾'].map(t => `<li><img src="${avatar(r, 44, pick(r, ['🙂', '😎', '🤖', '👩‍🚀', '🧑‍🎨']))}" alt="">${t}<span>${Math.floor(r() * 59)}m</span></li>`).join('')}</ul></div>`,
    };
  },
  // 4 Terminal: CRT glow, scanlines, box drawing, progress bars, ASCII art.
  (r, n) => {
    const bar = v => '█'.repeat(Math.round(v * 20)) + '▒'.repeat(20 - Math.round(v * 20));
    return {
      css: `body{background:radial-gradient(ellipse at center,#0b2e14,#020a04 78%);color:#39ff6a;font:12px/1.32 ${FONTS.mono};padding:16px 20px;box-sizing:border-box;text-shadow:0 0 3px #39ff6a,0 0 12px rgb(57 255 106/.55)}
body::after{content:'';position:absolute;inset:0;background:repeating-linear-gradient(rgb(0 0 0/.28) 0 1px,transparent 1px 3px),radial-gradient(ellipse at center,transparent 60%,rgb(0 0 0/.65));pointer-events:none}
pre{margin:0;font:inherit;white-space:pre}.a{color:#ffb000;text-shadow:0 0 6px #ffb000}.c{color:#3ff;text-shadow:0 0 6px #3ff}.m{color:#ff5cf4;text-shadow:0 0 6px #ff5cf4}
.cursor{display:inline-block;width:8px;height:14px;background:#39ff6a;vertical-align:-2px;box-shadow:0 0 8px #39ff6a}
button{font:12px ${FONTS.mono};color:#020a04;background:#39ff6a;border:0;padding:3px 10px;box-shadow:0 0 12px #39ff6a}`,
      body: `<pre><span class="a">╔══════════════════════════════════════════════════════════╗
║  ACME-OS v${(n % 9) + 1}.${n % 10}  ░▒▓ NODE ${String(n).padStart(4, '0')} ▓▒░        👾 💾 📟         ║
╚══════════════════════════════════════════════════════════╝</span>
<span class="c">   _____                       </span>  user@acme:~$ ./status --all
<span class="c">  /     \\   ___  ____  _____  </span>  ┌────────────┬──────────────────────┬──────┐
<span class="c"> |  ●  ● | / __|/ _  \\/  __/  </span>  │ cpu        │ ${bar(r())} │ ${Math.floor(r() * 100)}%  │
<span class="c"> |   ▽   || (__| (_) |\\__ \\   </span>  │ memory     │ ${bar(r())} │ ${Math.floor(r() * 100)}%  │
<span class="c">  \\_____/  \\___|\\___/ |___/   </span>  │ network    │ ${bar(r())} │ ${Math.floor(r() * 100)}%  │
                                 └────────────┴──────────────────────┴──────┘
<span class="m">[${String(n % 24).padStart(2, '0')}:${String(Math.floor(r() * 60)).padStart(2, '0')}:07]</span> handshake ok · 0x${Math.floor(r() * 0xffffff).toString(16).padStart(6, '0')} · λ=${r().toFixed(4)}
<span class="m">[${String(n % 24).padStart(2, '0')}:${String(Math.floor(r() * 60)).padStart(2, '0')}:12]</span> <span class="a">WARN</span> sector ${Math.floor(r() * 9999)} remapped ⚠
<span class="m">[${String(n % 24).padStart(2, '0')}:${String(Math.floor(r() * 60)).padStart(2, '0')}:31]</span> downloading ▸ ▸ ▸ ${bar(r())}
<span class="m">[${String(n % 24).padStart(2, '0')}:${String(Math.floor(r() * 60)).padStart(2, '0')}:58]</span> ∑ ∫ ∂ √ ∞ ≈ ≠ ≤ ≥ ← ↑ → ↓ ⌘ ⌥ ⇧ ⏎ ✓ ✗
<span class="c">  ┌─ queue ───────────────┐  ┌─ peers ──────────────┐</span>
<span class="c">  │</span> ▲ build  #${n}    run  <span class="c">│  │</span> ◉ alpha   ${Math.floor(r() * 90 + 10)} ms     <span class="c">│</span>
<span class="c">  │</span> ▼ test   #${n + 1}    wait <span class="c">│  │</span> ◉ bravo   ${Math.floor(r() * 90 + 10)} ms     <span class="c">│</span>
<span class="c">  └───────────────────────┘  └──────────────────────┘</span>
user@acme:~$ </pre>${button()} <span class="cursor"></span>`,
    };
  },
  // 5 Candy: polka dots, conic wheel, outlined marker type, emoji stickers, dashed card.
  (r, n, hue) => {
    const stickers = Array.from({ length: 10 }, () => `<span style="left:${Math.floor(between(r, 0, 580))}px;top:${Math.floor(between(r, 120, 420))}px;transform:rotate(${Math.floor(between(r, -30, 30))}deg);font-size:${Math.floor(between(r, 28, 56))}px">${pick(r, ['🍭', '🦄', '🍩', '🌈', '🧁', '🍓', '🎈', '⭐️', '🐙', '🍦', '🍬', '🎀'])}</span>`).join('');
    return {
      css: `body{background:radial-gradient(circle,#fff 3px,transparent 4px) 0 0/24px 24px,linear-gradient(135deg,hsl(${hue} 100% 88%),hsl(${hue + 150} 90% 85%));font-family:${FONTS.chalk};color:#4a2b6b}
.wheel{position:absolute;right:-70px;top:-70px;width:270px;height:270px;border-radius:50%;background:conic-gradient(#ff5fa2,#ffd93d,#6bcb77,#4d96ff,#c77dff,#ff5fa2);box-shadow:0 0 0 12px #fff,0 0 0 18px #ff5fa2}
h1{position:absolute;left:30px;top:26px;margin:0;font:56px/1 ${FONTS.marker};color:#ff3d7f;-webkit-text-stroke:2px #4a2b6b;text-shadow:4px 4px 0 #fff,8px 8px 0 #4d96ff;transform:rotate(-4deg)}
.stick span{position:absolute;filter:drop-shadow(2px 3px 0 #fff) drop-shadow(0 4px 6px rgb(0 0 0/.25))}
.card{position:absolute;left:30px;bottom:28px;width:300px;background:#fff;border-radius:26px;padding:14px 18px;border:4px dashed #ff5fa2;box-shadow:10px 10px 0 #4d96ff;font:14px/1.35 ${FONTS.noteworthy};transform:rotate(1.5deg)}
.wave{position:absolute;left:0;right:0;bottom:0;height:40px;background:url('${svg(80, 40, '<path d="M0 20 Q20 0 40 20 T80 20 V40 H0Z" fill="#fff" opacity=".8"/>')}') repeat-x}
button{font:700 14px ${FONTS.chalk};color:#fff;background:linear-gradient(#ff7eb3,#ff3d7f);border:3px solid #4a2b6b;border-radius:16px;padding:6px 16px;box-shadow:0 4px 0 #4a2b6b}`,
      body: `<div class="wheel"></div><div class="wave"></div><h1>Sugar Rush #${n + 1}!</h1><div class="stick">${stickers}</div>
<div class="card">Today's flavours: strawberry swirl, blue-raspberry fizz & cotton-cloud 🍬. Collect ${Math.floor(between(r, 3, 12))} stickers for a free scoop!<br>${button()}</div>`,
    };
  },
  // 6 Art deco: sunburst, gold gradient type, double frames, menu with dotted leaders.
  (r, n) => {
    const items = ['Oysters Rockefeller', 'Lobster Thermidor', 'Duck à l’Orange', 'Crêpes Suzette', 'Champagne Cocktail'];
    return {
      css: `body{background:repeating-conic-gradient(from 0deg at 50% 118%,#0e0e0e 0 3deg,#1c1a16 3deg 6deg);color:#e8c77a;font-family:${FONTS.copperplate};text-align:center}
.frame{position:absolute;inset:16px;border:2px solid #c9a24a;outline:1px solid #c9a24a;outline-offset:-9px}
.fan{position:absolute;left:50%;top:22px;margin-left:-110px;width:220px;height:110px;background:url('${svg(220, 110, Array.from({ length: 13 }, (_, k) => `<path d="M110 110 L${(110 + Math.cos(Math.PI + k / 12 * Math.PI) * 110).toFixed(1)} ${(110 + Math.sin(Math.PI + k / 12 * Math.PI) * 110).toFixed(1)}" stroke="#c9a24a" stroke-width="2"/>`).join('') + '<circle cx="110" cy="110" r="40" fill="#0e0e0e" stroke="#c9a24a" stroke-width="3"/><circle cx="110" cy="110" r="28" fill="none" stroke="#c9a24a"/>')}')}
h1{margin:136px 0 0;font:400 46px/1 ${FONTS.didot};letter-spacing:12px;background:linear-gradient(#fff6d5,#e8c77a 40%,#7a5a1a 52%,#f3d98b 70%,#c9a24a);-webkit-background-clip:text;background-clip:text;color:transparent;filter:drop-shadow(0 2px 0 #000)}
.or{font:26px ${FONTS.script};color:#f3d98b;margin:2px 0 10px}
.menu{width:380px;margin:0 auto;text-align:left;font:13px ${FONTS.copperplate};letter-spacing:1px}
.menu div{display:flex;gap:6px;margin:5px 0}.menu i{flex:1;border-bottom:2px dotted #6d5a2c;transform:translateY(-4px)}.menu b{font:400 15px ${FONTS.didot}}
.orn{font-size:18px;letter-spacing:10px;margin-top:10px;color:#c9a24a}
button{font:12px ${FONTS.copperplate};letter-spacing:3px;color:#0e0e0e;background:linear-gradient(#f3d98b,#c9a24a);border:1px solid #fff6d5;padding:6px 18px;margin-top:8px}`,
      body: `<div class="frame"></div><div class="fan"></div><h1>LE ${pick(r, ['GATSBY', 'CHRYSLER', 'ORIENT', 'SAVOY'])}</h1><div class="or">Soirée nº ${n + 1} · 1925</div>
<div class="menu">${items.map(item => `<div><span>${item}</span><i></i><b>${Math.floor(between(r, 12, 90))}</b></div>`).join('')}</div><div class="orn">❦ ✦ ◆ 🥂 ◆ ✦ ❦</div>${button()}`,
    };
  },
  // 7 Brutalist board: thick borders, offset shadows, kanban, striped progress, tags.
  (r, n, hue) => {
    const card = (title, tag, emoji, progress) => `<div class="card"><div class="tag" style="background:hsl(${Math.floor(r() * 360)} 90% 70%)">${tag}</div><b>${emoji} ${title}</b><div class="prog"><i style="width:${Math.floor(progress * 100)}%"></i></div><div class="who"><img src="${avatar(r, 40, pick(r, ['😀', '🤓', '🥸', '🤠']))}" alt=""><img src="${avatar(r, 40, pick(r, ['😺', '🐶', '🐵', '🐧']))}" alt=""></div></div>`;
    const column = (name, cards) => `<div class="col"><h3>${name}</h3>${cards.join('')}</div>`;
    return {
      css: `body{background:hsl(${hue} 95% 72%);font:12px ${FONTS.trebuchet};color:#000;padding:16px;box-sizing:border-box}
.top{display:flex;gap:12px;align-items:center;margin-bottom:12px}.top h1{margin:0;font:400 38px/1 ${FONTS.impact};letter-spacing:1px;text-transform:uppercase;background:#fff;border:3px solid #000;box-shadow:5px 5px 0 #000;padding:2px 10px;transform:rotate(-1deg)}
.board{display:grid;grid-template-columns:repeat(3,1fr);gap:12px}.col{background:#fff;border:3px solid #000;box-shadow:6px 6px 0 #000;border-radius:10px;padding:8px}
h3{margin:0 0 6px;font:700 13px ${FONTS.rockwell};text-transform:uppercase}
.card{border:2px solid #000;border-radius:8px;padding:7px;margin-bottom:8px;background:#fffbe6;box-shadow:3px 3px 0 #000}.card b{display:block;margin:4px 0}
.tag{display:inline-block;border:2px solid #000;border-radius:999px;padding:0 7px;font:700 9px ${FONTS.trebuchet};text-transform:uppercase}
.prog{height:9px;border:2px solid #000;border-radius:6px;overflow:hidden;background:#fff}.prog i{display:block;height:100%;background:repeating-linear-gradient(45deg,#000 0 4px,#ffde59 4px 8px)}
.who{display:flex;margin-top:5px}.who img{width:20px;height:20px;border:2px solid #000;border-radius:50%;margin-right:-6px;background:#fff}
button{font:400 18px ${FONTS.impact};text-transform:uppercase;background:#ff5ea8;border:3px solid #000;box-shadow:4px 4px 0 #000;padding:4px 14px}`,
      body: `<div class="top"><h1>Sprint ${n + 1}</h1>${button()}<span style="font:700 22px ${FONTS.trebuchet}">🚀🔥💥</span></div><div class="board">
${column('To do', [card('Fix login', 'bug', '🐛', r() * 0.2), card('New icons', 'design', '🎨', r() * 0.3)])}
${column('Doing', [card('API cache', 'backend', '⚙️', between(r, 0.3, 0.8)), card('Onboarding', 'ux', '🧭', between(r, 0.3, 0.8))])}
${column('Done 🎉', [card('Dark mode', 'ui', '🌙', 1), card('Pricing page', 'web', '💸', 1)])}</div>`,
    };
  },
  // 8 Botanical: paper texture, organic blobs, leaves, photo in a blob, five scripts.
  (r, n, hue) => {
    const leaves = Array.from({ length: 6 }, () => `<img class="leaf" src="${leaf(r, Math.floor(between(r, 90, 150)))}" style="left:${Math.floor(between(r, -30, 600))}px;top:${Math.floor(between(r, -30, 440))}px;transform:rotate(${Math.floor(r() * 360)}deg) scale(${between(r, 0.5, 1.2).toFixed(2)})" alt="">`).join('');
    return {
      css: `body{background:#f3eddc url('${svg(200, 200, '<filter id="p"><feTurbulence type="fractalNoise" baseFrequency=".035" numOctaves="4"/><feColorMatrix values="0 0 0 0 .55 0 0 0 0 .5 0 0 0 0 .35 0 0 0 .22 0"/></filter><rect width="200" height="200" filter="url(#p)"/>')}');font:13px/1.45 ${FONTS.optima};color:#2f3b24}
.leaf{position:absolute;width:110px;height:110px;opacity:.85;filter:drop-shadow(0 6px 6px rgb(0 0 0/.18))}
.blob{position:absolute;right:26px;top:36px;width:270px;height:300px;overflow:hidden;border-radius:62% 38% 46% 54%/60% 44% 56% 40%;box-shadow:0 18px 40px rgb(47 59 36/.35);border:6px solid #fff8}
.blob img{width:100%;height:100%;object-fit:cover}
.blob2{position:absolute;left:-60px;bottom:-80px;width:330px;height:260px;background:url('${svg(330, 260, `<path d="${blobPath(r, 165, 130, 120)}" fill="hsl(${hue % 60 + 70} 35% 55%)" opacity=".5"/>`)}')}
h1{position:absolute;left:34px;top:40px;margin:0;font:52px/1 ${FONTS.papyrus};color:#2f5d34;text-shadow:0 2px 0 #fff}
.script{position:absolute;left:38px;top:102px;font:30px ${FONTS.script};color:#8a5a2b}
ul{position:absolute;left:38px;top:160px;margin:0;padding:0;list-style:none;font-size:15px}li{margin:4px 0}
.note{position:absolute;left:38px;bottom:30px;width:230px;font:italic 12px ${FONTS.baskerville};color:#5b5140}
button{font:600 13px ${FONTS.optima};color:#f3eddc;background:#2f5d34;border:0;border-radius:999px 999px 999px 4px;padding:8px 18px}`,
      body: `${leaves}<div class="blob2"></div><div class="blob"><img src="${landscape(r, 300, 330, Math.floor(between(r, 60, 140)))}" alt=""></div>
<h1>Herbarium ${n + 1}</h1><div class="script">a field guide to small wonders</div>
<ul><li>🌿 Flower · <b>花</b> <span style="font-family:${FONTS.japanese}">はな</span></li><li>🍄 زهرة · <span lang="ar">الحديقة</span></li><li>🐝 फूल · <span lang="hi">बगीचा</span></li><li>🦋 ดอกไม้ · <span lang="th">สวน</span></li><li>🌻 Blüte · Fleur · Flor · Цветок</li></ul>
<div class="note">Pressed specimens collected along the river path, ${Math.floor(between(r, 1, 28))} ${pick(r, ['April', 'May', 'June', 'July'])}. ${button()}</div>`,
    };
  },
  // 9 Cyberpunk: neon glitch type, clip-path panels, hazard stripes, vertical Japanese, hex dump.
  (r, n) => {
    const hex = Array.from({ length: 6 }, () => Array.from({ length: 8 }, () => Math.floor(r() * 256).toString(16).padStart(2, '0')).join(' ')).join('<br>');
    const title = pick(r, ['NIGHT CITY', 'NEO-OSAKA', 'SECTOR 7', 'DATAFALL']);
    return {
      css: `body{background:linear-gradient(115deg,#0a0014 0 58%,#16002e 58%);color:#0ff;font:11px/1.4 ${FONTS.mono}}
body::before{content:'';position:absolute;inset:0;background:url('${svg(160, 160, '<filter id="n"><feTurbulence baseFrequency=".9" numOctaves="1"/><feColorMatrix values="0 0 0 0 0 0 0 0 0 .9 0 0 0 0 1 0 0 0 .14 0"/></filter><rect width="160" height="160" filter="url(#n)"/>')}');mix-blend-mode:screen}
.jp{position:absolute;right:16px;top:12px;writing-mode:vertical-rl;font:900 64px/1 ${FONTS.japanese};color:#ff003c;opacity:.85;text-shadow:0 0 18px #ff003c;letter-spacing:6px}
h1{position:absolute;left:24px;top:30px;margin:0;font:400 74px/.9 ${FONTS.impact};color:#fcee0a;text-shadow:-4px 0 #ff003c,4px 0 #00f0ff;transform:skew(-9deg);letter-spacing:2px}
.slice{position:absolute;left:30px;top:30px;margin:0;font:400 74px/.9 ${FONTS.impact};color:#00f0ff;transform:skew(-9deg) translateX(14px);clip-path:inset(38% 0 48% 0);letter-spacing:2px;mix-blend-mode:screen}
.panel{position:absolute;background:rgb(0 240 255/.08);border-left:3px solid #ff003c;clip-path:polygon(0 0,calc(100% - 18px) 0,100% 18px,100% 100%,18px 100%,0 calc(100% - 18px));padding:10px 14px;box-sizing:border-box}
.p1{left:24px;top:190px;width:250px;height:170px}.p2{left:290px;top:190px;width:200px;height:170px;color:#fcee0a}
.meter{height:7px;margin:6px 0 9px;background:linear-gradient(90deg,#ff003c,#fcee0a,#00f0ff);clip-path:polygon(0 0,var(--v) 0,calc(var(--v) - 6px) 100%,0 100%);box-shadow:0 0 10px #00f0ff}
.hazard{position:absolute;left:0;right:0;bottom:0;height:26px;background:repeating-linear-gradient(-45deg,#fcee0a 0 16px,#000 16px 32px)}
button{position:absolute;right:120px;bottom:44px;font:400 18px ${FONTS.impact};letter-spacing:2px;color:#000;background:#fcee0a;border:0;padding:6px 18px;clip-path:polygon(10px 0,100% 0,calc(100% - 10px) 100%,0 100%);box-shadow:0 0 18px #fcee0a}`,
      body: `<div class="jp">東京ネオン夜市</div><h1>${title}<br>${String(n).padStart(3, '0')}</h1><div class="slice">${title}</div>
<div class="panel p1">⚡ NEURAL LINK<div class="meter" style="--v:${Math.floor(between(r, 30, 100))}%"></div>🤖 SYNTH CORE<div class="meter" style="--v:${Math.floor(between(r, 30, 100))}%"></div>🔋 POWER CELL<div class="meter" style="--v:${Math.floor(between(r, 30, 100))}%"></div>ID ▸ ${Math.floor(r() * 1e9).toString(36).toUpperCase()}</div>
<div class="panel p2">${hex}</div><div class="hazard"></div>${button()}`,
    };
  },
];

/** The document for artboard `i`, laid out at `width` × `height` CSS pixels. Themes cycle with `i`. */
export function wild(i, width, height) {
  const r = random(i * 7919 + 17), theme = themes[i % themes.length](r, i, (i * 47) % 360);
  return `<!doctype html><meta charset="utf-8"><style>html,body{margin:0;width:${width}px;height:${height}px;overflow:hidden;position:relative}*{box-sizing:border-box}${theme.css}</style>${theme.body}`;
}
