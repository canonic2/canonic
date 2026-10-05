/* The modal form both size dialogs use; wb-sizes-editor adds its table. */
export const dialogStyles = `
:host { font-family:var(--wb-ui-font,system-ui,sans-serif); font-size:var(--wb-ui-font-size,13px); color:var(--wb-fg); }
:host([hidden]) { display:none !important; }
[hidden] { display:none !important; }
dialog { box-sizing:border-box; width:min(var(--wb-dialog-width,420px),calc(100vw - 32px)); max-height:calc(100vh - 48px);
  padding:0; color:var(--wb-menu-fg,var(--wb-fg)); background:var(--wb-menu-bg,var(--wb-panel));
  border:1px solid var(--wb-line); border-radius:12px; box-shadow:0 18px 48px #0008; }
dialog::backdrop { background:#0007; }
form { display:flex; flex-direction:column; gap:14px; margin:0; padding:20px; }
h2 { margin:0; font-size:15px; font-weight:600; }
.hint { margin:-6px 0 0; color:var(--wb-fg-2); }
label, .field { display:flex; flex-direction:column; gap:6px; color:var(--wb-fg-2); font-size:12px; }
.row { display:flex; gap:12px; }
.row > * { flex:1 1 0; min-width:0; }
.length { display:flex; align-items:center; gap:8px; }
.length input[type=number] { flex:1 1 auto; min-width:0; }
input[type=text], input[type=number] { box-sizing:border-box; width:100%; min-height:30px; padding:4px 8px; font:inherit; font-size:13px;
  color:var(--wb-fg); background:var(--wb-raised); border:1px solid var(--wb-line); border-radius:6px; }
input:disabled { opacity:.55; }
input:focus-visible, button:focus-visible { outline:2px solid var(--wb-focus); outline-offset:-1px; }
.check { flex-direction:row; align-items:center; gap:8px; color:var(--wb-fg); font-size:13px; }
.inline { flex-direction:row; align-items:center; gap:4px; color:var(--wb-fg-2); white-space:nowrap; font-size:12px; }
.icon-field { display:flex; align-items:center; gap:8px; }
.preview { display:grid; place-items:center; flex:none; width:30px; height:30px; color:var(--wb-fg); background:var(--wb-raised); border-radius:6px; }
svg { display:block; }
.problem { min-height:18px; margin:0; color:var(--wb-error); }
.problem:empty { visibility:hidden; }
.notice { margin:0; padding:8px 10px; color:var(--wb-fg-2); background:var(--wb-raised); border-radius:6px; }
.notice:empty { display:none; }
.actions { display:flex; justify-content:flex-end; gap:8px; }
button { font:inherit; color:inherit; cursor:pointer; }
button:disabled { cursor:not-allowed; opacity:.5; }
.secondary, .primary { min-height:30px; padding:4px 14px; border-radius:6px; }
.secondary { background:var(--wb-raised); border:1px solid var(--wb-line); }
.primary { color:var(--wb-button-fg); background:var(--wb-button-bg); border:0; font-weight:600; }
@media (forced-colors:active) { dialog { border:1px solid CanvasText; } }
`;
