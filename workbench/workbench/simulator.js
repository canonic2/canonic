/* Interactive iOS Simulator surface
   ---------------------------------
   A ScreenCaptureKit helper sends hardware-encoded H.264 over a loopback
   WebSocket, outside VS Code's display-capture policy. WDA supplies input. */
(function () {
  var canvas = document.getElementById('simulatorCanvas');
  var context = canvas.getContext('2d', { alpha: false });
  var prompt = document.getElementById('simulatorPrompt');
  var share = document.getElementById('shareSimulator');
  var status = document.getElementById('simulatorStatus');
  var active = null;
  var streamUdid = null;
  var screen = null;
  var pointer = null;
  var nativeSocket = null;
  var nativeReader = null;
  var decoder = null;
  var connectingUdid = null;
  var reconnectTimer = null;
  var jpegBusy = false;
  var pendingJpeg = null;

  function message(text) {
    status.textContent = text;
  }

  function diagnostic(event, error) {
    try {
      fetch('/_workbench/log', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          level: 'error',
          event: event,
          details: { message: String(error && (error.message || error) || 'unknown error') },
        }),
      }).catch(function () {});
    } catch (_) {}
  }

  function post(path, body) {
    return fetch(path, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    }).then(function (response) {
      return response.json().then(function (answer) {
        if (!response.ok || !answer.ok) throw new Error(answer.error || 'Simulator request failed.');
        return answer;
      });
    });
  }

  function disconnected(selected) {
    if (streamUdid === selected.udid) {
      streamUdid = null;
      screen = null;
      canvas.classList.remove('is-interactive');
    }
    if (!active || active.udid !== selected.udid) return;
    prompt.hidden = false;
    share.disabled = false;
    message('Simulator stream disconnected. Reconnecting…');
    clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(connectNative, 1000);
  }

  function stopNative(selected) {
    var socket = nativeSocket;
    nativeSocket = null;
    if (socket) socket.close();
    var reader = nativeReader;
    nativeReader = null;
    if (reader) reader.cancel().catch(function () {});
    if (decoder && decoder.state !== 'closed') decoder.close();
    decoder = null;
    jpegBusy = false;
    pendingJpeg = null;
    if (streamUdid === selected.udid) streamUdid = null;
  }

  function startWda(selected) {
    message('Starting interaction…');
    return post('/_workbench/simulator', {
      implementation: selected.implementation,
      udid: selected.udid,
    }).then(function (answer) {
      if (!active || active.udid !== selected.udid) return;
      screen = answer.screen;
      canvas.classList.add('is-interactive');
    }, function (error) {
      message('Simulator interaction failed: ' + String(error.message || error));
      prompt.hidden = false;
      throw error;
    });
  }

  function connectNative() {
    if (!active || connectingUdid) return;
    var selected = active;
    connectingUdid = selected.udid;
    share.disabled = true;
    message('Connecting to “' + selected.label + '”…');
    stopNative(selected);
    startWda(selected).then(function () {
      if (streamUdid === selected.udid) message('Interactive');
    }).catch(function (error) {
      diagnostic('simulator.interaction.failed', error);
      console.error('[workbench] Simulator interaction failed', error);
    });
    post('/_workbench/simulator/stream', {
      implementation: selected.implementation,
      udid: selected.udid,
      /* VS Code does not promise the proprietary H.264 decoder exposed by a
         full browser. JPEG uses the webview's ordinary image decoder. */
      codec: window.parent === window && 'VideoDecoder' in window ? 'h264' : 'jpeg',
    }).then(function (answer) {
      if (!active || active.udid !== selected.udid) return;
      function painted(width, height) {
        streamUdid = selected.udid;
        prompt.hidden = true;
        message(screen ? 'Interactive' : 'Video live · starting interaction…');
      }
      if (answer.codec === 'h264') {
        decoder = new VideoDecoder({
          output: function (frame) {
            if (!active || active.udid !== selected.udid) { frame.close(); return; }
            if (canvas.width !== frame.displayWidth || canvas.height !== frame.displayHeight) {
              canvas.width = frame.displayWidth;
              canvas.height = frame.displayHeight;
            }
            context.drawImage(frame, 0, 0, canvas.width, canvas.height);
            painted(canvas.width, canvas.height);
            frame.close();
          },
          error: function (error) {
            message('Simulator decoder failed: ' + String(error.message || error));
            diagnostic('simulator.decoder.failed', error);
          },
        });
        decoder.configure({ codec: 'avc1.42E01F', optimizeForLatency: true, hardwareAcceleration: 'prefer-hardware' });
      }
      function decodeJpeg(bytes) {
        if (jpegBusy) { pendingJpeg = bytes.slice(); return; }
        jpegBusy = true;
        createImageBitmap(new Blob([bytes], { type: 'image/jpeg' })).then(function (bitmap) {
          if (!active || active.udid !== selected.udid) { bitmap.close(); return; }
          if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
            canvas.width = bitmap.width;
            canvas.height = bitmap.height;
          }
          context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
          painted(canvas.width, canvas.height);
          bitmap.close();
        }).catch(function (error) {
          message('Simulator image decode failed: ' + String(error.message || error));
          diagnostic('simulator.image-decode.failed', error);
        }).finally(function () {
          jpegBusy = false;
          if (pendingJpeg) {
            var next = pendingJpeg;
            pendingJpeg = null;
            decodeJpeg(next);
          }
        });
      }
      function readHttpStream() {
        return fetch(answer.stream, { cache: 'no-store' }).then(function (response) {
          if (!response.ok || !response.body) throw new Error('Simulator stream answered ' + response.status + '.');
          var reader = response.body.getReader();
          nativeReader = reader;
          var pending = new Uint8Array(0);
          message('Waiting for Simulator video…');
          function read() {
            return reader.read().then(function (result) {
              if (result.done) throw new Error('Simulator stream ended.');
              if (nativeReader !== reader) return;
              var joined = new Uint8Array(pending.length + result.value.length);
              joined.set(pending);
              joined.set(result.value, pending.length);
              pending = joined;
              while (pending.length >= 4) {
                var length = new DataView(pending.buffer, pending.byteOffset, pending.byteLength).getUint32(0, false);
                if (pending.length < 4 + length) break;
                var packet = pending.slice(4, 4 + length);
                pending = pending.slice(4 + length);
                if (packet.length >= 10) decodeJpeg(packet.subarray(9));
              }
              return read();
            });
          }
          return read();
        }).catch(function (error) {
          if (!active || active.udid !== selected.udid || nativeReader === null) return;
          nativeReader = null;
          diagnostic('simulator.http-stream.failed', error);
          disconnected(selected);
        });
      }
      if (answer.codec === 'jpeg') {
        readHttpStream();
        return;
      }
      var protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
      var socket = new WebSocket(protocol + '//' + location.host + answer.stream);
      nativeSocket = socket;
      socket.binaryType = 'arraybuffer';
      socket.onopen = function () { message('Waiting for Simulator video…'); };
      socket.onmessage = function (event) {
        var data = new Uint8Array(event.data);
        if (data.length < 10) return;
        if (answer.codec === 'jpeg') { decodeJpeg(data.subarray(9)); return; }
        if (!decoder || decoder.state === 'closed') return;
        var timestamp = Number(new DataView(data.buffer, data.byteOffset, data.byteLength).getBigUint64(1, false));
        try {
          decoder.decode(new EncodedVideoChunk({
            type: data[0] === 1 ? 'key' : 'delta', timestamp: timestamp, data: data.subarray(9),
          }));
        } catch (error) {
          message('Simulator decode failed: ' + String(error.message || error));
        }
      };
      socket.onclose = function () {
        if (nativeSocket !== socket) return;
        nativeSocket = null;
        if (decoder && decoder.state !== 'closed') decoder.close();
        decoder = null;
        disconnected(selected);
      };
      socket.onerror = function () {
        message('Simulator stream connection failed.');
        diagnostic('simulator.websocket.failed', 'WebSocket connection failed');
      };
    }).catch(function (error) {
      prompt.hidden = false;
      share.disabled = false;
      message('Simulator stream failed: ' + String(error.message || error));
      diagnostic('simulator.stream.failed', error);
      console.error('[workbench] native Simulator stream failed', error);
    }).finally(function () {
      if (connectingUdid === selected.udid) connectingUdid = null;
    });
  }

  function choose() {
    if (!active) return;
    connectNative();
  }

  function point(event) {
    if (!screen || !canvas.width || !canvas.height) return null;
    var rect = canvas.getBoundingClientRect();
    var cssScale = Math.min(rect.width / canvas.width, rect.height / canvas.height);
    var renderedWidth = canvas.width * cssScale;
    var renderedHeight = canvas.height * cssScale;
    var renderedLeft = rect.left + (rect.width - renderedWidth) / 2;
    var renderedTop = rect.top + (rect.height - renderedHeight) / 2;
    var px = (event.clientX - renderedLeft) / cssScale;
    var py = (event.clientY - renderedTop) / cssScale;
    /* Simulator's window chrome sits above its device display. Fit WDA's
       reported screen to the captured width and anchor it to the bottom. */
    var scale = Math.min(canvas.width / screen.width, canvas.height / screen.height);
    var contentWidth = screen.width * scale;
    var contentHeight = screen.height * scale;
    var left = (canvas.width - contentWidth) / 2;
    var top = canvas.height - contentHeight;
    var x = (px - left) / contentWidth;
    var y = (py - top) / contentHeight;
    if (x < 0 || x > 1 || y < 0 || y > 1) return null;
    return { x: x, y: y };
  }

  canvas.addEventListener('pointerdown', function (event) {
    var at = point(event);
    if (!at) return;
    event.preventDefault();
    canvas.setPointerCapture(event.pointerId);
    pointer = { id: event.pointerId, point: at, time: performance.now() };
  });

  canvas.addEventListener('pointerup', function (event) {
    if (!pointer || pointer.id !== event.pointerId || !active) return;
    event.preventDefault();
    var start = pointer;
    pointer = null;
    var end = point(event);
    if (!end) return;
    var distance = Math.hypot(end.x - start.point.x, end.y - start.point.y);
    var body = { implementation: active.implementation, udid: active.udid };
    if (distance < 0.015) {
      body.action = 'tap';
      body.x = end.x;
      body.y = end.y;
    } else {
      body.action = 'swipe';
      body.fromX = start.point.x;
      body.fromY = start.point.y;
      body.toX = end.x;
      body.toY = end.y;
      body.duration = Math.max(0.05, (performance.now() - start.time) / 1000);
    }
    var started = performance.now();
    post('/_workbench/simulator/input', body).then(function () {
      message('Interactive · ' + Math.round(performance.now() - started) + ' ms');
    }).catch(function (error) {
      message('Input failed: ' + String(error.message || error));
      diagnostic('simulator.input.failed', error);
      console.error('[workbench] Simulator input failed', error);
    });
  });

  canvas.addEventListener('pointercancel', function () { pointer = null; });
  share.addEventListener('click', choose);

  window.wbSimulator = {
    show: function (options) {
      active = options;
      canvas.hidden = false;
      var current = streamUdid === options.udid && !!screen;
      prompt.hidden = current;
      share.disabled = false;
      if (current) {
        message('Interactive');
      } else {
        message('Connecting to “' + options.label + '”…');
        connectNative();
      }
    },
    hide: function () {
      active = null;
      canvas.hidden = true;
      prompt.hidden = true;
    },
    reload: function () { if (active && streamUdid !== active.udid) connectNative(); },
  };
})();
