// Bound intercepted sockets before the pinned helper converts or buffers data.
// Unmatched sockets retain the backend's ordinary passthrough behavior.
((Mock, matches) => {
  // At most 32 queued messages and 2 MiB of queued server-bound data.
  const maximum = 524288, maximumQueued = 32, maximumQueuedBytes = 4 * 524288, maximumSockets = 256;
  const states = new WeakMap();
  const blobSize = Object.getOwnPropertyDescriptor(Blob.prototype, 'size').get;
  const blobBuffer = Blob.prototype.arrayBuffer;
  const bufferSize = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get;
  let created = 0;
  const size = value => {
    if (typeof value === 'string') {
      if (value.length > maximum) throw new RangeError('WEBSOCKET_MESSAGE_BOUNDS');
      const bytes = new TextEncoder().encode(value).byteLength;
      // Escaped text must also fit the callback and command transport.
      if (bytes > maximum || JSON.stringify(value).length > 786432)
        throw new RangeError('WEBSOCKET_MESSAGE_BOUNDS');
      return bytes;
    }
    const bytes = value instanceof Blob ? blobSize.call(value)
      : value instanceof ArrayBuffer ? bufferSize.call(value)
      : ArrayBuffer.isView(value) ? value.byteLength : NaN;
    if (!Number.isSafeInteger(bytes) || bytes < 0 || bytes > maximum)
      throw new RangeError('WEBSOCKET_MESSAGE_BOUNDS');
    return bytes;
  };
  return class WebSocket extends Mock {
    constructor(url, protocols) {
      const resolved = new URL(url, window.document.baseURI);
      const selected = matches(resolved.href.replace(/^http/, 'ws'));
      if (selected && resolved.href.includes('#'))
        throw new DOMException('WebSocket URLs cannot contain fragments', 'SyntaxError');
      if (selected && created >= maximumSockets) throw new RangeError('WEBSOCKET_CAPACITY');
      let offered = protocols;
      if (selected) {
        const names = Array.isArray(protocols) ? protocols : protocols === undefined ? [] : [protocols];
        const length = names.length;
        if (!Number.isInteger(length) || length < 0 || length > 16)
          throw new RangeError('WEBSOCKET_PROTOCOL_BOUNDS');
        offered = [];
        for (let index = 0; index < length; index++) {
          const name = names[index];
          if (typeof name !== 'string' || name.length > 128)
            throw new RangeError('WEBSOCKET_PROTOCOL_BOUNDS');
          if (!/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(name) || offered.includes(name))
            throw new DOMException('Invalid or duplicate WebSocket subprotocol', 'SyntaxError');
          offered.push(name);
        }
      }
      super(resolved.href.replace(/^http/, 'ws'), offered);
      if (selected) {
        created++;
        states.set(this, {queued: 0, bytes: 0, conversions: 0});
      }
    }
    send(message) {
      const state = states.get(this);
      if (!state || this._passthrough) return super.send(message);
      size(message);
      if (!(message instanceof Blob)) return super.send(message);
      // The upstream send does not return its asynchronous Blob conversion.
      // Convert once here so its allocation remains bounded and accounted for.
      if (state.conversions >= maximumQueued) throw new RangeError('WEBSOCKET_QUEUE_BOUNDS');
      if (this.readyState !== Mock.OPEN) return super.send(message);
      state.conversions++;
      blobBuffer.call(message).then(buffer => {
        if (this.readyState === Mock.OPEN) super.send(buffer);
      }).catch(() => {
        this._apiClosePage(3009, 'WebSocket conversion failed', false);
        this._apiCloseServer(3009, 'WebSocket conversion failed', false);
      }).finally(() => state.conversions--);
    }
    _apiSendToPage(message) {
      if (states.has(this) && !this._passthrough) size(message);
      return super._apiSendToPage(message);
    }
    _apiSendToServer(message) {
      const state = states.get(this);
      if (state && !this._passthrough) {
        const bytes = size(message);
        if (this._ws && (this._ws.bufferedAmount || 0) + bytes > maximumQueuedBytes)
          throw new RangeError('WEBSOCKET_QUEUE_BOUNDS');
        if (this._ws?.readyState === Mock.CONNECTING) {
          if (state.queued >= maximumQueued || state.bytes + bytes > maximumQueuedBytes)
            throw new RangeError('WEBSOCKET_QUEUE_BOUNDS');
          state.queued++;
          state.bytes += bytes;
        }
      }
      return super._apiSendToServer(message);
    }
    _apiCloseServer(code, reason, wasClean) {
      const socket = this._ws;
      if (states.has(this) && !this._passthrough && code === undefined && socket) {
        if (socket.readyState === Mock.CONNECTING || socket.readyState === Mock.OPEN) {
          // With an explicit reason, undefined can become invalid native code 0.
          // An empty close omits arguments; a reason needs a status on the wire.
          if (reason) socket.close(1000, reason);
          else socket.close();
        }
        return;
      }
      return super._apiCloseServer(code, reason, wasClean);
    }
    _apiConnect() {
      super._apiConnect();
      const state = states.get(this);
      if (!state || this._passthrough) return;
      const socket = this._ws, opened = socket.onopen, received = socket.onmessage;
      socket.onopen = event => {
        state.queued = 0;
        state.bytes = 0;
        opened(event);
      };
      socket.onmessage = event => {
        try {
          size(event.data);
          if (state.conversions >= maximumQueued) throw new RangeError('WEBSOCKET_QUEUE_BOUNDS');
        }
        catch {
          this._apiClosePage(3009, 'WebSocket message exceeds limit', false);
          this._apiCloseServer(3009, 'WebSocket message exceeds limit', false);
          return;
        }
        if (!(event.data instanceof Blob)) return received(event);
        state.conversions++;
        blobBuffer.call(event.data).then(buffer => {
          if (this._ws === socket && socket.readyState === Mock.OPEN) received({data: buffer});
        }).catch(() => {
          this._apiClosePage(3009, 'WebSocket conversion failed', false);
          this._apiCloseServer(3009, 'WebSocket conversion failed', false);
        }).finally(() => state.conversions--);
      };
    }
  };
})
