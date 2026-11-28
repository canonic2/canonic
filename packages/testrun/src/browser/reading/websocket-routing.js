// Realm-local routing owner. The caller installs the pinned mock with emit as
// its binding, then supplies its dispatcher. Replies use data-only commands.
((installMock, guard, compileRegex) => {
  // The realm's URL parser and RegExp at installation, for URLPattern
  // components and patterns the host DFA cannot run.
  const NativeURL = URL, NativeRegExp = RegExp, regExpTest = RegExp.prototype.test;
  // Decoded bytes, and JSON-encoded text within the shared transport limits.
  const maximumBytes = 524288, maximumBase64 = 699052, maximumEncoded = 786432;
  const policies = new Map(), sockets = new Map();
  const handlerKeys = {onMessageFromPage:'pageMessage',onMessageFromServer:'serverMessage',onClosePage:'pageClose',onCloseServer:'serverClose'};
  const listenerId = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
  let pending = 0;
  const close = id => {
    const socket = sockets.get(id);
    if (!socket) return;
    sockets.delete(id);
    dispatch({id, type:'closePage', code:3001, reason:'Route released', wasClean:false});
    dispatch({id, type:'closeServer', code:3001, reason:'Route released', wasClean:false});
  };
  const message = value => {
    if (!value || typeof value.data !== 'string' || typeof value.isBase64 !== 'boolean')
      throw Error('WEBSOCKET_MESSAGE_INVALID');
    if (value.isBase64) {
      if (value.data.length > maximumBase64 || atob(value.data).length > maximumBytes)
        throw Error('WEBSOCKET_MESSAGE_BOUNDS');
    } else if (value.data.length > maximumBytes || new TextEncoder().encode(value.data).length > maximumBytes
        || JSON.stringify(value.data).length > maximumEncoded)
      throw Error('WEBSOCKET_MESSAGE_BOUNDS');
    return {data:value.data, isBase64:value.isBase64};
  };
  const commands = (id, reply) => {
    const sdk = sockets.get(id)?.owner.sdkDefaults === true;
    if (sdk && reply == null) reply = [];
    if (!Array.isArray(reply) || reply.length > 8) throw Error('WEBSOCKET_REPLY_INVALID');
    // Validate the whole reply before dispatching its first mutation.
    const validated = reply.map(command => {
      if (!command || !['connect','ensureOpened','sendToPage','sendToServer','closePage','closeServer','setForwarding','setCloseForwarding'].includes(command.type))
        throw Error('WEBSOCKET_COMMAND_INVALID');
      if (sdk && (command.type === 'setForwarding' || command.type === 'setCloseForwarding'))
        throw Error('WEBSOCKET_SDK_HANDLER_COMMAND_CONFLICT');
      const result = {id, type:command.type};
      if (command.type === 'setForwarding' || command.type === 'setCloseForwarding') {
        if (!['page','server'].includes(command.from) || typeof command.enabled !== 'boolean')
          throw Error('WEBSOCKET_FORWARDING_INVALID');
        Object.assign(result, {from:command.from, enabled:command.enabled});
      }
      if (command.type.startsWith('send')) result.data = message(command.data);
      if (command.type.startsWith('close')) {
        const code = command.code, reason = command.reason ?? '';
        const validCode = code === undefined || code === 1000 || code >= 3000 && code <= 4999
          || command.type === 'closePage' && code >= 1001 && code <= 1014 && ![1004,1005,1006].includes(code);
        if (!validCode || code !== undefined && !Number.isInteger(code)
            || typeof reason !== 'string' || reason.length > 123 || new TextEncoder().encode(reason).length > 123)
          throw Error('WEBSOCKET_CLOSE_INVALID');
        Object.assign(result, {code, reason, wasClean:true});
      }
      return result;
    });
    // Connection ownership outlives the native server object, including its
    // final close callback. Validate state before an earlier command can write.
    const current = sockets.get(id);
    if (!current) return;
    const state = {...current};
    for (const command of validated) {
      if (command.type === 'connect') {
        if (state.connected) throw Error('WEBSOCKET_ALREADY_CONNECTED');
        if (state.closedPage || state.closedServer || state.serverClosing)
          throw Error('WEBSOCKET_SOCKET_CLOSED');
        state.connected = true;
      }
      if (command.type === 'sendToServer') {
        if (!state.connected) throw Error('WEBSOCKET_SERVER_NOT_CONNECTED');
        if (state.closedServer || state.serverClosing) throw Error('WEBSOCKET_SOCKET_CLOSED');
      }
      if (command.type === 'sendToPage' && state.closedPage)
        throw Error('WEBSOCKET_SOCKET_CLOSED');
      if (command.type === 'closeServer') state.serverClosing = true;
      if (command.type === 'closePage') {
        state.closedPage = true;
        // Terminal mock closure deliberately ignores subsequent commands.
        if (!state.connected) break;
      }
    }
    for (const command of validated) {
      if (!sockets.has(id)) break;
      const socket = sockets.get(id);
      if (command.type === 'setForwarding') socket.forward[command.from] = command.enabled;
      else if (command.type === 'setCloseForwarding') socket.forwardClose[command.from] = command.enabled;
      else {
        if (command.type === 'connect') socket.connected = true;
        if (command.type === 'closeServer') {
          if (socket.closedServer || socket.serverClosing) continue;
          socket.serverClosing = true;
        }
        if (command.type === 'closePage') {
          socket.closedPage = true;
          socket.pageCloseSent = true;
          // A mock with no server attachment has no outstanding close witness.
          if (!socket.connected) socket.closedServer = true;
        }
        dispatch(command);
      }
      if (socket.closedServer && socket.pageCloseSent) sockets.delete(id);
    }
  };
  const emit = event => {
    const id = event.id;
    if (event.type === 'onCreate') {
      let owner;
      for (const policy of policies.values()) {
        if (policy.matches(event.url) && (!owner || policy.page && !owner.page
            || policy.page === owner.page && policy.order >= owner.order)) owner = policy;
      }
      if (!owner) return dispatch({id, type:'passthrough'});
      sockets.set(id, {owner, pending:0, connected:false, closedPage:false, closedServer:false, serverClosing:false, pageCloseSent:false,
        handlers:{pageMessage:null,serverMessage:null,pageClose:null,serverClose:null},
        forward:{page:owner.sdkDefaults,server:owner.sdkDefaults},forwardClose:{page:owner.sdkDefaults,server:owner.sdkDefaults}});
    }
    const socket = sockets.get(id);
    if (!socket) return;
    // Snapshot arrival ownership before asynchronous delivery or replacement.
    const handler = socket.owner.sdkDefaults && socket.handlers[handlerKeys[event.type]];
    if (handler) event = {...event, listenerId:handler};
    try {
      if (event.type === 'onMessageFromPage' || event.type === 'onMessageFromServer') {
        const data = message(event.data), from = event.type === 'onMessageFromPage' ? 'page' : 'server';
        if (socket.forward[from]) {
          if (socket.owner.sdkDefaults && from === 'page' && !socket.connected) return;
          if (from === 'page' ? socket.closedServer || socket.serverClosing : socket.closedPage) {
            // The SDK wrapper catches failed default sends independently; it
            // does not turn one closed endpoint into closure of its survivor.
            if (socket.owner.sdkDefaults) return;
            throw Error('WEBSOCKET_SOCKET_CLOSED');
          }
          dispatch({id,type:from === 'page' ? 'sendToServer' : 'sendToPage',data});
          return;
        }
      }
      if (event.type === 'onClosePage' || event.type === 'onCloseServer') {
        const from = event.type === 'onClosePage' ? 'page' : 'server';
        const field = from === 'page' ? 'closedPage' : 'closedServer';
        // Command mode suppresses closePage acknowledgement reentry. SDK mode
        // delivers that acknowledgement to its current handler or default relay.
        if (socket[field] && !(socket.owner.sdkDefaults && from === 'page' && socket.pageCloseSent)) return;
        socket[field] = true;
        if (socket.closedServer && socket.pageCloseSent && !handler) { sockets.delete(id); return; }
        if (socket.forwardClose[from]) {
          if (from === 'server') {
            socket.closedPage = true;
            socket.pageCloseSent = true;
            dispatch({id,type:'closePage',code:event.code,reason:event.reason,wasClean:event.wasClean});
          } else if (!socket.closedServer && !socket.serverClosing) {
            socket.serverClosing = true;
            dispatch({id,type:'closeServer',code:event.code,reason:event.reason,wasClean:event.wasClean});
          }
          if (socket.closedServer && socket.pageCloseSent) sockets.delete(id);
          return;
        }
      }
    } catch { close(id); return; }
    if (pending >= 64 || socket.pending >= 32) return close(id);
    pending++;
    socket.pending++;
    Promise.resolve().then(() => socket.owner.callback(event)).then(reply => {
      if (sockets.get(id) === socket) {
        commands(id, reply);
        if (event.type === 'onCreate' && socket.owner.sdkDefaults && !socket.connected && !socket.closedPage)
          dispatch({id,type:'ensureOpened'});
      }
    }).catch(() => close(id)).finally(() => { pending--; socket.pending--; });
    if (socket.closedServer && socket.pageCloseSent) sockets.delete(id);
  };
  const {Mock, dispatch} = installMock(emit);
  const WebSocket = guard(Mock, url => {
    // Consult every policy before allocation, including bounded regexes after
    // an older broad match. Selection itself still uses Page/newest precedence.
    let selected = false;
    for (const policy of policies.values()) if (policy.matches(url)) selected = true;
    return selected;
  });
  return {
    WebSocket,
    add(key, url, callback, page, order = 0, sdkDefaults = false) {
      if (policies.has(key)) return;
      if (policies.size >= 64 || typeof callback !== 'function'
          || !Number.isSafeInteger(order) || order < 0 || typeof sdkDefaults !== 'boolean') throw Error('WEBSOCKET_POLICY_BOUNDS');
      let matches;
      if (url.automaton) {
        matches = compileRegex(url.automaton);
      } else if (url.nativeRegex) {
        // Like the SDK's fresh RegExp per test: lastIndex never carries over.
        const pattern = new NativeRegExp(url.nativeRegex.source, url.nativeRegex.flags);
        matches = value => {
          if (!value.startsWith('ws://') && !value.startsWith('wss://')) return false;
          if (value.length > 8192) throw new RangeError('WEBSOCKET_URL_BOUNDS');
          pattern.lastIndex = 0;
          return regExpTest.call(pattern, value);
        };
      } else if (Array.isArray(url.urlPattern) && url.urlPattern.length === 8) {
        // Pinned URLPattern: every component matches its own program; null is `*`.
        const parts = url.urlPattern.map(program => program && compileRegex(program, true));
        matches = value => {
          if (!value.startsWith('ws://') && !value.startsWith('wss://')) return false;
          const parsed = new NativeURL(value);
          const components = [parsed.protocol.slice(0, -1), parsed.username, parsed.password,
            parsed.hostname, parsed.port, parsed.pathname, parsed.search.slice(1), parsed.hash.slice(1)];
          return parts.every((part, index) => !part || part(components[index]));
        };
      } else {
        const source = typeof url === 'string'
          ? '^' + new URL(url).href.replace(/^ws/, 'http').replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '$'
          : url.regexSource;
        const matcher = new RegExp(source);
        matches = url => matcher.test(url.replace(/^ws/, 'http'));
      }
      policies.set(key, {key, matches, callback, page:!!page, order, sdkDefaults});
    },
    remove(key) {
      policies.delete(key);
      for (const [id, socket] of sockets) if (socket.owner.key === key) close(id);
    },
    handlers(key,id,handlers) {
      const socket=sockets.get(id);
      if(!policies.has(key)||!socket||socket.owner.key!==key)return 'WEBSOCKET_SOCKET_EXPIRED';
      if(!socket.owner.sdkDefaults)return 'WEBSOCKET_SDK_MODE_REQUIRED';
      if(!handlers||typeof handlers!=='object'||Array.isArray(handlers))throw Error('WEBSOCKET_HANDLERS_INVALID');
      const entries=Object.entries(handlers),keys=['pageMessage','serverMessage','pageClose','serverClose'];
      if(!entries.length||entries.length>4||entries.some(([key,value])=>!keys.includes(key)||value!==null&&(typeof value!=='string'||!listenerId.test(value))))
        throw Error('WEBSOCKET_HANDLERS_INVALID');
      // Validate every field before replacing the first handler.
      for(const [key,value] of entries){
        socket.handlers[key]=value;
        const from=key.startsWith('page')?'page':'server';
        (key.endsWith('Close')?socket.forwardClose:socket.forward)[from]=value===null;
      }
      return 'configured';
    },
    closeSocket(key,id,to,options={}) {
      const socket=sockets.get(id);
      if(!policies.has(key)||!socket||socket.owner.key!==key)return 'WEBSOCKET_SOCKET_EXPIRED';
      if(to!=='page'&&to!=='server')throw Error('WEBSOCKET_COMMAND_INVALID');
      if(!options||typeof options!=='object'||Array.isArray(options)||Object.keys(options).some(key=>key!=='code'&&key!=='reason'))
        throw Error('WEBSOCKET_CLOSE_INVALID');
      commands(id,[{...options,type:to==='page'?'closePage':'closeServer'}]);
      return 'close-requested';
    },
    send(key,id,to,data) {
      const socket=sockets.get(id);
      if(!policies.has(key)||!socket||socket.owner.key!==key)return 'WEBSOCKET_SOCKET_EXPIRED';
      if(to!=='page'&&to!=='server')throw Error('WEBSOCKET_COMMAND_INVALID');
      if(to==='page'?socket.closedPage:socket.closedServer||socket.serverClosing)return 'WEBSOCKET_SOCKET_CLOSED';
      if(to==='server'&&!socket.connected)return 'WEBSOCKET_SERVER_NOT_CONNECTED';
      commands(id,[{type:to==='page'?'sendToPage':'sendToServer',data}]);
      return 'sent';
    },
    get active() { return sockets.size; },
    get pending() { return pending; }
  };
})
