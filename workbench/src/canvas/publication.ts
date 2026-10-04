import type { CanvasState, Space } from './model.ts';
import { report } from './review.ts';
interface Request { id: string; operation: 'context'; body: { client: string; closed?: boolean; view?: unknown; activity?: number } }
interface Snapshot { state: CanvasState; browsed: string; spaces: readonly Space[]; activity: number }
export function spaceIds(snapshot: Snapshot): Set<string> {
  const ids = new Set([snapshot.browsed, ...snapshot.state.artboards.map(b => b.target.space.id)].filter(Boolean));
  const roots = new Set(snapshot.spaces.filter(p => ids.has(p.id)).map(p => p.root).filter(Boolean));
  // Sibling servers share their root's announcement file. Any address Shield
  // discovers must expose the same complete canvas.
  for (const s of snapshot.spaces) if (s.root && roots.has(s.root)) ids.add(s.id);
  return ids;
}
export function createPublisher(ports: { read(): Snapshot; send(request: Request): Promise<unknown>; withdraw(request: Request): void; error(error: unknown): void }) {
  const published = new Set<string>();
  let disposed = false, queued = Promise.resolve(), debounce: ReturnType<typeof setTimeout> | undefined;
  function publish() {
    queued = queued.catch(() => {}).then(async () => {
      if (disposed) return;
      const snapshot = ports.read(), ids = spaceIds(snapshot);
      for (const id of published) if (!ids.has(id)) {
        if (disposed) return;
        await ports.send({ id, operation: 'context', body: { client: snapshot.state.id, closed: true } }); published.delete(id);
      }
      const view = report(snapshot.state);
      for (const id of ids) {
        if (disposed) return;
        // Record attempted publication as well: disposal withdraws a request
        // that might have reached its server before its response arrived.
        published.add(id);
        await ports.send({ id, operation: 'context', body: { client: snapshot.state.id, view, activity: snapshot.activity } });
      }
    });
    return queued;
  }
  const heartbeat = setInterval(() => { void publish().catch(ports.error); }, 30000);
  return { publish,
    schedule() { clearTimeout(debounce); debounce = setTimeout(() => { void publish().catch(ports.error); }, 150); },
    dispose() {
      if (disposed) return; disposed = true; clearInterval(heartbeat); clearTimeout(debounce);
      const client = ports.read().state.id;
      for (const id of published) ports.withdraw({ id, operation: 'context', body: { client, closed: true } });
      published.clear();
    },
  };
}
