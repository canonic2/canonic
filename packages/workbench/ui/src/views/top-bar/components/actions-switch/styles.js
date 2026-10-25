export const styles = `
:host { display:inline-flex; align-items:center;
  font-family:var(--wb-ui-font,system-ui,sans-serif); font-size:var(--wb-ui-font-size,13px); color:var(--wb-fg); }
:host([hidden]) { display:none !important; }
button { display:flex; align-items:center; gap:8px; box-sizing:border-box; height:36px; padding:0 10px;
  font:inherit; color:var(--wb-fg-2); background:var(--wb-raised);
  border:1px solid var(--wb-line); border-radius:8px; cursor:pointer; }
button:hover:not(:disabled), button[aria-checked=true] { color:var(--wb-fg); }
button:focus-visible { outline:2px solid var(--wb-focus); outline-offset:-2px; }
button:disabled { cursor:default; opacity:.6; }
svg { flex:none; display:block; }
.icon { display:grid; place-items:center; }
.icon:empty { display:none; }
.track { position:relative; flex:none; width:26px; height:16px; border-radius:999px;
  background:var(--wb-selected); transition:background 120ms ease; }
.knob { position:absolute; top:3px; left:3px; width:10px; height:10px; border-radius:50%;
  background:var(--wb-fg-2); transition:transform 120ms ease, background 120ms ease; }
button[aria-checked=true] .track { background:var(--wb-accent); }
button[aria-checked=true] .knob { transform:translateX(10px); background:#fff; }
@media (prefers-reduced-motion:reduce) { .track, .knob { transition:none; } }
@media (forced-colors:active) { button { border-color:ButtonText; }
  .track { outline:1px solid ButtonText; } .knob { background:ButtonText; }
  button[aria-checked=true] .track { background:Highlight; } button[aria-checked=true] .knob { background:HighlightText; } }
`;
