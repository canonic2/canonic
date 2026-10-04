import type { StudioSnapshot } from '../../src/application/types.ts';
export const activeProject = (state?: StudioSnapshot) =>
  state?.projects.find((project) => project.id === state.activeProjectId);
export const activeSession = (state?: StudioSnapshot) => {
  const project = activeProject(state);
  return project?.sessions.find((session) => session.id === project.activeSessionId);
};
