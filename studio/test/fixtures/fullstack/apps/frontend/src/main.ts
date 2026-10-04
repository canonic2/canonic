export const frontendLabel = 'Vite frontend';
interface SessionInfo {
  sessionId: string;
  title: string;
  counter: number;
  checkout: string;
  pid: number;
  backendPort: number;
  redisPort: number;
}
declare global {
  interface Window {
    studioInfo: SessionInfo;
  }
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const value = document.querySelector<T>(selector);
  if (!value) throw new Error(`Missing fixture element: ${selector}`);
  return value;
}
async function update(method = 'GET') {
  const response = await fetch(method === 'POST' ? '/api/increment' : '/api/health', { method });
  if (!response.ok) throw new Error('Backend request failed');
  const info = (await response.json()) as SessionInfo;
  element('#title').textContent = info.title;
  element('#counter').textContent = `${frontendLabel} · Counter: ${info.counter}`;
  element('#identity').textContent = JSON.stringify(info, null, 2);
  window.studioInfo = info;
}
element<HTMLButtonElement>('#increment').onclick = () => update('POST');
await update();
if (import.meta.hot) import.meta.hot.accept();
