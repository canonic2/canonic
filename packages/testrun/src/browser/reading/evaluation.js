// Internal value bridge. CDP receives ordinary CallArguments/RemoteObjects;
// these tags belong solely to Testrun's bounded structured-value helper.
(() => {
  const arrays = { i8: Int8Array, ui8: Uint8Array, ui8c: Uint8ClampedArray,
    i16: Int16Array, ui16: Uint16Array, i32: Int32Array, ui32: Uint32Array,
    f32: Float32Array, f64: Float64Array, bi64: BigInt64Array, bui64: BigUint64Array };
  const fail = code => { throw new Error(`canonic:${code}`); };
  const detachedBuffers = new WeakSet();
  const bufferLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;
  const isBuffer = value => { try { bufferLength.call(value); return true; } catch { return false; } };
  const decode = (value, handles, refs = new Map()) => {
    if ('ref' in value) return refs.get(value.ref);
    if ('h' in value) return handles[value.h].value;
    if ('v' in value) return ({ undefined, null: null, NaN, Infinity, '-Infinity': -Infinity, '-0': -0 })[value.v];
    if ('n' in value) return value.n;
    if ('s' in value) return value.s;
    if ('b' in value) return value.b;
    if ('bi' in value) return BigInt(value.bi);
    if ('d' in value) return new Date(value.d);
    if ('u' in value) return new URL(value.u);
    if ('r' in value) return new RegExp(value.r.p, value.r.f);
    if ('e' in value) return Object.assign(new Error(value.e.m), {name:value.e.n,stack:value.e.s});
    if ('ta' in value) {
      const bytes = Uint8Array.from(atob(value.ta.b), c => c.charCodeAt(0));
      return new arrays[value.ta.k](bytes.buffer);
    }
    const result = 'a' in value ? [] : {};
    refs.set(value.id, result);
    if ('a' in value) for (const item of value.a) result.push(decode(item,handles,refs));
    else for (const item of value.o) Object.defineProperty(result,item.k,{value:decode(item.v,handles,refs),enumerable:true,writable:true,configurable:true});
    return result;
  };
  const encode = (input, limit = 1048576, prefixErrorStack = false) => {
    const maximum = Math.min(1048576,limit);
    if(!Number.isSafeInteger(maximum)||maximum<1)fail('VALUE_BOUNDS');
    const refs = new Map(); let nodes = 0, budget = 0, jsonProjection = false;
    const emptyObject = depth => {
      if (depth!==undefined) {
        if (++nodes>10000 || depth>100) fail('VALUE_BOUNDS');
        budget+=32; if (budget>maximum) fail('VALUE_BOUNDS');
      }
      const id=refs.size+1; refs.set({},id); return {id,o:[]};
    };
    const charge = value => { budget += new TextEncoder().encode(value).byteLength; if (budget > maximum) fail('VALUE_BOUNDS'); return value; };
    const visit = (value, depth) => {
      if (++nodes > 10000 || depth > 100) fail('VALUE_BOUNDS');
      budget += 32; if (budget > maximum) fail('VALUE_BOUNDS');
      if (value === undefined) return {v:'undefined'};
      if (value === null) return {v:'null'};
      if (typeof value === 'string') return {s:charge(value)};
      if (typeof value === 'boolean') return {b:value};
      if (typeof value === 'number') {
        if (Number.isNaN(value)) return {v:'NaN'};
        if (Object.is(value,-0)) return {v:'-0'};
        if (!Number.isFinite(value)) return {v:value>0?'Infinity':'-Infinity'};
        if (Number.isInteger(value) && !Number.isSafeInteger(value)) fail('UNSAFE_INTEGER');
        return {n:value};
      }
      if (typeof value === 'bigint') return {bi:charge(String(value))};
      if (typeof value === 'symbol') return {v:'undefined'};
      if (typeof value === 'function') return value.name.startsWith('__pw_fn_') ? {fn:charge(value.name)} : {v:'undefined'};
      if (typeof value !== 'object') fail('VALUE_CONVERSION_REQUIRED');
      if (value instanceof Date) return {d:value.toISOString()};
      if (value instanceof URL) return {u:charge(value.href)};
      if (value instanceof RegExp) return {r:{p:charge(value.source),f:value.flags}};
      if (value instanceof Error) {
        const header = `${value.name}: ${value.message}`;
        const stack = prefixErrorStack && !value.stack?.startsWith(header) ? `${header}\n${value.stack}` : value.stack;
        return {e:{n:charge(value.name),m:charge(value.message),s:charge(stack||'')}};
      }
      if (ArrayBuffer.isView(value) && !(value instanceof DataView)) {
        if (value.byteLength > maximum/2) fail('VALUE_BOUNDS');
        const kind = Object.keys(arrays).find(key => value instanceof arrays[key]);
        if (!kind) fail('VALUE_CONVERSION_REQUIRED');
        const bytes = new Uint8Array(value.buffer,value.byteOffset,value.byteLength);
        let binary = ''; for (const byte of bytes) binary += String.fromCharCode(byte);
        return {ta:{k:kind,b:charge(btoa(binary))}};
      }
      if (refs.has(value)) return {ref:refs.get(value)};
      if (!Array.isArray(value) && Object.getPrototypeOf(value)!==Object.prototype && Object.getPrototypeOf(value)!==null) {
        if (isBuffer(value)) {
          try { new Uint8Array(value); } catch (error) { detachedBuffers.add(error); throw error; }
          // The pinned public protocol projects each injected ArrayBuffer copy
          // as a fresh ordinary object. It carries neither bytes nor properties.
          return emptyObject();
        }
        // Preserve explicit refusal for special values whose native conversion
        // is not supplied by the ordinary own-property projection.
        if (typeof Node==='function' && value instanceof Node || typeof Window==='function' && value instanceof Window) fail('VALUE_CONVERSION_REQUIRED');
        let tag; try { tag=Object.prototype.toString.call(value); } catch {}
        if (['[object Date]','[object RegExp]','[object URL]','[object Error]','[object Window]'].includes(tag)) fail('VALUE_CONVERSION_REQUIRED');
      }
      const id = refs.size+1; refs.set(value,id);
      if (Array.isArray(value)) {
        if (value.length > 10000) fail('VALUE_BOUNDS');
        return {id,a:Array.from(value,item=>visit(item,depth+1))};
      }
      const entries=[], keys=Object.keys(value);
      if (keys.length > 10000) fail('VALUE_BOUNDS');
      for (const key of keys) {
        let item;
        try { item=value[key]; } catch { continue; }
        entries.push({k:charge(key),v:key==='toJSON' && typeof item==='function' ? emptyObject(depth+1) : visit(item,depth+1)});
      }
      let wrapper;
      try { if (entries.length===0 && value.toJSON && typeof value.toJSON==='function') wrapper={value:value.toJSON()}; } catch {}
      if (wrapper) { jsonProjection=true; return visit(wrapper.value,depth+1); }
      return {id,o:entries};
    };
    let result = visit(input,0);
    if (jsonProjection) {
      // The injected serializer can retire an object node in favor of toJSON.
      // Its server decoder turns references to that unpublished node into
      // undefined before the public client protocol serializes the value.
      const defined = new Set();
      const children = value => value.a || (value.o ? value.o.map(entry=>entry.v) : []);
      const collect = value => { if (value.a || value.o) defined.add(value.id); for (const child of children(value)) collect(child); };
      const project = value => {
        if ('ref' in value && !defined.has(value.ref)) return {v:'undefined'};
        if (value.a) value.a=value.a.map(project);
        if (value.o) for (const entry of value.o) entry.v=project(entry.v);
        return value;
      };
      collect(result); result=project(result);
    }
    if (new TextEncoder().encode(JSON.stringify(result)).byteLength > maximum) fail('VALUE_BOUNDS');
    return result;
  };
  // Pinned normalizeEvaluationExpression semantics. Compile only to recognize
  // method source; caller code is evaluated once by the execution owner.
  const normalize = (expression, isFunction) => {
    expression = expression.trim();
    if (isFunction) {
      try { new Function('(' + expression + ')'); }
      catch {
        expression = expression.startsWith('async ')
          ? 'async function ' + expression.substring('async '.length)
          : 'function ' + expression;
        try { new Function('(' + expression + ')'); }
        catch { fail('INVALID_EVALUATION_FUNCTION'); }
      }
    }
    if (/^(async)?\s*function(\s|\()/.test(expression)) expression = '(' + expression + ')';
    return expression;
  };
  const encodeResult = input => { try { return encode(input); } catch (error) {
    if (detachedBuffers.has(error)) return {v:'undefined'};
    throw error;
  } };
  return {decode,encode,encodeResult,encodeBinding:input=>encode(input,undefined,true),normalize};
})()
