import type { SessionSnapshot } from '../../src/application/types.ts';
import { element } from './dom.ts';
export function renderRuntime(session: SessionSnapshot | undefined, busy: boolean) {
  const running = session?.serviceStatus === 'running';
  const pending = session?.serviceStatus === 'starting' || session?.serviceStatus === 'stopping';
  element('service-status').textContent = session
    ? `Runtime ${session.serviceStatus}`
    : 'No session selected';
  element('endpoint').textContent = session?.serviceUrl || '';
  element('toggle-service').textContent = running ? 'Stop runtime' : 'Start runtime';
  element('toggle-service').disabled = !session || pending || busy;
  const endpoints = Object.entries(session?.ports || {})
    .map(([name, port]) => `${name}: ${port}`)
    .join('\n');
  element('endpoint').title = endpoints;
}
