import { createStore, shallow } from '@canonic2/lattice';
export { createSizeControls } from './sizes.js';
import {
  DESIGN_KEY,
  pageLenses,
  hasDesignLens,
  isDocsLens,
} from '../../../../contracts/src/docs/lenses.js';

/** Top-bar choices and preferences. Project resolution supplies facts, never DOM. */
export function createTopBar(ports = {}) {
  const store = createStore({
    src: null,
    scenario: null,
    example: null,
    iterations: {},
    lensPref: null,
    actionsOn: false,
    lenses: [],
    lens: null,
    scenarios: null,
    scenarioKind: 'scenario',
    sources: [],
    sourceAvailable: false,
    openLink: '',
    pageLabel: '',
  });
  /** @type {import('@canonic2/lattice').StoreReader<ReturnType<typeof store.get>>} */
  const state = { get: store.get, watch: store.watch };
  const remember = (name, value) => ports.remember?.(name, value);
  function commit(patch) {
    const before = store.get();
    for (const field of ['lenses', 'scenarios', 'sources']) {
      const value = patch[field],
        previous = before[field];
      if (
        Array.isArray(value) &&
        Array.isArray(previous) &&
        value.length === previous.length &&
        value.every((row, at) => shallow(row, previous[at]))
      )
        patch[field] = previous;
    }
    if (patch.lens && shallow(patch.lens, before.lens)) patch.lens = before.lens;
    store.update(patch);
  }
  const setLensPref = (value) => {
    const lensPref = value || null;
    if (lensPref === state.get().lensPref) return;
    commit({ lensPref });
    remember('lens', lensPref || '');
  };
  const setActions = (value) => {
    const actionsOn = !!value;
    if (actionsOn === state.get().actionsOn) return;
    commit({ actionsOn });
    remember('actions', actionsOn ? 'on' : 'off');
  };
  function write(scenario, lens, example, iterations) {
    ports.writeHash?.(state.get().src, scenario, ports.size?.(), lens, example, iterations);
  }
  return {
    state,
    select: {
      lenses: {
        fields: ['lenses', 'lens'],
        read: (s) => ({ choices: s.lenses, showing: s.lens?.key || DESIGN_KEY }),
        equals: shallow,
      },
      scenarios: {
        fields: ['scenarios', 'scenarioKind'],
        read: (s) => ({ rows: s.scenarios, kind: s.scenarioKind }),
        equals: shallow,
      },
      sources: {
        fields: ['sources', 'sourceAvailable'],
        read: (s) => ({ rows: s.sources, available: s.sourceAvailable }),
        equals: shallow,
      },
      actions: {
        fields: ['actionsOn', 'lens'],
        read: (s) => {
          const locked = !!s.lens && !['workbench', 'docs'].includes(s.lens.kind);
          return {
            checked: locked || s.actionsOn,
            disabled: locked,
            hint: locked
              ? 'always on here: ' +
                s.lens.label +
                ' serves this page, not the workbench, so nothing inside it is switched off'
              : '',
          };
        },
        equals: shallow,
      },
      page: {
        fields: ['src', 'pageLabel'],
        read: (s) => ({ visible: !!s.src, label: s.pageLabel }),
        equals: shallow,
      },
      openLink: { fields: ['openLink'], read: (s) => s.openLink },
    },
    setPage: (src) => commit({ src: src || null }),
    setScenario: (scenario) => commit({ scenario: scenario || null }),
    setExample: (example) => commit({ example: example || null }),
    setIterations: (iterations) => commit({ iterations: iterations || {} }),
    setLensPref,
    setActions,
    restore({ lens, actions }) {
      commit({ lensPref: lens || null, actionsOn: actions === 'on' });
    },
    showPage(item) {
      commit({ pageLabel: item?.label || '' });
    },
    showLenses(item, implementations, lens) {
      const lenses = pageLenses(item, implementations);
      const choices = hasDesignLens(item)
        ? [...lenses.filter(isDocsLens), null, ...lenses.filter((impl) => !isDocsLens(impl))]
        : lenses;
      commit({
        lens: lens || null,
        lenses: choices.map((impl) => ({
          key: impl?.key || DESIGN_KEY,
          label: impl?.label || ports.authoredLabel?.(item) || 'Design',
          title: impl ? 'This page as ' + impl.label + ' has it' : 'This page as designed',
        })),
      });
    },
    showScenarios(item, lens, scenarios) {
      const mapped =
        !lens ||
        item.implementationOnly ||
        isDocsLens(lens) ||
        !!item.implementations?.[lens.key]?.scenarios;
      const streamed = lens && ['ios-simulator', 'window'].includes(lens.kind);
      commit({
        scenarioKind: 'scenario',
        scenarios:
          !scenarios || !mapped || streamed
            ? null
            : scenarios.map((scenario, at) => ({
                id: scenario.id,
                label: scenario.label,
                target: at === 0 ? null : scenario.id,
                current: state.get().scenario ? state.get().scenario === scenario.id : at === 0,
              })),
      });
    },
    showStories(list, picked) {
      commit({
        scenarioKind: 'story',
        scenarios:
          list?.map((story) => ({
            id: story.id,
            label: story.name,
            target: story.scenario,
            current: story.id === picked.id,
          })) || null,
      });
    },
    clearScenarios() {
      commit({ scenarios: null });
    },
    showSources(item, info, implementations) {
      const rows = [];
      if (info?.design)
        rows.push({
          id: 'design',
          label: hasDesignLens(item) ? ports.authoredLabel?.(item) || 'Design' : 'Docs',
          sub: item.src,
          enabled: true,
          title: info.design,
          path: null,
        });
      const lenses = pageLenses(item, implementations);
      for (const entry of info?.code || []) {
        const impl =
          lenses.find((lens) => lens.key === entry.implementation) ||
          implementations[entry.implementation];
        rows.push({
          id: JSON.stringify([entry.implementation, entry.relative, entry.path]),
          label: entry.label || impl?.label || entry.implementation,
          sub: entry.relative,
          enabled: !!entry.exists,
          title: !entry.path
            ? 'Implementation “' + entry.implementation + '” has no root, so this can’t be found'
            : entry.exists
              ? entry.path
              : 'Not on this machine: ' + entry.path,
          path: entry.path,
        });
      }
      commit({ sources: rows, sourceAvailable: !!info });
    },
    setOpenLink(url) {
      commit({ openLink: url || '' });
    },
    chooseLens(key) {
      if (!state.get().lenses.some((lens) => lens.key === key)) return;
      setLensPref(key);
      const { scenario, example, iterations } = state.get();
      write(scenario, key, example, iterations);
    },
    chooseScenario(target) {
      const row = state.get().scenarios?.find((row) => (row.target || null) === (target || null));
      if (row) write(row.target, state.get().lens?.key || null);
    },
    chooseActions(value) {
      if (state.get().lens && !['workbench', 'docs'].includes(state.get().lens.kind)) return;
      setActions(value);
      if (state.get().src) return ports.actionsChanged?.();
    },
    chooseSource(id) {
      const source = state.get().sources.find((source) => source.id === id);
      if (source?.enabled) return ports.openSource?.(state.get().src, source.path);
    },
    reload: () => ports.reload?.(),
  };
}
