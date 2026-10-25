/** Connect capability owners to the existing shell and its selected artboard. */

import { bindLegacyChrome, createController, createPublisher } from '../../modules/canvas/index.js';
import { bindToolbar } from '../toolbar/bindings.js';
import {
  createRuntime,
  createPreviewSessions,
  fetchStoryCatalog,
} from '../../modules/preview-sessions/index.js';
import {
  capture,
  reference,
  createReview,
  createBrowserReview,
} from '../../modules/review/index.js';
import { createNavigation, shownIterations } from '../../modules/navigation/index.js';
import { requestJson as json } from '../../host/http.js';
import { dimensions } from '../../../../contracts/src/sizes/browser/geometry.js';

const chrome = window;
const $ = (id) => document.getElementById(id);
export function initialize() {
  if (!chrome.wbMultiArtboardHost || chrome.wbMultiCanvas) return;
  chrome.wbMultiCanvas = mount();
  window.dispatchEvent(new Event('wb-multi-ready'));
}
function mount() {
  const toolbar = chrome.wbAnnotationToolbar();
  const canvas = document.createElement('wb-canvas');
  canvas.id = 'artboards';
  canvas.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;display:none';
  $('canvas').append(canvas);
  const style = document.createElement('style');
  style.textContent = '.wb-canvas>#artboard,.wb-canvas>#artboardLabel{display:none!important}';
  document.head.append(style);
  const connection = new AbortController(),
    signal = connection.signal;
  let disposed = false;
  let fitPending = false;
  let space,
    registry = [],
    activity = Date.now();
  let previewBoard = '',
    toastTimer;
  const previews = new Map();
  const ready = json('/_workbench/spaces').then(async (listed) => {
    registry = listed.spaces;
    const resolved = await json('/_workbench/canvas/space', {
      id: listed.current,
      operation: 'config',
    });
    space = { ...registry.find((s) => s.id === listed.current), url: resolved.url };
  });
  void ready.catch(fail);
  function say(error) {
    $('toast').textContent = error instanceof Error ? error.message : String(error);
    $('toast').hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      $('toast').hidden = true;
    }, 5000);
  }
  function act(run) {
    try {
      const result = run();
      if (result instanceof Promise) void result.catch(say);
    } catch (error) {
      say(error);
    }
  }
  const selected = () =>
    controller.snapshot().artboards.find((b) => b.id === controller.snapshot().selected);
  const available = () => ({ width: $('canvas').clientWidth, height: $('canvas').clientHeight });
  // The selected renderer reports only its own iteration. The editor also
  // needs to know whether any artboard shows one: then a pick of the same
  // page and scenario returns the canvas to the authored design.
  function withShownIterations(message, selection) {
    const iterations = shownIterations(selection, controller.snapshot().artboards);
    return Object.keys(iterations).length ? { ...message, iterations } : message;
  }
  function syncSize(state = controller.snapshot()) {
    if (review.busy()) return;
    const board = state.artboards.find((b) => b.id === state.selected);
    const preset = navigation.takeSize(board?.view);
    if (!preset) return;
    const size = dimensions(preset, available(), board.size);
    if (size.width !== board.size.width || size.height !== board.size.height)
      act(() => controller.command(board.id, 'size', size));
  }
  // A filled axis takes the room the canvas has, so when that room changes the
  // size already applied is resolved again. Fixed axes keep their length and
  // Resizable keeps what it was dragged to; the frame resizes without reloading.
  function refill() {
    if (disposed || review.busy()) return;
    const board = selected();
    const preset = navigation.appliedSize(board?.view);
    if (!preset) return;
    const size = dimensions(preset, available(), board.size);
    if (size.width !== board.size.width || size.height !== board.size.height)
      act(() => controller.command(board.id, 'size', size));
  }
  function present() {
    const state = controller.snapshot();
    navigation.observe(state);
    const { visible, pending, loading, inert, displayed, problem } =
      navigation.select.feedback.read(navigation.state.get());
    const displayedState = displayed?.model;
    // Readiness needs real iframe geometry, including before the first paint.
    // Unpublished renderers get invisible slots without replacing the old view.
    canvas.style.display = state.artboards.length || displayedState?.artboards.length ? '' : 'none';
    if (visible || !pending) {
      if (visible) canvas.reading = !!navigation.snapshot().selection?.docs;
      if (canvas.state !== state) {
        canvas.state = state;
      }
      canvas.style.display = state.artboards.length ? '' : 'none';
    }
    canvas.staged = pending ? state.artboards : [];
    const retaining = pending && !loading && !!displayedState?.artboards.length;
    canvas.style.visibility = visible || retaining ? '' : 'hidden';
    canvas.inert = inert;
    $('canvas').setAttribute('aria-busy', String(pending));
    publisher.schedule();
    if (pending) {
      $('blank').textContent = 'Loading preview…';
      $('blank').classList.toggle('is-loading', loading);
      $('blank').hidden = !loading;
    } else if (problem) {
      $('blank').textContent = problem;
      $('blank').hidden = false;
      $('blank').classList.remove('is-loading');
    } else if (visible) {
      navigation.complete(
        controller.snapshot().artboards.some((board) => board.view?.problem) ? 'failed' : 'visible',
      );
      $('blank').hidden = true;
      $('blank').classList.remove('is-loading');
    } else $('blank').classList.remove('is-loading');
    if (visible || loading || !pending) sessions.finish();
    if (visible && fitPending) {
      fitPending = false;
      canvas.fitAll();
    }
  }
  function presented() {
    const state = controller.snapshot();
    return navigation.state.get().displayed?.model || { ...state, selected: null, artboards: [] };
  }
  function requestFit() {
    fitPending = true;
    if (!navigation.presentation().pending) present();
  }
  function fail(error) {
    if (disposed) return;
    fitPending = false;
    navigation.fail(error);
    present();
  }
  function command(name, value) {
    const board = selected();
    if (!board) return Promise.resolve();
    return controller.command(board.id, name, value);
  }
  function activityChanged() {
    activity = Date.now();
    publisher.schedule();
  }
  const controller = createController({
    mount(board, callbacks) {
      const runtime = createRuntime(board, {
        ...callbacks,
        stories: sessions.stories,
        navigated(from, id, destination) {
          const previous = navigation.state.get().requested;
          if (!navigation.adopt(from, id, destination)) return false;
          if (
            destination.src !== previous.src ||
            destination.scenario !== previous.scenario ||
            destination.lens !== previous.lens
          ) {
            // Navigation owns this classic route adapter until project/top-bar adoption.
            queueMicrotask(() => {
              if (!disposed && navigation.state.get().id === id)
                previous.select(
                  destination.scenario,
                  selected()?.size,
                  destination.actions,
                  destination.src,
                  destination.lens,
                );
            });
          } else act(() => sessions.identify(id, board.id));
          return true;
        },
        accepts: (id) => id === navigation.state.get().id,
      });
      runtime.element.slot = board.id;
      canvas.append(runtime.element);
      return runtime;
    },
    render(state) {
      syncSize(state);
      present();
      // Reflect the selected ready renderer into classic controls after identity adoption.
      queueMicrotask(focusSelection);
      if (navigation.presentation().pending) return;
      const board = state.artboards.find((b) => b.id === state.selected);
      if (!board) return;
      const count = state.artboards.reduce(
        (total, b) =>
          total +
          (Array.isArray(b.view?.payload.annotations) ? b.view.payload.annotations.length : 0),
        0,
      );
      toolbar.count(count);
      if (board.id !== previewBoard) {
        previewBoard = board.id;
        chrome.wbPreviewControls.reset();
        if (previews.has(board.id)) chrome.wbPreviewControls.receive(previews.get(board.id));
      }
      if (board.view?.tool) toolbar.tool(board.view.tool);
    },
    host(board, data) {
      const message = data;
      if (message.type === 'wb-say') {
        say(String(message.message || ''));
        return;
      }
      if (message.type === 'wb-runtime-preview') {
        const value = message.value;
        if (value?.event === 'ready') previews.set(board.id, value);
        if (controller.snapshot().selected === board.id) chrome.wbPreviewControls.receive(value);
        return;
      }
      if (message.type === 'wb-runtime-camera') {
        const value = message.value;
        if (value.key === 'pan-start' || value.key === 'pan-stop')
          canvas.panReady(value.key === 'pan-start');
        else if (value.key) controls.camera(value.key);
        else if (typeof value.wheel === 'number') {
          const rect = controller.runtime(board.id).element.getBoundingClientRect(),
            outer = canvas.getBoundingClientRect();
          canvas.wheelZoom(value.wheel, {
            x: rect.left - outer.left + (value.x || 0) * canvas.scale,
            y: rect.top - outer.top + (value.y || 0) * canvas.scale,
          });
        }
        return;
      }
      if (message.type === 'wb-here') {
        const { selection } = navigation.snapshot();
        // Only the selected renderer's requested destination may acknowledge
        // navigation to the editor. Pending reports do not settle selection.
        if (
          !message.pending &&
          selected()?.id === board.id &&
          message.src === selection?.src &&
          (message.scenario || null) === selection?.scenario &&
          (message.lens || null) === selection?.lens &&
          window.parent !== window
        )
          window.parent.postMessage(withShownIterations(message, selection), '*');
        return;
      }
      if (message.type === 'wb-frame-change') return;
      if (message.type === 'wb-context-menu') {
        const rect = controller.runtime(board.id).element.getBoundingClientRect();
        message.x = rect.left + Number(message.x || 0) * canvas.scale;
        message.y = rect.top + Number(message.y || 0) * canvas.scale;
      }
      if (window.parent !== window) window.parent.postMessage(message, '*');
    },
    error: say,
    rowWidth: 1600,
    activity: activityChanged,
    available,
  });
  const sessions = createPreviewSessions({
    displayed: () =>
      navigation.state.get().displayed?.model?.artboards.map((board) => board.id) || [],
    visible: () =>
      controller.snapshot().artboards.map((board) => ({
        id: board.id,
        target: board.target,
        initialized: !!board.view,
      })),
    create: controller.create,
    choose: controller.choose,
    close: controller.close,
    park: controller.park,
    resume: controller.resume,
    command: controller.command,
    conceal(id, hidden) {
      const element = controller.runtime(id).element;
      element.style.display = hidden ? 'none' : '';
      element.inert = hidden;
    },
    busy: () => review.busy(),
    stories: fetchStoryCatalog,
  });
  const publisher = createPublisher({
    read: () => ({
      state: presented(),
      spaces: registry,
      browsed: space?.id || '',
      activity,
    }),
    send: (request) => json('/_workbench/canvas/space', request),
    withdraw: (request) => {
      void fetch('/_workbench/canvas/space', {
        method: 'POST',
        keepalive: true,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(request),
      }).catch(() => {});
    },
    error: say,
  });
  function select(board) {
    controller.choose(board.id);
    activityChanged();
    navigation
      .snapshot()
      .selection?.select(
        board.target.scenario,
        board.size,
        board.view?.actions?.checked,
        board.target.src,
        board.target.lens,
      );
    act(() => command('preview', { command: 'inspect' }));
  }
  canvas.addEventListener(
    'wb-artboard-select',
    (event) => {
      const board = controller.snapshot().artboards.find((b) => b.id === event.detail.id);
      if (board) select(board);
    },
    { signal },
  );
  canvas.addEventListener(
    'wb-artboard-resize',
    (event) => {
      const { id, width, height } = event.detail;
      act(() => controller.command(id, 'size', { width, height }));
    },
    { signal },
  );
  canvas.addEventListener('wb-artboard-deselect', () => act(() => command('deselect')), { signal });
  // A click inside a preview also selects its owning artboard.
  let lastSelected = '';
  const identity = (board) =>
    JSON.stringify([
      board.id,
      board.target.src,
      board.target.scenario,
      board.target.lens,
      board.target.iteration || null,
    ]);
  const focusSelection = () => {
    const { navigating, expanding } = navigation.snapshot();
    const board = selected();
    if (
      !review.busy() &&
      !navigating &&
      board?.view?.ready &&
      !expanding &&
      lastSelected !== identity(board)
    ) {
      lastSelected = identity(board);
      navigation
        .snapshot()
        .selection?.select(
          board.target.scenario,
          board.size,
          board.view.actions?.checked,
          board.target.src,
          board.target.lens,
        );
    }
  };
  canvas.addEventListener('focusin', focusSelection, { signal });
  const controls = bindLegacyChrome({
    canvas,
    document,
    signal,
    selected,
    command,
    act,
    activityChanged,
    say,
    fit: requestFit,
    zoom: {
      get current() {
        return chrome.wbZoom;
      },
      set current(value) {
        chrome.wbZoom = value;
      },
    },
    previewControls: chrome.wbPreviewControls,
  });
  bindToolbar({
    document,
    signal,
    selected,
    command,
    act,
    say,
    copyReference: () => chrome.wbReference.copy(reference(presented())),
  });
  const delivery = createBrowserReview(document);
  const review = createReview({
    ...delivery,
    available: () => !canvas.inert,
    hold() {
      chrome.wbArtboardHeld = true;
      navigation.captureStarted();
      const locked = Array.from(
        document.querySelectorAll('.wb-topbar,.wb-sidebar,#toolbar,#viewControls'),
      ).map((node) => ({ node, inert: node.inert }));
      locked.forEach(({ node }) => {
        node.inert = true;
      });
      return () => {
        chrome.wbArtboardHeld = false;
        navigation.captureEnded();
        locked.forEach(({ node, inert }) => {
          node.inert = inert;
        });
        // A window resize can change the reading viewport while capture holds
        // its immutable dimensions. Reconcile it after the review releases.
        queueMicrotask(() => {
          if (!disposed && canvas.reading) canvas.state = controller.snapshot();
          refill();
        });
      };
    },
    capture: controller.capture,
    acquire: capture,
    notify: say,
  });
  const navigation = createNavigation({
    ready,
    busy: review.busy,
    present,
    async activate(next) {
      previews.clear();
      await sessions.activate({
        target: {
          space,
          src: next.src,
          scenario: next.scenario,
          lens: next.lens,
          example: next.example,
          label: next.label,
          trace: next.trace,
          navigation: next.navigation,
        },
        size: next.docs
          ? available()
          : dimensions(next.size, available(), { width: 1024, height: 768 }),
        pageLabel: next.pageLabel,
        artboards: !next.docs ? next.artboards : undefined,
        iterations: !next.docs ? next.iterations : undefined,
      });
    },
    // Another iteration re-renders only the artboards whose iteration changed.
    iterate: (next) => sessions.iterate(!next.docs ? next.iterations : undefined, next.navigation),
    fit: requestFit,
    async settle(next) {
      syncSize();
      lastSelected = selected() ? identity(selected()) : '';
      const board = selected();
      if (board?.view?.actions && board.view.actions.checked !== next.actions)
        await command('actions', next.actions);
      if (board?.view?.ready) await command('preview', { command: 'inspect' });
      activityChanged();
    },
  });
  navigation.state.watch(navigation.select.feedback, present, { signal });
  canvas.addEventListener(
    'wb-reading-resize',
    (event) => {
      const board = selected(),
        next = navigation.snapshot().selection;
      if (
        review.busy() ||
        !next?.docs ||
        board?.id !== event.detail.id ||
        board.view?.src !== next.src ||
        board.view?.lens !== next.lens
      )
        return;
      act(() => controller.command(board.id, 'size', event.detail));
    },
    { signal },
  );
  canvas.addEventListener(
    'wb-reading-scroll',
    (event) => {
      if (!review.busy() && navigation.snapshot().selection?.docs)
        act(() => command('scroll', event.detail));
    },
    { signal },
  );
  const room = new ResizeObserver(refill);
  room.observe($('canvas'));
  signal.addEventListener('abort', () => room.disconnect(), { once: true });
  $('shoot').addEventListener('click', () => act(() => review.run(false)), { signal });
  $('handoff').addEventListener('click', () => act(() => review.run(true)), { signal });
  void json('/_workbench/handoff')
    .then((value) => {
      if (!disposed) $('handoffControl').hidden = !value.available;
    })
    .catch(() => {});
  window.addEventListener('wb-say', (event) => say(event.detail.message), {
    signal,
  });
  window.addEventListener(
    'pagehide',
    () => {
      disposed = true;
      connection.abort();
      navigation.dispose();
      clearTimeout(toastTimer);
      sessions.dispose();
      delivery.dispose();
      publisher.dispose();
      controller.dispose();
      previews.clear();
    },
    { once: true },
  );
  return {
    fail,
    begin() {
      fitPending = false;
      navigation.begin();
    },
    show: navigation.show,
    resize(size) {
      navigation.resize(size);
      return command(
        'size',
        dimensions(size, available(), selected()?.size || { width: 1024, height: 768 }),
      );
    },
    selectedSize: () => selected()?.size,
    selectedArtboard: () => selected()?.target.artboard,
    selectedIteration: () => selected()?.target.iteration || undefined,
    stories: sessions.stories,

    refresh: sessions.refresh,
    clear() {
      fitPending = false;
      navigation.clear();
      sessions.clear();
    },
  };
}
