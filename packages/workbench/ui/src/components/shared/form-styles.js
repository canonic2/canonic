/** Shared presentation for native form fields; no input wrappers or behavior. */
export const formStyles = `
label, .field { display:flex; flex-direction:column; gap:6px; color:var(--wb-fg-2); font-size:12px; }
input[type=text], input[type=number] { box-sizing:border-box; width:100%; min-height:30px; padding:4px 8px; font:inherit; font-size:13px;
  color:var(--wb-fg); background:var(--wb-raised); border:1px solid var(--wb-line); border-radius:6px; }
select { font:inherit; padding:6px; color:var(--wb-fg,#222); background:var(--wb-raised,#fff); border:1px solid var(--wb-line,#bbb); border-radius:6px; }
input:disabled { opacity:.55; }
input:focus-visible, button:focus-visible { outline:2px solid var(--wb-focus); outline-offset:-1px; }
`;
