/** @file Compose an independently owned task capability and one rendered list lifetime. */
import { createTasks } from './tasks.js';
import { TaskList, defineTaskList } from './task-list.js';
import { bindTaskList, bindTaskRow } from './bindings.js';

/**
 * @typedef {object} TaskView
 * @property {import('./tasks.js').Tasks} tasks Application state/actions; may outlive the view.
 * @property {TaskList} list Plain-input presentation.
 * @property {HTMLOutputElement} summary Native collection-wide summary.
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
  if (signal.aborted) return { tasks, list, summary };
  const failed = new AbortController(), own = AbortSignal.any([signal, failed.signal]);
  list.bindItem = (key, row, rowSignal) => bindTaskRow(row, tasks, key, { signal: rowSignal, report });
  /** Remove the view's owned DOM when its scope ends. @returns {void} */
  const remove = () => { list.dispose(); list.remove(); summary.remove(); };
  own.addEventListener('abort', remove, { once: true });
  try {
    container.append(summary, list);
    bindTaskList(list, summary, tasks, own);
  } catch (error) { failed.abort(); throw error; }
  return { tasks, list, summary };
}
