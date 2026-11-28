// Testrun-owned entry for the pinned Playwright server (Apache-2.0).
// SDK files remain unchanged. Framing matches its little-endian driver pipe.
(() => {
  function correctClockBootstrap(source) {
    if (!source.includes('globalThis.__pwClock = (module.exports.inject())')) return source;
    // Bundle admission pins this SDK source. Correct only its injected clock,
    // in memory, for both future-document registration and current-frame use.
    const patches = [
      ['    } else {\n      this._realTime = void 0;\n    }\n    this._log.length = 0;',
        '    } else {\n      this._realTime = void 0;\n      this._currentRealTimeTimer?.cancel();\n      this._currentRealTimeTimer = void 0;\n    }\n    this._log.length = 0;'],
      ['result.__defineGetter__("timeOrigin", () => clock._now.origin || 0);',
        'result.__defineGetter__("timeOrigin", () => { clock.performanceNow(); return clock._now.origin || 0; });'],
    ];
    for (const [before, after] of patches) {
      if (source.split(before).length !== 2) throw Error('CLOCK_BOOTSTRAP_UNSUPPORTED');
      source = source.replace(before, after);
    }
    return source;
  }
  function installClockCorrections(playwright, BrowserContext) {
    const watched = new WeakSet();
    playwright.instrumentation.addListener({
      onBeforeCall(_progress, object) {
        const context = object.attribution?.context;
        if (!(context instanceof BrowserContext) || watched.has(context)) return;
        const clock = context.clock;
        if (typeof context.addInitScript !== 'function' || typeof clock?._evaluateInFrames !== 'function')
          throw Error('CLOCK_BOOTSTRAP_UNSUPPORTED');
        const add = context.addInitScript.bind(context);
        const evaluate = clock._evaluateInFrames.bind(clock);
        context.addInitScript = (progress, source) => add(progress, correctClockBootstrap(source));
        clock._evaluateInFrames = source => evaluate(correctClockBootstrap(source));
        watched.add(context);
      }
    });
  }
  function installChildObservations(playwright, write, timers = {setInterval,clearInterval}) {
    const watched = new WeakMap();
    let count = 0, limited = false, unavailable = false;
    const emit = value => {
      try { write('testrun:owned-child '+JSON.stringify(value)+'\n'); }
      catch { /* Observation delivery cannot alter the SDK finalizer. */ }
    };
    const flag = value => typeof value === 'boolean' ? value : null;
    const own = (object,name) => {
      const descriptor = Object.getOwnPropertyDescriptor(object,name);
      return descriptor && 'value' in descriptor ? descriptor.value : undefined;
    };
    const counter = value => Number.isInteger(value) && value >= 0 && value <= 6 ? value : null;
    const nativeState = child => {
      const code = own(child,'exitCode'), signal = own(child,'signalCode'), handle = own(child,'_handle');
      return {exitStatusKnown:Number.isInteger(code) || typeof signal === 'string' ? true : code === null && signal === null ? false : null,
        nativeHandlePresent:handle === null ? false : typeof handle === 'object' || typeof handle === 'function' ? true : null,
        closesNeeded:counter(own(child,'_closesNeeded')),closesGot:counter(own(child,'_closesGot'))};
    };
    const streams = child => Array.from({length:Math.min(child.stdio.length,5)},(_,index) => {
      const stream = child.stdio[index];
      return {index,present:!!stream,closed:flag(stream?.closed),destroyed:flag(stream?.destroyed),readableEnded:flag(stream?.readableEnded),writableFinished:flag(stream?.writableFinished)};
    });
    playwright.instrumentation.addListener({
      onBrowserOpen(browser) {
        const child = browser.options?.browserProcess?.process;
        if (!child || typeof child.once !== 'function' || !Array.isArray(child.stdio)) {
          if (!unavailable) { unavailable = true; emit({phase:'unavailable'}); }
          return;
        }
        if (watched.has(child)) return;
        if (count === 4) {
          if (!limited) { limited = true; emit({phase:'capacity'}); }
          return;
        }
        const owner = ++count;
        watched.set(child,owner);
        const snapshot = () => ({streams:streams(child),nativeState:nativeState(child)});
        const initial = snapshot();let fingerprint = JSON.stringify(initial);
        emit({phase:'observing',owner,...initial});
        const changed = () => {
          const value = snapshot(), next = JSON.stringify(value);
          if (next === fingerprint) return;
          fingerprint = next;
          emit({phase:'native-state',owner,...value});
        };
        const timer = timers.setInterval(changed,1000);timer.unref();
        child.once('exit',() => emit({phase:'native-exit',owner,...snapshot()}));
        child.once('close',() => {timers.clearInterval(timer);emit({phase:'native-close',owner,...snapshot()});});
        child.stdio.slice(0,5).forEach((stream,index) => {
          if (!stream || typeof stream.once !== 'function') return;
          for (const event of ['end','finish','close']) {
            stream.once(event,() => emit({phase:'stream-'+event,owner,index}));
          }
        });
      }
    });
  }
  function installContextCleanup(playwright, BrowserContext) {
    const watched = new WeakSet();
    playwright.instrumentation.addListener({
      onBeforeCall(_progress, object) {
        const context = object.attribution?.context;
        if (!(context instanceof BrowserContext) || watched.has(context)) return;
        watched.add(context);
        const debugger_ = context.debugger();
        const closed = () => {
          if (context._closedStatus !== 'closed') return;
          context.off(BrowserContext.Events.Close, closed);
          // The whole context is closed, including pages and workers. Resuming
          // this retained SDK pause cannot dispatch into a live scope.
          if (debugger_.isPaused()) debugger_.resume();
        };
        context.on(BrowserContext.Events.Close, closed);
      }
    });
  }
  function createTransport(input, output, maximum, receive, finish) {
    let closed = false, header = Buffer.alloc(4), headerBytes = 0, body = null, bodyBytes = 0;
    const stop = code => {
      if (closed) return;
      closed = true;
      body = null;
      finish(code);
    };
    input.on('data', chunk => {
      if (closed) return;
      let offset = 0;
      while (offset < chunk.length && !closed) {
        if (!body) {
          const count = Math.min(4 - headerBytes, chunk.length - offset);
          chunk.copy(header, headerBytes, offset, offset + count);
          headerBytes += count; offset += count;
          if (headerBytes < 4) continue;
          const length = header.readUInt32LE();
          headerBytes = 0;
          if (!length || length > maximum) { stop(1); return; }
          body = Buffer.allocUnsafe(length);
        }
        const count = Math.min(body.length - bodyBytes, chunk.length - offset);
        chunk.copy(body, bodyBytes, offset, offset + count);
        bodyBytes += count; offset += count;
        if (bodyBytes === body.length) {
          const text = body.toString('utf8');
          body = null; bodyBytes = 0;
          setImmediate(() => {
            if (closed) return;
            try { Promise.resolve(receive(JSON.parse(text))).catch(() => stop(1)); }
            catch { stop(1); }
          });
        }
      }
    });
    input.on('close', () => stop(headerBytes || body ? 1 : 0));
    input.on('error', () => stop(1));
    output.on('error', () => stop(1));
    return {
      send(message) {
        if (closed) return;
        try {
          const data = Buffer.from(JSON.stringify(message), 'utf8');
          if (!data.length || data.length > maximum) { stop(1); return; }
          const head = Buffer.alloc(4);
          head.writeUInt32LE(data.length);
          output.write(head); output.write(data);
        } catch { stop(1); }
      }
    };
  }
  function run(core, process, maximum) {
    const {server, utils} = core;
    const connection = new server.DispatcherConnection();
    new server.RootDispatcher(connection, async (scope, {sdkLanguage}) => {
      const playwright = server.createPlaywright({sdkLanguage,isClientCollocatedWithServer:true});
      installContextCleanup(playwright, server.BrowserContext);
      installClockCorrections(playwright, server.BrowserContext);
      installChildObservations(playwright, value => {
        if (!process.stderr.destroyed) process.stderr.write(value,() => {});
      });
      return new server.PlaywrightDispatcher(scope, playwright);
    });
    const transport = createTransport(process.stdin, process.stdout, maximum,
      message => connection.dispatch(message),
      code => {
        connection.onmessage = () => {};
        utils.gracefullyProcessExitDoNotHang(code);
      });
    connection.onmessage = message => transport.send(message);
    process.on('SIGINT', () => {});
  }
  return {correctClockBootstrap, installClockCorrections, installChildObservations, installContextCleanup, createTransport, run};
})()
