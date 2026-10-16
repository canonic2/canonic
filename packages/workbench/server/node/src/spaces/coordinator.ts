/** Fixed space operations. This is not a general-purpose loopback proxy. */
export interface SpaceRequest { id: string; operation: 'config' | 'context'; body?: unknown }
export function parseSpaceRequest(value: unknown): SpaceRequest {
  if (!value || typeof value !== 'object') throw new Error('Invalid space operation.');
  const ask = value as SpaceRequest;
  if (typeof ask.id !== 'string' || !ask.id || ask.id.length > 200 || !['config','context'].includes(ask.operation)) throw new Error('Invalid space operation.');
  return ask;
}
export async function coordinate(ask: SpaceRequest, open: (id: string) => Promise<{ url: string }>) {
  const opened = await open(ask.id);
  const base = new URL(opened.url);
  if (base.protocol !== 'http:' || !['127.0.0.1','localhost'].includes(base.hostname)) throw new Error('Space server must be on loopback.');
  const path = ask.operation === 'config' ? '/_workbench/config' : '/_workbench/view';
  const response = await fetch(new URL(path, base), ask.operation === 'context'
    ? { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(ask.body), signal: AbortSignal.timeout(10000) }
    : { signal: AbortSignal.timeout(30000) });
  const result = await response.json();
  if (!response.ok || result.ok === false) throw new Error(result.error || 'Space request failed.');
  return ask.operation === 'config' ? { url: opened.url, config: result } : { ok: true };
}
