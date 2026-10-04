import { activeProject, activeSession } from './selectors.ts';
import { renderProjects } from './projects.ts';
import { renderSessions } from './sessions.ts';
import { renderRuntime } from './runtime.ts';
import { element } from './dom.ts';
import type { StudioSnapshot } from '../../src/application/types.ts';
import type { WorkspaceMode } from '../contracts.ts';
export function render({
  state,
  mode,
  opening,
  select,
  close,
}: {
  state?: StudioSnapshot;
  mode: WorkspaceMode;
  opening: boolean;
  select: (projectId: string, sessionId?: string) => void;
  close: (projectId: string) => void;
}) {
  if (!state) return;
  const project = activeProject(state);
  const session = activeSession(state);
  renderProjects(state, select, close);
  renderSessions(project, session, select);
  renderRuntime(session, opening);
  const text = (
    id: 'project-name' | 'session-name' | 'branch' | 'checkout' | 'status',
    value: string,
  ) => {
    element(id).textContent = value;
  };
  text('project-name', project?.name || 'Studio');
  text('session-name', session?.name || 'Choose a session');
  text('branch', session?.branch || '');
  text('checkout', session?.checkout || '');
  text(
    'status',
    opening
      ? 'Working…'
      : `${state.projects.length} projects · ${state.projects.flatMap((item) => item.sessions).filter((item) => item.serviceStatus === 'running').length} runtimes running`,
  );
  element('new-session').disabled = !project || opening;
  element('open-workspace').disabled = !session || opening;
  element('open-project').disabled = opening;
  for (const button of document.querySelectorAll<HTMLButtonElement>('[data-mode]')) {
    button.classList.toggle('selected', button.dataset.mode === mode);
    button.disabled =
      !session ||
      opening ||
      (button.dataset.mode === 'preview' && session.serviceStatus !== 'running');
  }
}
