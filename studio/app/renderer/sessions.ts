import type { ProjectSnapshot, SessionSnapshot } from '../../src/application/types.ts';
import { element } from './dom.ts';
export function renderSessions(
  project: ProjectSnapshot | undefined,
  session: SessionSnapshot | undefined,
  select: (projectId: string, sessionId?: string) => void,
) {
  element('sessions').replaceChildren(
    ...(project?.sessions || []).map((item) => {
      const button = document.createElement('button');
      button.className = `session-button${item.id === session?.id ? ' active' : ''}`;
      button.setAttribute('aria-pressed', String(item.id === session?.id));
      const title = document.createElement('strong');
      title.textContent = item.name;
      const branch = document.createElement('small');
      branch.textContent = item.branch;
      const status = document.createElement('small');
      status.textContent = item.serviceStatus;
      button.append(title, branch, status);
      button.onclick = () => select(project!.id, item.id);
      return button;
    }),
  );
}
