// Injected by CDP into every document; never shipped. Cross-document timestamps
// use timeOrigin + performance.now(), all from Chromium's monotonic clock.
(() => {
  window.__labId = crypto.randomUUID();
  window.__labLongTasks = [];
  window.__labDomWork = [];
  const html = Object.getOwnPropertyDescriptor(Element.prototype, 'innerHTML');
  Object.defineProperty(Element.prototype, 'innerHTML', {
    ...html,
    set(value) {
      if (this.tagName !== 'TEMPLATE') return html.set.call(this, value);
      const start = performance.timeOrigin + performance.now();
      try {
        return html.set.call(this, value);
      } finally {
        __labDomWork.push({
          phase: 'template-parse',
          start,
          duration: performance.timeOrigin + performance.now() - start,
        });
      }
    },
  });
  const replace = Element.prototype.replaceChildren;
  Element.prototype.replaceChildren = function (...nodes) {
    if (this.tagName !== 'CODE' || !this.isConnected) return replace.apply(this, nodes);
    const start = performance.timeOrigin + performance.now();
    try {
      return replace.apply(this, nodes);
    } finally {
      __labDomWork.push({
        phase: 'code-insert',
        start,
        duration: performance.timeOrigin + performance.now() - start,
      });
    }
  };
  try {
    new PerformanceObserver((list) =>
      __labLongTasks.push(
        ...list.getEntries().map((e) => ({ start: e.startTime, duration: e.duration })),
      ),
    ).observe({ type: 'longtask', buffered: true });
  } catch {}
  window.__labFind = (src) => {
    const walk = (doc, chain = []) => {
      const w = doc.defaultView;
      if (w?.__workbenchDocs?.page === src) return { doc, w, chain };
      for (const frame of doc.querySelectorAll('iframe')) {
        try {
          const found = frame.contentDocument && walk(frame.contentDocument, [...chain, frame]);
          if (found) return found;
        } catch {}
      }
    };
    return walk(document);
  };
  window.__labSelect = async (src, action = 'select') => {
    const started = performance.timeOrigin + performance.now();
    location.hash = src + '~html';
    const deadline = performance.now() + 20000;
    let found, loaderFirst, loaderLast;
    while (performance.now() < deadline) {
      const blank = document.getElementById('blank');
      if (blank && !blank.hidden && /Loading/.test(blank.textContent || '')) {
        loaderFirst ??= performance.now();
        loaderLast = performance.now();
      }
      found = __labFind(src);
      if (found?.w.__workbenchError) throw Error(found.w.__workbenchError);
      if (found?.w.__workbenchReady && !found.w.__labColourPromise) {
        const w = found.w;
        w.__labColourPromise = w.wbDocsPage
          .whenHighlighted()
          .then(() => (w.__labColourAt = performance.timeOrigin + performance.now()));
      }
      if (
        found?.w.__workbenchReady &&
        found.chain.every(
          (frame) =>
            frame.ownerDocument.defaultView.getComputedStyle(frame).visibility === 'visible',
        ) &&
        found.doc.querySelector('h1')
      )
        break;
      await new Promise((r) => setTimeout(r, 5));
    }
    if (
      !found?.w.__workbenchReady ||
      !found.chain.every(
        (frame) => frame.ownerDocument.defaultView.getComputedStyle(frame).visibility === 'visible',
      )
    )
      throw Error(
        'Visibility timeout for ' +
          src +
          ' observed=' +
          JSON.stringify({
            hash: location.hash,
            found: !!found,
            ready: found?.w.__workbenchReady,
            error: found?.w.__workbenchError,
            visibility: document.visibilityState,
            chain: found?.chain.map((frame) => ({
              src: frame.src,
              visibility: frame.ownerDocument.defaultView.getComputedStyle(frame).visibility,
            })),
          }),
      );
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const visible = performance.timeOrigin + performance.now();
    const { doc, w } = found;
    const assets = {
      fonts: doc.fonts.status,
      images: [...doc.images]
        .filter((img) => img.getBoundingClientRect().top < innerHeight)
        .every((img) => img.complete && img.naturalWidth > 0),
      examples: [...doc.querySelectorAll('[data-wb-example]')].map((node) => ({
        id: node.dataset.wbExample,
        status: node.dataset.status,
        text: node.querySelector('[data-wb-example-stage]')?.textContent,
      })),
    };
    await w.__labColourPromise;
    const coloured = w.__labColourAt;
    // Let PerformanceObserver receive the just-finished DOM task. Timestamps
    // above are already stopped; this turn is only for collecting diagnostics.
    await new Promise((r) => setTimeout(r, 0));
    const blocks = [...doc.querySelectorAll('code[data-wb-code]')].map((node) => ({
      text: node.textContent,
      language: node.dataset.wbCode,
      spans: node.children.length,
    }));
    const navigation = w.performance.getEntriesByType('navigation')[0];
    return {
      action,
      src,
      startedAbs: started,
      visibleMs: visible - started,
      colouredMs: Math.max(0, coloured - started),
      visibleAndColouredMs: Math.max(visible, coloured) - started,
      documentId: w.__labId,
      assets,
      blocks,
      loaderExposed: loaderFirst !== undefined,
      loaderMs: loaderFirst === undefined ? 0 : loaderLast - loaderFirst,
      workers: w.__labHighlightMetrics || [],
      domWork: (w.__labDomWork || []).filter((e) => e.start >= started),
      longTasks: (w.__labLongTasks || []).filter(
        (e) => w.performance.timeOrigin + e.start + e.duration >= started,
      ),
      document: navigation && {
        requestMs: navigation.responseEnd - navigation.requestStart,
        transferSize: navigation.transferSize,
        decodedBodySize: navigation.decodedBodySize,
        serverTiming: navigation.serverTiming?.map((t) => ({ name: t.name, duration: t.duration })),
      },
      resources: w.performance
        .getEntriesByType('resource')
        .filter((e) => w.performance.timeOrigin + e.startTime >= started)
        .map((e) => ({
          name: new URL(e.name).pathname,
          start: w.performance.timeOrigin + e.startTime,
          transferSize: e.transferSize,
          decodedBodySize: e.decodedBodySize,
          duration: e.duration,
          serverTiming: e.serverTiming?.map((t) => ({ name: t.name, duration: t.duration })),
        })),
      heap: performance.memory?.usedJSHeapSize,
    };
  };
  window.__labSource = async (src, id) => {
    const { doc, w } = __labFind(src);
    const figure = doc.querySelector('[data-wb-example="' + id + '"]');
    const button = figure?.querySelector('[data-wb-show-code]');
    if (!button) throw Error('Missing Show code ' + id);
    const panel = figure.querySelector('[data-wb-example-code]');
    if (!panel.hidden) button.click();
    const started = performance.timeOrigin + performance.now();
    button.click();
    const end = performance.now() + 10000;
    while (!panel.querySelector('code') && performance.now() < end)
      await new Promise((r) => setTimeout(r, 2));
    const code = panel.querySelector('code');
    if (!code)
      throw Error(
        'Source timeout: ' +
          JSON.stringify({
            hidden: panel.hidden,
            loaded: panel.dataset.loaded,
            text: panel.textContent.slice(0, 250),
          }),
      );
    const completion = w.wbDocsPage
      .whenHighlighted()
      .then(() => performance.timeOrigin + performance.now());
    await new Promise((r) => requestAnimationFrame(r));
    const readable = performance.timeOrigin + performance.now();
    const coloured = await completion;
    await new Promise((r) => setTimeout(r, 0));
    return {
      startedAbs: started,
      readableMs: readable - started,
      colouredMs: coloured - started,
      text: code.textContent,
      spans: code.children.length,
      workers: w.__labHighlightMetrics || [],
      resources: w.performance
        .getEntriesByType('resource')
        .filter((e) => w.performance.timeOrigin + e.startTime >= started)
        .map((e) => ({
          name: new URL(e.name).pathname,
          start: w.performance.timeOrigin + e.startTime,
          transferSize: e.transferSize,
          decodedBodySize: e.decodedBodySize,
          duration: e.duration,
        })),
      domWork: (w.__labDomWork || []).filter((e) => e.start >= started),
      longTasks: (w.__labLongTasks || []).filter(
        (e) => w.performance.timeOrigin + e.start + e.duration >= started,
      ),
    };
  };
  window.__labCorrectness = (src, inspectGlyphs = false) => {
    const { doc, w, chain } = __labFind(src);
    const nodes = [...doc.querySelectorAll('code')];
    let glyphGeometry;
    if (inspectGlyphs) {
      const rects = (node) => {
        const walker = doc.createTreeWalker(node, 4), range = doc.createRange(), result = [];
        let text;
        while ((text = walker.nextNode())) for (let at = 0; at < text.length; at++) {
          range.setStart(text, at); range.setEnd(text, at + 1);
          const r = range.getBoundingClientRect();
          result.push([r.x, r.y, r.width, r.height]);
        }
        return result;
      };
      glyphGeometry = { characters: 0, differing: 0, maxDelta: 0, examples: [] };
      for (const [index, node] of nodes.entries()) {
        const html = node.innerHTML, text = node.textContent, before = rects(node);
        let after;
        try { node.textContent = text; after = rects(node); }
        finally { node.innerHTML = html; }
        glyphGeometry.characters += before.length;
        for (let at = 0; at < before.length; at++) {
          const delta = Math.max(...before[at].map((value, key) => Math.abs(value - after[at][key])));
          glyphGeometry.maxDelta = Math.max(glyphGeometry.maxDelta, delta);
          if (delta > 0) {
            glyphGeometry.differing++;
            if (glyphGeometry.examples.length < 10) glyphGeometry.examples.push({ node: index, offset: at, delta, before: before[at], after: after[at] });
          }
        }
      }
    }
    const geometry = nodes.every((node) => {
      const html = node.innerHTML;
      const text = node.textContent;
      const before = node.getBoundingClientRect().toJSON();
      node.textContent = text;
      const plain = node.getBoundingClientRect().toJSON();
      node.innerHTML = html;
      return JSON.stringify(before) === JSON.stringify(plain) && node.textContent === text;
    });
    const typography = nodes.every((node) => {
      const base = w.getComputedStyle(node);
      return [...node.querySelectorAll('*')].every((span) => {
        const style = w.getComputedStyle(span);
        return (
          span.tagName === 'SPAN' &&
          !span.hasAttribute('style') &&
          [
            'fontFamily',
            'fontSize',
            'fontStyle',
            'fontWeight',
            'lineHeight',
            'letterSpacing',
          ].every((name) => style[name] === base[name]) &&
          style.display === 'inline' &&
          style.position === 'static'
        );
      });
    });
    const codeBoxes = nodes
      .filter((node) => node.closest('pre'))
      .map((node) => {
        const rect = node.getBoundingClientRect();
        let x = rect.x,
          y = rect.y,
          width = rect.width,
          height = rect.height;
        for (const frame of chain.slice().reverse()) {
          const box = frame.getBoundingClientRect(),
            sx = box.width / frame.offsetWidth,
            sy = box.height / frame.offsetHeight;
          x = box.x + x * sx;
          y = box.y + y * sy;
          width *= sx;
          height *= sy;
        }
        let ancestor = node,
          background = 'rgb(255, 255, 255)';
        while (ancestor) {
          const color = w.getComputedStyle(ancestor).backgroundColor;
          if (color !== 'rgba(0, 0, 0, 0)' && color !== 'transparent') {
            background = color;
            break;
          }
          ancestor = ancestor.parentElement;
        }
        const palette = [
          ...new Set(
            [node, ...node.querySelectorAll('*')].map(
              (element) => w.getComputedStyle(element).color,
            ),
          ),
        ];
        return { x, y, width, height, background, palette };
      });
    return {
      geometry,
      glyphGeometry,
      typography,
      codeBoxes,
      images: [...doc.images].every((img) => img.complete && img.naturalWidth > 0),
      fonts: doc.fonts.status,
      examples: [...doc.querySelectorAll('[data-wb-example]')].every(
        (node) => node.dataset.status === 'ready',
      ),
      unexpectedMarkup: nodes.some((node) => node.querySelector('script,img,iframe,style,button')),
      workerErrors: (w.__labHighlightMetrics || []).filter((m) => m.error),
    };
  };
  window.__labRoles = (src, expected) => {
    const { doc } = __labFind(src);
    const palette = {
      'hljs-comment': 'rgb(89, 99, 110)',
      'hljs-keyword': 'rgb(207, 34, 46)',
      'hljs-number': 'rgb(5, 80, 174)',
      'hljs-section': 'rgb(102, 57, 186)',
      'hljs-name': 'rgb(17, 99, 41)',
      'hljs-deletion': 'rgb(130, 7, 30)',
      'hljs-addition': 'rgb(10, 48, 105)',
      'hljs-variable': 'rgb(5, 80, 174)',
      'hljs-attr': 'rgb(5, 80, 174)',
    };
    return [...doc.querySelectorAll('code[data-wb-code]')].map((code, index) => {
      const text = code.textContent;
      const segments = [];
      const walker = doc.createTreeWalker(code, NodeFilter.SHOW_TEXT);
      let offset = 0,
        node;
      while ((node = walker.nextNode())) {
        segments.push({ start: offset, end: offset + node.textContent.length, node });
        offset += node.textContent.length;
      }
      const roles = expected[index].roles.map(([token, role]) => {
        let at = -1,
          pass = false;
        while ((at = text.indexOf(token, at + 1)) !== -1) {
          const covered = segments.filter((s) => s.end > at && s.start < at + token.length);
          if (
            covered.length &&
            covered.every(
              (s) =>
                s.node.parentElement.closest('.' + role) &&
                doc.defaultView.getComputedStyle(s.node.parentElement).color === palette[role],
            )
          ) {
            pass = true;
            break;
          }
        }
        return { text: token, role, expectedColour: palette[role], pass };
      });
      return {
        language: expected[index].language,
        roles,
        plain: !code.children.length,
        html: code.innerHTML,
      };
    });
  };
  window.__labCapture = async (src) => {
    const { chain } = __labFind(src);
    const frame = chain[0];
    const channel = new URL(frame.src).searchParams.get('artboard-runtime');
    const id = crypto.randomUUID();
    const value = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        removeEventListener('message', receive);
        reject(Error('Capture protocol timeout'));
      }, 20000);
      function receive(event) {
        const data = event.data;
        if (
          event.source !== frame.contentWindow ||
          event.origin !== location.origin ||
          data?.channel !== channel ||
          data.id !== id ||
          data.event !== 'response'
        )
          return;
        clearTimeout(timer);
        removeEventListener('message', receive);
        data.error ? reject(Error(data.error)) : resolve(data.value);
      }
      addEventListener('message', receive);
      frame.contentWindow.postMessage(
        { type: 'wb-artboard-command', channel, id, command: 'capture' },
        location.origin,
      );
    });
    const bytes = new Uint8Array(await value.blob.arrayBuffer());
    return {
      bytes: bytes.length,
      type: value.blob.type,
      signature: [...bytes.slice(0, 8)],
      src: value.payload.src,
      docs: !!value.payload.docs,
    };
  };
})();
