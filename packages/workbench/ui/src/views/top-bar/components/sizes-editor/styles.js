import { dialogStyles } from '../../../../components/shared/dialog-styles.js';
export const styles =
  dialogStyles +
  `
:host { --wb-dialog-width: 840px; }
.rows { display:flex; flex-direction:column; gap:4px; margin:0; padding:0; list-style:none; max-height:min(420px,calc(100vh - 260px)); overflow:auto; }
.size { display:grid; grid-template-columns:30px minmax(100px,1.3fr) minmax(140px,1fr) minmax(140px,1fr) minmax(100px,.9fr) 52px 86px;
  align-items:center; gap:8px; padding:6px; border-radius:8px; }
.size:hover { background:var(--wb-hover); }
.size.local { color:var(--wb-fg-2); }
.head { color:var(--wb-fg-3); font-size:11px; font-weight:600; letter-spacing:.06em; text-transform:uppercase; }
.head:hover { background:none; }
.none { color:var(--wb-fg-3); font-size:12px; }
.tools { display:flex; gap:2px; }
.tool { display:grid; place-items:center; width:26px; height:26px; padding:0; color:var(--wb-fg-2); background:none; border:0; border-radius:6px; }
.tool:hover:not(:disabled) { color:var(--wb-fg); background:var(--wb-raised); }
.note { grid-column:2 / -1; margin:-2px 0 0; color:var(--wb-fg-3); font-size:12px; }
`;
