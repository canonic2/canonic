export const styles = `
:host { position:relative; display:inline-flex; align-items:center;
  font-family:var(--wb-ui-font,system-ui,sans-serif); font-size:var(--wb-ui-font-size,13px); color:var(--wb-fg); }
:host([hidden]) { display:none !important; }
[hidden] { display:none !important; }
button { font:inherit; color:inherit; box-sizing:border-box; cursor:pointer; }
button:disabled { cursor:not-allowed; }
button:focus-visible { outline:2px solid var(--wb-focus); outline-offset:-2px; }
svg { flex:none; display:block; }
.group { display:flex; align-items:center; gap:2px; padding:3px; background:var(--wb-raised);
  border:1px solid var(--wb-line); border-radius:8px; }
.size, .more { display:grid; place-items:center; width:32px; height:28px; padding:0;
  color:var(--wb-fg-2); background:none; border:0; border-radius:6px; }
.size:hover:not(:disabled), .more:hover:not(:disabled) { color:var(--wb-fg); background:var(--wb-hover); }
.size[aria-pressed=true], .more[aria-pressed=true], .more[aria-expanded=true] { color:var(--wb-fg); background:var(--wb-selected); }
.size:disabled, .more:disabled { color:var(--wb-fg-3); opacity:.38; }
/* A divider between the button sizes and the menu button, when there are any. */
.size + .more { position:relative; margin-left:7px; }
.size + .more::before { content:''; position:absolute; left:-5px; top:6px; bottom:6px; width:1px; background:var(--wb-line); }
.menu { position:absolute; top:calc(100% + 8px); left:0; z-index:20;
  display:flex; flex-direction:column; width:max-content; min-width:300px; max-width:min(360px,calc(100vw - 24px));
  max-height:min(560px,calc(100vh - 80px)); overflow:auto; padding:6px;
  color:var(--wb-menu-fg,var(--wb-fg)); background:var(--wb-menu-bg,var(--wb-panel));
  border:1px solid var(--wb-line); border-radius:10px; box-shadow:0 12px 32px #0007; }
.heading { padding:8px 10px 4px; color:var(--wb-fg-3); font-size:11px; font-weight:600; letter-spacing:.08em; text-transform:uppercase; }
.item { display:grid; grid-template-columns:16px 16px 1fr auto; align-items:center; gap:8px; width:100%;
  min-height:34px; padding:4px 10px; text-align:left; background:none; border:0; border-radius:6px; }
.item:hover:not(:disabled), .item:focus-visible { background:var(--wb-hover); }
.item[aria-checked=true] { background:var(--wb-selection-bg,var(--wb-hover)); }
.item:disabled { color:var(--wb-fg-3); }
.check { color:var(--wb-check); visibility:hidden; }
.item[aria-checked=true] .check { visibility:visible; }
.dims { color:var(--wb-fg-3); font-variant-numeric:tabular-nums; padding-left:24px; }
/* Actions have no check or icon; their text starts where the icons do. */
.action { grid-template-columns:16px 1fr; }
.action .label { grid-column:2; }
.rule { height:1px; margin:6px 4px; background:var(--wb-line); }
@media (forced-colors:active) { .group, .menu { border:1px solid ButtonText; }
  .size[aria-pressed=true], .more[aria-pressed=true], .item[aria-checked=true] { outline:1px solid Highlight; }
  .size + .more::before { background:ButtonText; } }
`;
