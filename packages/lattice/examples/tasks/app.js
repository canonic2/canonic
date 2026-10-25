/** @file Compose an independently owned task capability and one rendered list lifetime. */
import { createTasks } from './tasks.js';
import { TaskList, defineTaskList } from './task-list.js';
import { bindTaskList } from './bindings.js';
import { coordinateTaskList } from './list-view.js';

/**
 * @typedef {object} TaskView
 * @property {import('./tasks.js').Tasks} tasks Application state/actions; may outlive the view.
 * @property {TaskList} list Plain-input presentation.
 * @property {HTMLOutputElement} summary Native collection-wide summary.
 * @property {() => void} render Reconcile the requested window in application composition.
 * @property {(key: string) => void} focusKey Mount and focus a keyboard/command destination.
 */

/**
 * Attach presentation and bindings without taking ownership of capability state.
 * @param {HTMLElement} container Example host.
 * @param {{ signal: AbortSignal, tasks?: import('./tasks.js').Tasks, report?: (error: unknown) => void }} options Ports and view lifetime.
 * @returns {TaskView} Presentation and capability handles for application commands.
 */
export function mountTasks(container, { signal, tasks = createTasks(), report = reportError }) {
  defineTaskList();
  const list = new TaskList(), summary = document.createElement('output');
  if (signal.aborted) return { tasks, list, summary, render: () => {}, focusKey: () => {} };
  const failed = new AbortController(), own = AbortSignal.any([signal, failed.signal]);
  /** @type {ReturnType<typeof coordinateTaskList> | null} */
  let retention = null;
  /** Remove the view's owned DOM when its scope ends. @returns {void} */
  const remove = () => {
    try { retention?.dispose(); } catch (error) { try { report(error); } catch (failure) { reportError(failure); } }
    finally { list.dispose(); list.remove(); summary.remove(); }
  };
  own.addEventListener('abort', remove, { once: true });
  try {
    container.append(summary, list);
    bindTaskList(list, summary, tasks, own);
    retention = coordinateTaskList(list, tasks, { signal: own, report });
  } catch (error) { failed.abort(); throw error; }
  return { tasks, list, summary, render: retention.render, focusKey: retention.focusKey };
}
