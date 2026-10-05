export const styles = `
:host { display:inline-grid; flex:none; width:22px; height:22px; }
:host([hidden]) { display:none; }
.mark { display:grid; place-items:center; width:100%; height:100%; overflow:hidden;
  border-radius:var(--wb-mark-radius,6px); color:white; font-size:var(--wb-mark-font-size,12px);
  font-weight:650; line-height:1; }
img { display:block; width:100%; height:100%; object-fit:contain; }
.colored img { width:76%; height:76%; }
svg { width:62%; height:62%; }
`;
