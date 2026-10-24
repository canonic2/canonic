/** Authenticated canvas/content protocol. Owns no application presentation. */

import { validDimensions } from '../../../../contracts/src/sizes/browser/geometry.js';

export function initializeRenderer(ports) {
  const params = new URLSearchParams(location.search);
  const channel = params.get('artboard-runtime'),
    origin = params.get('canvas-origin');
  if (channel && origin && window.parent !== window) start(ports, channel, origin);
}
function start(ports, channel, origin) {
  const params = new URLSearchParams(location.search);
  let capturedAnnotations = [],
    capturing = false,
    held = false;
  let contentSize;
  let observedContent = null;
  const contentObserver = new ResizeObserver(() => read());
  let previous = '';
  const emit = (event, value, id, error) =>
    window.parent.postMessage({ type: 'wb-artboard', channel, event, value, id, error }, origin);
  function read() {
    const shell = ports.content.read();
    const problem = ports.content.problem();
    if (!shell) {
      if (!problem) return null;
      const src = location.hash.replace(/^#/, '').split(/[:!@~]/)[0] || 'unavailable';
      const info = {
        src,
        scenario: null,
        lens: null,
        label: src,
        hash: location.hash,
        ready: false,
        problem,
        reference: '',
        payload: {},
        scenarios: [],
        lenses: [],
      };
      const serial = JSON.stringify(info);
      if (serial !== previous) {
        previous = serial;
        emit('view', info);
      }
      return info;
    }
    const payload = ports.annotations.payload();
    const artboard = params.get('artboard');
    if (artboard) {
      payload.artboard = artboard;
      payload.label = shell.label;
    }
    if (!held && !capturing && shell.ready) {
      contentSize = undefined;
      try {
        const doc = document.querySelector('#artboardContent iframe.is-active')?.contentDocument;
        const content = doc?.querySelector('[data-wb-fit-content]');
        if ((content || null) !== observedContent) {
          contentObserver.disconnect();
          observedContent = content || null;
          if (content) contentObserver.observe(content);
        }
        if (content) {
          const box = content.getBoundingClientRect();
          const measured = {
            width: Math.max(1, Math.ceil(box.width)),
            height: Math.max(1, Math.ceil(box.height)),
          };
          if (Math.abs(box.x) < 1 && Math.abs(box.y) < 1 && validDimensions(measured))
            contentSize = measured;
        }
      } catch {
        /* External frames retain their explicitly selected dimensions. */
      }
    }
    const reference = ports.reference(ports.view()) || '';
    const info = {
      ...shell,
      contentSize,
      payload,
      tool: ports.annotations.toolName(),
      reference: reference + (artboard ? '\n- Artboard: ' + artboard : ''),
      problem,
    };
    const status = document.getElementById('simulatorStatus');
    if (status && /failed|denied|requires|not found/i.test(status.textContent || ''))
      info.problem = status.textContent;
    const serial = JSON.stringify(info);
    if (serial !== previous) {
      previous = serial;
      emit('view', info);
      if (info.ready)
        window.parent.postMessage(
          {
            type: 'wb-here',
            src: info.src,
            scenario: info.scenario,
            lens: info.lens,
            pending: false,
          },
          origin,
        );
    }
    return info;
  }
  async function command(event) {
    const data = event.data;
    if (
      event.source !== window.parent ||
      event.origin !== origin ||
      data?.type !== 'wb-artboard-command' ||
      data.channel !== channel ||
      typeof data.id !== 'string'
    )
      return;
    try {
      const before = previous;
      if (
        (held || capturing) &&
        !['inspect', 'capture', 'unlock', 'clear-captured'].includes(data.command)
      )
        throw new Error('Artboard capture is in progress.');
      let result = null;
      switch (data.command) {
        case 'lock':
          held = true;
          ports.held = true;
          document.getElementById('artboardContent').inert = true;
          break;
        case 'unlock':
          held = false;
          ports.held = false;
          document.getElementById('artboardContent').inert = false;
          break;
        case 'navigate':
          ports.content.navigate(data.value);
          break;
        case 'size': {
          if (!validDimensions(data.value)) throw new Error('Invalid artboard size.');
          ports.content.size(data.value.width, data.value.height);
          break;
        }
        case 'scroll':
          ports.content.scroll(data.value);
          break;
        case 'scenario':
          ports.content.pickScenario(String(data.value));
          break;
        case 'lens':
          ports.content.lens(String(data.value || ''));
          break;
        case 'reload':
          ports.content.reload();
          break;
        case 'refresh':
          ports.content.refresh();
          break;
        case 'actions':
          ports.content.action('actions', data.value);
          break;
        case 'preview': {
          const frame = document.querySelector('#artboardContent iframe.is-active');
          frame?.contentWindow?.postMessage(
            { type: 'workbench-preview-command', ...data.value },
            location.origin,
          );
          break;
        }
        case 'tool':
          if (
            !['pointer', 'draw', 'arrow', 'line', 'rect', 'ellipse', 'text', 'comment'].includes(
              data.value,
            )
          )
            throw new Error('Unknown annotation tool.');
          ports.annotations.tool(data.value);
          break;
        case 'undo':
          ports.annotations.undo();
          break;
        case 'deselect':
          ports.annotations.deselect();
          break;
        case 'clear':
          ports.annotations.clear();
          break;
        case 'clear-captured':
          ports.annotations.clearCaptured(capturedAnnotations);
          capturedAnnotations = [];
          break;
        case 'capture': {
          capturing = true;
          try {
            const captured = await ports.annotations.capture();
            capturedAnnotations = captured.annotations;
            const artboard = params.get('artboard');
            if (artboard) {
              captured.payload.artboard = artboard;
              captured.payload.label = ports.content.read()?.label || captured.payload.label;
            }
            if (captured.payload.docs) {
              const response = await fetch('/_workbench/canvas/review', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(captured.payload),
              });
              const result = await response.json();
              if (!response.ok) throw new Error(result.error || 'Could not resolve docs review.');
              captured.payload = result.payload;
            }
            result = { blob: captured.blob, payload: captured.payload, at: captured.at };
          } finally {
            capturing = false;
          }
          break;
        }
        case 'inspect':
          result = read();
          break;
        default:
          throw new Error('Unknown artboard command.');
      }
      const info = read();
      if (data.command === 'navigate' && info?.ready && before === previous)
        window.parent.postMessage(
          {
            type: 'wb-here',
            src: info.src,
            scenario: info.scenario,
            lens: info.lens,
            pending: false,
          },
          origin,
        );
      emit('response', result, data.id);
    } catch (error) {
      emit('response', null, data.id, error instanceof Error ? error.message : String(error));
    }
  }
  // command reports failures through the response protocol.
  const receiveCommand = (event) => {
    void command(event);
  };
  window.addEventListener('message', receiveCommand);
  window.addEventListener('message', (event) => {
    const frame = document.querySelector('#artboardContent iframe.is-active');
    if (event.source !== frame?.contentWindow) return;
    if (event.origin === location.origin && event.data?.type === 'workbench-preview') {
      emit('preview', event.data);
      return;
    }
    if (!ports.content.read()?.ready) return;
    const keyboard = ports.keyboard?.(event);
    if (keyboard) {
      const key = cameraKey(keyboard.event);
      if (key) emit('camera', { key });
      else window.parent.postMessage(keyboard, origin);
      return;
    }
    if (event.origin !== new URL(frame.src).origin) return;
    if (['wb-keyboard', 'wb-context-reset'].includes(event.data?.type))
      window.parent.postMessage(event.data, origin);
    if (event.data?.type === 'wb-context-menu') {
      const box = frame.getBoundingClientRect();
      window.parent.postMessage(
        {
          ...event.data,
          x: box.x + Number(event.data.x || 0),
          y: box.y + Number(event.data.y || 0),
        },
        origin,
      );
    }
  });
  window.addEventListener('wb-frame-change', read);
  window.addEventListener('wb-view-change', read);
  window.addEventListener('wb-content-say', (event) => {
    const detail = event.detail;
    window.parent.postMessage({ type: 'wb-say', message: detail.message }, origin);
  });
  window.addEventListener('hashchange', read);
  window.addEventListener('focus', () => emit('focus'));
  document.addEventListener('pointerdown', () => emit('focus'), true);
  // Same-origin preview documents receive clicks without bubbling to the shell.
  const watched = new WeakSet();
  function watchDocument(doc) {
    if (watched.has(doc)) return;
    watched.add(doc);
    doc.addEventListener('pointerdown', () => emit('focus'), true);
    doc.addEventListener(
      'wheel',
      (event) => {
        if (!event.ctrlKey && !event.metaKey) return;
        event.preventDefault();
        emit('camera', { wheel: event.deltaY, x: event.clientX, y: event.clientY });
      },
      { passive: false },
    );
    doc.addEventListener(
      'keydown',
      (event) => {
        const node = event.composedPath()[0];
        if (node?.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(node?.tagName)) return;
        const key = cameraKey(event);
        if (key) {
          event.preventDefault();
          event.stopImmediatePropagation();
          emit('camera', { key });
        }
      },
      true,
    );
    doc.addEventListener(
      'keyup',
      (event) => {
        if (event.code === 'Space') emit('camera', { key: 'pan-stop' });
      },
      true,
    );
  }
  function watch() {
    for (const frame of document.querySelectorAll('#artboardContent iframe')) {
      try {
        const doc = frame.contentDocument;
        if (!doc) continue;
        watchDocument(doc);
      } catch {
        /* Cross-origin focus is detected by the shell blur below. */
      }
    }
  }
  window.addEventListener('blur', () => {
    if (document.hasFocus() && document.activeElement?.tagName === 'IFRAME') emit('focus');
  });
  const timer = setInterval(() => {
    read();
    watch();
  }, 500);
  watchDocument(document);
  window.addEventListener(
    'pagehide',
    () => {
      clearInterval(timer);
      contentObserver.disconnect();
      window.removeEventListener('message', receiveCommand);
    },
    { once: true },
  );
  read();
}

function cameraKey(event) {
  if ((event.metaKey || event.ctrlKey) && !event.altKey)
    return event.key === '0'
      ? '100'
      : ['=', '+'].includes(event.key)
        ? 'in'
        : event.key === '-'
          ? 'out'
          : null;
  return event.shiftKey && event.key === '1'
    ? 'fit-all'
    : event.shiftKey && event.key === '2'
      ? 'fit-selected'
      : event.code === 'Space'
        ? 'pan-start'
        : null;
}
