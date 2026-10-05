export const styles = `
:host { display:block; position:relative; padding:8px 8px 4px;
  font-family:var(--wb-ui-font,system-ui,sans-serif); font-size:var(--wb-ui-font-size,13px);
  color:var(--wb-fg); }
:host([hidden]) { display:none !important; }
button { font:inherit; cursor:pointer; box-sizing:border-box; }
button:disabled { cursor:default; opacity:.5; }
button:focus-visible { outline:2px solid var(--wb-focus); outline-offset:-2px; }
.trigger { display:flex; align-items:center; gap:10px; width:100%; min-height:40px;
  padding:6px 8px; font-weight:590; text-align:left; color:inherit;
  background:var(--wb-raised); border:1px solid var(--wb-line); border-radius:8px; }
.trigger:hover, .trigger[aria-expanded=true] { background:var(--wb-hover); }
.trigger wb-space-mark { width:24px; height:24px; }
.label { flex:1 1 auto; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.caret { color:var(--wb-fg-3); }
svg { flex:none; display:block; }
.menu { position:absolute; top:calc(100% + 2px); left:8px; right:8px; z-index:5;
  display:flex; flex-direction:column; max-height:min(420px,calc(100vh - 80px)); overflow:auto;
  padding:4px; color:var(--wb-menu-fg,var(--wb-fg)); background:var(--wb-menu-bg,var(--wb-panel));
  border:1px solid var(--wb-line); border-radius:10px; box-shadow:0 10px 28px #0006; }
[hidden] { display:none !important; }
.heading { padding:8px 8px 6px; color:var(--wb-fg-3); font-size:11px;
  font-weight:600; letter-spacing:.08em; text-transform:uppercase; }
.row { position:relative; display:flex; }
.item { display:flex; align-items:center; gap:10px; width:100%; min-height:36px; padding:6px 8px;
  font-weight:560; text-align:left; color:inherit; background:none; border:0; border-radius:6px; }
.item:hover { background:var(--wb-hover); }
.item[aria-checked=true] { background:var(--wb-selection-bg,var(--wb-hover)); color:var(--wb-selection-fg); }
.check { color:var(--wb-check); visibility:hidden; }
.item[aria-checked=true] .check { visibility:visible; }
.remove { position:absolute; top:50%; right:6px; display:grid; place-items:center; width:24px; height:24px;
  padding:0; color:var(--wb-fg-3); background:none; border:0; border-radius:4px;
  opacity:0; transform:translateY(-50%); }
.row:hover .remove, .remove:focus-visible { opacity:1; }
.row:hover:has(.remove) .check, .row:has(.remove:focus-visible) .check { visibility:hidden; }
.remove:hover { color:var(--wb-fg); background:var(--wb-hover); }
.rule { height:1px; margin:4px; background:var(--wb-line); }
.add { color:var(--wb-fg-2); font-weight:500; }
:host([variant=breadcrumb]) { display:inline-block; padding:0; }
:host([variant=breadcrumb]) .trigger { width:auto; max-width:200px; min-height:28px; gap:8px;
  padding:0 4px; color:var(--wb-fg-2); background:none; border:0; border-radius:4px; font-size:12px; }
:host([variant=breadcrumb]) .trigger:hover, :host([variant=breadcrumb]) .trigger[aria-expanded=true] { background:var(--wb-hover); }
:host([variant=breadcrumb]) .trigger wb-space-mark { width:18px; height:18px; --wb-mark-radius:5px; --wb-mark-font-size:10px; }
:host([variant=breadcrumb]) .caret { display:none; }
:host([variant=breadcrumb]) .menu { top:calc(100% + 8px); left:0; right:auto; width:max-content;
  min-width:220px; max-width:min(320px,calc(100vw - 24px)); }
@media (forced-colors:active) { .trigger, .menu { border:1px solid ButtonText; }
  .item[aria-checked=true] { outline:1px solid Highlight; } }
`;
