export const styles = `
:host { display:block; position:fixed; z-index:40; top:58px; right:12px;
  box-sizing:inherit; width:280px; max-width:calc(100vw - 24px); max-height:calc(100vh - 90px);
  overflow:auto; padding:16px; border:1px solid var(--wb-line,#ddd); border-radius:var(--wb-radius-8,8px);
  background:var(--wb-panel,#fff); color:var(--wb-fg,#222); box-shadow:0 8px 30px #0004;
  font:12px var(--wb-ui-font,system-ui,sans-serif); }
:host([hidden]) { display:none; }
h2 { margin:0 0 16px; font-size:14px; } h3 { font-size:12px; margin-top:20px; }
label { display:flex; flex-direction:column; gap:6px; margin-bottom:14px; }
input, select, textarea, button { font:inherit; color:inherit; background:var(--wb-raised,#f5f5f5);
  border:1px solid var(--wb-line,#ddd); border-radius:4px; padding:6px; }
input[type=checkbox] { align-self:flex-start; }
ol, details { padding-left:18px; overflow-wrap:anywhere; }
:focus-visible { outline:2px solid var(--wb-focus,#2675df); outline-offset:2px; }
`;
