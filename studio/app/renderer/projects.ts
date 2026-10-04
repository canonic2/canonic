import type { StudioSnapshot } from '../../src/application/types.ts';
import { element } from './dom.ts';
export function renderProjects(
  state: StudioSnapshot,
  select: (projectId: string, sessionId?: string) => void,
  close: (projectId: string) => void,
) {
  element('projects').replaceChildren(
    ...state.projects.map((project) => {
      const tab = document.createElement('div');
      tab.className = `project-tab${project.id === state.activeProjectId ? ' active' : ''}`;
      const button = document.createElement('button');
      button.textContent = project.name;
      button.setAttribute('aria-pressed', String(project.id === state.activeProjectId));
      button.onclick = () => select(project.id);
      const dismiss = document.createElement('button');
      dismiss.textContent = '×';
      dismiss.setAttribute('aria-label', `Close ${project.name}`);
      dismiss.title = 'Stop this project’s sessions and close its tab';
      dismiss.onclick = () => close(project.id);
      tab.append(button, dismiss);
      return tab;
    }),
  );
}
