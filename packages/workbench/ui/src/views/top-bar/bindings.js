import { createMenuBehavior } from '../../components/shared/menu-behavior.js';
import { createTopBarPresentation } from './presentation.js';

/** Connect supplied top-bar targets to their sole state owner. */
export function bindTopBar(elements, owner, { signal, report }) {
  if (signal.aborted) return;
  const failed = new AbortController(),
    own = AbortSignal.any([signal, failed.signal]);
  const document = elements.lensesBox.ownerDocument;
  const view = createTopBarPresentation({ ...elements, document });
  function present(error) {
    if (own.aborted) return globalThis.reportError(error);
    try {
      report(error);
    } catch (failure) {
      globalThis.reportError(failure);
    }
  }
  function act(run) {
    try {
      Promise.resolve(run()).catch(present);
    } catch (error) {
      present(error);
    }
  }
  function menu(target) {
    return createMenuBehavior({
      host: target.control,
      root: document,
      trigger: target.button,
      menu: target.menu,
      disabled: () => target.button.disabled,
      items: () =>
        [...target.menu.querySelectorAll('button')].filter(
          (node) => !node.disabled && !node.hidden,
        ),
      preferred: () => target.menu.querySelector('[aria-current=true]:not(:disabled)'),
      toggled() {},
    });
  }
  const scenarios = menu(elements.scenario),
    sources = menu(elements.source);
  own.addEventListener(
    'abort',
    () => {
      scenarios.setOpen(false, false, elements.scenario.menu.contains(document.activeElement));
      sources.setOpen(false, false, elements.source.menu.contains(document.activeElement));
    },
    { once: true },
  );
  try {
    owner.state.watch(owner.select.lenses, view.lenses, { signal: own });
    owner.state.watch(
      owner.select.scenarios,
      (value) => {
        view.scenarios(value);
        scenarios.setOpen(false);
      },
      { signal: own },
    );
    owner.state.watch(
      owner.select.sources,
      (value) => {
        view.sources(value);
        sources.setOpen(false);
      },
      { signal: own },
    );
    owner.state.watch(
      owner.select.actions,
      (value) => {
        elements.actions.checked = value.checked;
        elements.actions.disabled = value.disabled;
        elements.actions.hint = value.hint;
      },
      { signal: own },
    );
    owner.state.watch(
      owner.select.page,
      (value) => {
        if (elements.crumb.hidden !== !value.visible) elements.crumb.hidden = !value.visible;
        if (elements.pageName.textContent !== value.label)
          elements.pageName.textContent = value.label;
      },
      { signal: own },
    );
    owner.state.watch(
      owner.select.openLink,
      (value) => {
        if (value) {
          if (elements.open.getAttribute('href') !== value)
            elements.open.setAttribute('href', value);
        } else if (elements.open.hasAttribute('href')) elements.open.removeAttribute('href');
      },
      { signal: own },
    );
    scenarios.connect(own);
    sources.connect(own);
    elements.lensesBox.addEventListener(
      'click',
      (event) => {
        const target = event.target.closest('button[data-lens]');
        if (target) act(() => owner.chooseLens(target.dataset.lens));
      },
      { signal: own },
    );
    elements.scenario.button.addEventListener(
      'click',
      (event) => {
        sources.setOpen(false);
        scenarios.setOpen(!scenarios.open, true, event.detail === 0);
      },
      { signal: own },
    );
    elements.source.button.addEventListener(
      'click',
      (event) => {
        scenarios.setOpen(false);
        sources.setOpen(!sources.open, true, event.detail === 0);
      },
      { signal: own },
    );
    elements.scenario.menu.addEventListener(
      'click',
      (event) => {
        const target = event.target.closest('button[data-scenario]');
        if (!target || target.disabled) return;
        scenarios.setOpen(false, true, true);
        act(() => owner.chooseScenario(target.dataset.scenario || null));
      },
      { signal: own },
    );
    elements.source.menu.addEventListener(
      'click',
      (event) => {
        const target = event.target.closest('button[data-source]');
        if (!target || target.disabled) return;
        sources.setOpen(false, true, true);
        act(() => owner.chooseSource(target.dataset.source));
      },
      { signal: own },
    );
    elements.actions.addEventListener(
      'wb-actions-toggle',
      (event) => act(() => owner.chooseActions(event.detail.checked)),
      { signal: own },
    );
    elements.reload.addEventListener('click', () => act(() => owner.reload()), { signal: own });
  } catch (error) {
    failed.abort();
    throw error;
  }
}
