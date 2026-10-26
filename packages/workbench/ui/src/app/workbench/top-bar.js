/** Compose top-bar owners and host operations once for this document. */
import { createTopBar, createSizeControls } from '../../modules/top-bar/index.js';
import { bindTopBar } from '../../views/top-bar/bindings.js';
import { bindSizes } from '../../views/top-bar/size-bindings.js';
import { bindResponsiveTopBar } from '../../views/top-bar/top-bar.js';
import { requestJson } from '../../host/http.js';

export function mountTopBar(document, window) {
  const lifetime = new AbortController(),
    signal = lifetime.signal;
  const $ = (id) => document.getElementById(id);
  const report = (error) =>
    window.dispatchEvent(
      new CustomEvent('wb-say', {
        detail: { message: error instanceof Error ? error.message : String(error) },
      }),
    );
  let project;
  const owner = createTopBar({
    authoredLabel: window.wbManifest.authoredLensLabel,
    remember(name, value) {
      try {
        window.localStorage.setItem('canonic-workbench-' + name, value);
      } catch {
        /* Preferences still work when browser storage is unavailable. */
      }
    },
    writeHash: (...args) => project.writeHash(...args),
    size: () => project.size(),
    actionsChanged: () => project.actionsChanged(),
    reload: () => project.reload(),
    async openSource(src, path) {
      try {
        const result = await requestJson('/_workbench/open', { src, path });
        if (!signal.aborted) report('Opened ' + result.opened.split('/').pop());
      } catch (error) {
        throw new Error('Couldn’t open it — ' + String(error.message || error), { cause: error });
      }
    },
  });
  const elements = {
    lensesBox: $('lenses'),
    actions: $('actionsToggle'),
    reload: $('reload'),
    open: $('open'),
    crumb: $('crumb'),
    pageName: $('crumbPage'),
    scenario: {
      control: $('scenarioControl'),
      button: $('scenarioButton'),
      menu: $('scenarioMenu'),
      name: $('scenarioName'),
    },
    source: { control: $('sourceControl'), button: $('openSource'), menu: $('sourceMenu') },
  };
  try {
    bindTopBar(elements, owner, { signal, report });
    const topbar = document.querySelector('.wb-topbar');
    bindResponsiveTopBar(
      {
        topbar,
        left: topbar.querySelector('.wb-topbar-left'),
        center: topbar.querySelector('.wb-topbar-center'),
        right: topbar.querySelector('.wb-topbar-right'),
        overflow: $('topbarOverflow'),
        button: $('topbarOverflowButton'),
        menu: $('topbarOverflowMenu'),
        sourceMenu: $('sourceMenu'),
        controls: Object.fromEntries(
          [
            'reload',
            'copyReference',
            'open',
            'previewControls',
            'configureSpace',
            'exportSpace',
          ].map((id) => [id, $(id)]),
        ),
      },
      { signal },
    );
    for (const element of [
      $('sizeSwitcher'),
      $('actionsToggle'),
      $('sizeDialog'),
      $('sizesEditor'),
    ])
      element.iconRenderer = window.wbIcon;
    // Project owns this resolver adapter until project/page-list adoption. It holds no top-bar state.
    project = window.wbCreateShell({
      topBar: owner,
      clearPreview: () => window.wbMultiCanvas?.clearControls(),
      postSizes: window.wbProjectConfiguration.postSizes,
      connectSizes(ports) {
        const sizes = createSizeControls({
          ...ports,
          report: (error) => globalThis.reportError(error),
        });
        signal.addEventListener('abort', sizes.dispose, { once: true });
        bindSizes(
          { switcher: $('sizeSwitcher'), add: $('sizeDialog'), editor: $('sizesEditor') },
          sizes,
          { signal, report },
        );
      },
    });
    window.addEventListener('pagehide', () => lifetime.abort(), { once: true });
    return owner;
  } catch (error) {
    lifetime.abort();
    throw error;
  }
}
