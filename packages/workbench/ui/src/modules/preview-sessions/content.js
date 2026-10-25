/** Content selection and metadata. No application controls or menu DOM. */
import {
  chooseLens,
  hasDesignLens,
  isDocsLens,
  pageLenses,
} from '../../../../contracts/src/docs/lenses.js';
import { scrollAfterZoom } from '../docs/canvas/docs-layout.js';
import { begin as beginTrace, phase, address } from '../trace/browser.js';
import { createRendererNavigation } from '../navigation/index.js';

export function createContentController(options) {
  const { frames, services } = options;
  let config = null;
  const pages = new Map();
  const params = new URLSearchParams(location.search);
  const navigation = createRendererNavigation(
    params.get('navigation'),
    services.address.parse(location.hash),
    params.get('artboard') || '',
  );
  const currentTarget = () => navigation.state.get().requested;
  // This renderer's declared artboard, or "" for a scenario's implicit one:
  // the key of its iteration in the address's `+` parts.
  const artboardId = params.get('artboard') || '';
  let trace = beginTrace(
    currentTarget(),
    {
      selection: params.get('wbTrace'),
      started: Number(params.get('wbTraceStart')) || undefined,
    },
    'renderer',
  );
  phase(trace, 'renderer-ready');
  let view = null;
  let mounted = null;
  let problem = null;
  let rows = [];
  let generation = 0;
  let closed = false;
  let actions = false;
  let width = innerWidth,
    height = innerHeight;
  function changed() {
    window.dispatchEvent(new Event('wb-view-change'));
  }
  function fail(error) {
    problem = error instanceof Error ? error.message : String(error);
    options.viewport.classList.add('is-pending');
    options.viewport.classList.add('has-problem');
    changed();
  }
  function flags(src, scenario, iteration) {
    const url = new URL(src, options.root);
    url.searchParams.set('actions', actions ? 'on' : 'off');
    if (scenario) url.searchParams.set('scenario', scenario);
    if (artboardId) url.searchParams.set('artboard', artboardId);
    if (iteration) url.searchParams.set('iteration', iteration);
    return address(url.href, trace);
  }
  // A scenario as the page index lists it, with its artboards and iterations:
  // a preview's from its definition, an authored page's from its config item.
  function scenarioEntry(item, scenario) {
    const list = config?.pages[item.src]?.preview?.scenarios || item.scenarios || [];
    return scenario ? list.find((s) => s.id === scenario) || null : list[0] || null;
  }
  // The iteration the address names for this artboard, when the scenario declares it.
  function iterationEntry(item, scenario) {
    const id = currentTarget().iterations?.[artboardId];
    const entry = id ? scenarioEntry(item, scenario) : null;
    if (!entry) return null;
    const owner = artboardId
      ? entry.artboards?.find((board) => board.id === artboardId)
      : entry.artboards?.length
        ? null
        : entry;
    return owner?.iterations?.find((iteration) => iteration.id === id) || null;
  }
  function withIteration(next, iteration) {
    const rest = { ...next };
    delete rest.iteration;
    delete rest.iterations;
    return iteration ? { ...rest, iterations: { [artboardId]: iteration } } : rest;
  }
  function write(next) {
    navigation.resolved(next);
    if (next.trace) {
      trace = beginTrace(next, next.trace, 'renderer');
      phase(trace, 'renderer-ready', { retained: true });
    }
    const hash = '#' + services.address.write(next);
    // Commands route immediately; hashchange is for external navigation only.
    history.replaceState(null, '', location.pathname + location.search + hash);
    window.dispatchEvent(new Event('wb-content-destination'));
  }
  function reveal() {
    const target = currentTarget();
    if (!target.example) return;
    try {
      const preview = frames.frame().contentWindow;
      preview?.wbDocsPage?.scrollToExample(target.example);
    } catch {
      /* Only same-origin docs expose this port. */
    }
  }
  async function show(force = false) {
    if (!config || closed) return;
    const target = currentTarget();
    const sequence = ++generation;
    mounted = null;
    problem = null;
    options.viewport.classList.remove('has-problem');
    rows = [];
    options.viewport.classList.add('is-pending');
    const item = pages.get(target.src);
    view = null;
    changed();
    if (!item) {
      frames.suspend();
      services.native.hide();
      fail(new Error('Page not found: ' + target.src));
      return;
    }
    const lens = chooseLens(item, config.implementations, target.lens);
    const storybook = lens?.kind === 'storybook';
    const scenario =
      lens && ['ios-simulator', 'window'].includes(lens.kind)
        ? null
        : storybook
          ? target.scenario
          : item.scenarios?.slice(1).find((s) => s.id === target.scenario)?.id || null;
    // Iterations belong to the design; other lenses show the authored artboard
    // and keep the address's choice for the way back.
    const iteration = lens ? null : iterationEntry(item, scenario);
    view = {
      src: item.src,
      item,
      lens,
      scenario: storybook ? null : scenario,
      scenarioLabel: scenario
        ? item.scenarios?.find((s) => s.id === scenario)?.label || null
        : null,
      iteration,
      story: null,
      url: null,
      code: (config.pages[item.src]?.code || []).filter((c) => c.exists).map((c) => c.path),
    };
    if (iteration?.source && !view.code.includes(iteration.source))
      view.code.unshift(iteration.source);
    const destination = { ...target, src: item.src, scenario, lens: lens?.key || 'design' };
    write(lens ? destination : withIteration(destination, iteration?.id));
    if (lens?.problem) {
      frames.suspend();
      services.native.hide();
      fail(new Error(lens.problem));
      return;
    }
    options.surface.dataset.contentMode = isDocsLens(lens) ? 'docs' : 'preview';
    if (lens && ['ios-simulator', 'window'].includes(lens.kind)) {
      frames.suspend();
      services.native.show({
        kind: lens.kind,
        implementation: lens.key,
        udid: item.implementations?.[lens.key]?.device,
        src: item.src,
        label: item.label,
      });
      options.viewport.classList.remove('is-pending');
      changed();
      return;
    }
    services.native.hide();
    if (storybook && lens) {
      try {
        const list = await options.stories(lens.key, item.implementations?.[lens.key]?.title || '');
        if (closed || sequence !== generation) return;
        const picked = list.find((s) => s.scenario === scenario) || list[0];
        if (!picked) throw new Error('No stories available.');
        view.story = picked;
        view.scenarioLabel = picked.name;
        view.url = services.lenses.storyUrl(lens, picked.id);
        view.code = [...new Set([...(picked.code || []), ...view.code])];
        write({ ...currentTarget(), scenario: picked.scenario });
        rows = list.map((s, i) => ({
          id: String(i),
          target: s.scenario,
          label: s.name,
          current: s.id === picked.id,
        }));
      } catch (error) {
        if (!closed && sequence === generation) fail(error);
        return;
      }
    } else {
      const mapped = !lens || isDocsLens(lens) || !!item.implementations?.[lens.key]?.scenarios;
      rows =
        mapped && (item.scenarios?.length || 0) > 1
          ? item.scenarios.map((s, i) => ({
              id: String(i),
              target: i ? s.id : null,
              label: s.label,
              current: i ? s.id === scenario : !scenario,
            }))
          : [];
      if (isDocsLens(lens)) {
        const url = new URL(flags(item.markdown, scenario));
        url.searchParams.set('lens', lens.key);
        view.url = url.href;
      } else if (!lens) view.url = flags(item.src, scenario, iteration?.id);
      else {
        view.url = services.lenses.url(lens, item.implementations[lens.key], scenario);
        if (lens.kind === 'workbench') view.url = flags(view.url, scenario);
      }
    }
    changed();
    mounted = view;
    if (trace) {
      trace.page = item.src;
      trace.scenario = scenario || 'default';
      trace.lens = view.lens?.key || 'design';
      view.trace = trace;
      view.phase = phase;
    }
    frames.show(view, force);
  }
  function navigate(next, incoming) {
    const target = currentTarget();
    const same =
      next.src === target.src &&
      (next.scenario || null) === (target.scenario || null) &&
      (next.lens || 'design') === (target.lens || 'design') &&
      (next.example || null) === (target.example || null) &&
      (next.iterations?.[artboardId] || null) === (target.iterations?.[artboardId] || null);
    if (same && content.read()?.ready) {
      if (incoming) {
        navigation.identify(incoming);
        changed();
      }
      return;
    }
    navigation.picked(next, incoming);
    write(next);
    void show().catch(fail);
  }
  const content = {
    identity: navigation.identity,
    identify(id) {
      navigation.identify(id);
      changed();
    },
    problem: () => problem,
    read() {
      if (!view || !config) return null;
      const target = currentTarget();
      const { item, lens, scenarioLabel, iteration } = view;
      const declared = artboardId
        ? scenarioEntry(item, view.scenario)?.artboards?.find((b) => b.id === artboardId)
        : null;
      const disabled = !!lens && !['workbench', 'docs'].includes(lens.kind);
      const info = {
        src: item.src,
        scenario: target.scenario,
        lens: lens?.key || null,
        iteration: iteration?.id || null,
        ...(iteration ? { iterationLabel: iteration.label || iteration.id } : {}),
        hash: location.hash,
        label: declared
          ? item.label + ' / ' + declared.label
          : [item.label, scenarioLabel].filter(Boolean).join(' / '),
        ready:
          !problem &&
          (lens && ['ios-simulator', 'window'].includes(lens.kind)
            ? services.native.ready()
            : mounted === view && frames.ready()),
        scenarios: rows,
        lenses: [
          ...pageLenses(item, config.implementations).filter(isDocsLens),
          ...(hasDesignLens(item) ? [{ key: 'design', label: 'Design' }] : []),
          ...pageLenses(item, config.implementations).filter((l) => !isDocsLens(l)),
        ].map((l) => ({ id: l.key, label: l.label })),
        sizes: isDocsLens(lens) ? undefined : config.pages[item.src]?.sizes || config.sizes,
        actions: { checked: disabled || actions, disabled },
      };
      return { ...info, ...navigation.report({ ...info, problem }) };
    },
    navigate(value) {
      if (!config) throw new Error('The space is still loading.');
      if (!value || typeof value !== 'object' || !('src' in value) || typeof value.src !== 'string')
        throw new Error('Invalid content target.');
      const next = value;
      navigate(
        withIteration(
          { ...next, scenario: next.scenario || null, lens: next.lens || 'design' },
          typeof next.iteration === 'string' ? next.iteration : null,
        ),
        next.navigation,
      );
    },
    size(w, h) {
      width = w;
      height = h;
      options.surface.style.width = w + 'px';
      options.surface.style.height = h + 'px';
      changed();
    },
    scroll(value) {
      if (!isDocsLens(view?.lens)) return;
      const doc = frames.frame()?.contentWindow;
      if (!doc) return;
      if (Number.isFinite(value?.dy)) doc.scrollBy(0, value.dy);
      else if (
        Number.isFinite(value?.from) &&
        value.from > 0 &&
        Number.isFinite(value.to) &&
        value.to > 0 &&
        Number.isFinite(value.pointY)
      )
        doc.scrollTo(doc.scrollX, scrollAfterZoom(doc.scrollY, value.pointY, value.from, value.to));
    },
    pickScenario(id) {
      const row = rows.find((r) => r.id === id);
      if (row) navigate(withIteration({ ...currentTarget(), scenario: row.target || null }, null));
    },
    lens(key) {
      navigate({ ...currentTarget(), lens: key || 'design' });
    },
    reload() {
      navigation.picked(currentTarget());
      if (view?.lens && ['ios-simulator', 'window'].includes(view.lens.kind))
        services.native.reload();
      else void show(true).catch(fail);
    },
    refresh() {
      navigation.picked(currentTarget());
      void load().catch(fail);
    },
    action(name, value) {
      if (name === 'actions') {
        if (actions === !!value) return;
        actions = !!value;
        // External implementations are always interactive. Updating the owning
        // canvas's preference must not restart their pending or warm document.
        if (view?.lens && !['workbench', 'docs'].includes(view.lens.kind)) {
          changed();
          return;
        }
        if (!view?.url) return;
        navigation.picked(currentTarget());
        void show().catch(fail);
      }
    },
  };
  async function load() {
    const sequence = ++generation;
    const result = await options.loadConfig();
    if (closed || sequence !== generation) return;
    if (!result.ok) throw new Error(result.error || 'Could not load project data.');
    config = result;
    pages.clear();
    for (const collection of services.mergeCollections(
      result.collections,
      result.catalogCollections || [],
    ))
      for (const entry of collection.items)
        for (const page of 'group' in entry ? entry.items : [entry]) pages.set(page.src, page);
    await show();
  }
  function hashChanged() {
    if (!options.held()) {
      navigation.picked(services.address.parse(location.hash));
      void show().catch(fail);
    } else write(currentTarget());
  }
  function message(event) {
    frames.storyEvent(event);
    if (options.held() || !frames.ready() || event.source !== frames.frame().contentWindow) return;
    const data = event.data;
    if (event.origin === location.origin && data?.type === 'workbench-preview') {
      if (data.event === 'navigate') {
        const page = [...pages.values()].find(
          (p) => config?.pages[p.src]?.preview?.id === data.preview,
        );
        if (page) navigate({ src: page.src, scenario: data.scenario || null, lens: 'design' });
      } else if (data.event === 'docs-navigate' && isDocsLens(view?.lens)) {
        const page = [...pages.values()].find((p) => p.markdown === data.page);
        if (page)
          navigate({
            src: page.src,
            scenario: null,
            lens: view?.lens?.key || null,
            example: data.example,
          });
      }
    } else if (data?.type === 'wb-preview-navigate') {
      const url = new URL(String(data.href), options.root);
      if (!url.href.startsWith(options.root)) return;
      const src = decodeURIComponent(url.pathname.slice(new URL(options.root).pathname.length));
      if (pages.has(src)) navigate({ src, scenario: data.scenario || null, lens: 'design' });
    }
  }
  window.addEventListener('hashchange', hashChanged);
  window.addEventListener('message', message);
  return {
    content,
    view: () => view,
    target: currentTarget,
    item: (src) => pages.get(src),
    size: () => ({
      key: 'resizable',
      kind: 'resizable',
      label: width + ' × ' + height,
      width,
      height,
    }),
    docs: () => isDocsLens(view?.lens),
    fail,
    renderFailed(failedView, error) {
      if (!closed && failedView === mounted && failedView === view) fail(error);
    },
    reveal,
    settled() {
      if (mounted === view && frames.ready()) options.viewport.classList.remove('is-pending');
      changed();
    },
    start() {
      void load().catch(fail);
    },
    dispose() {
      if (closed) return;
      closed = true;
      generation++;
      window.removeEventListener('hashchange', hashChanged);
      window.removeEventListener('message', message);
      frames.close();
      services.native.hide();
    },
  };
}
