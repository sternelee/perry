// Perry WASM Runtime Bridge
// Provides JavaScript runtime functions imported by the WASM module.
// Handles NaN-boxing, string management, handle store, and browser API access.

// NaN-boxing constants (matching perry-runtime/src/value.rs)
const TAG_UNDEFINED = 0x7FFC_0000_0000_0001n;
const TAG_NULL      = 0x7FFC_0000_0000_0002n;
const TAG_FALSE     = 0x7FFC_0000_0000_0003n;
const TAG_TRUE      = 0x7FFC_0000_0000_0004n;
const STRING_TAG    = 0x7FFFn;
const POINTER_TAG   = 0x7FFDn;
const INT32_TAG     = 0x7FFEn;

// f64 <-> u64 conversion via shared buffer
const _convBuf = new ArrayBuffer(8);
const _f64 = new Float64Array(_convBuf);
const _u64 = new BigUint64Array(_convBuf);

function f64ToU64(f) { _f64[0] = f; return _u64[0]; }
function u64ToF64(u) { _u64[0] = u; return _f64[0]; }

// NaN-box helpers
function nanboxString(id) {
  return u64ToF64((STRING_TAG << 48n) | BigInt(id));
}
function nanboxPointer(id) {
  return u64ToF64((POINTER_TAG << 48n) | BigInt(id));
}
function isString(val) {
  return (f64ToU64(val) >> 48n) === STRING_TAG;
}
function isPointer(val) {
  return (f64ToU64(val) >> 48n) === POINTER_TAG;
}
function getStringId(val) {
  return Number(f64ToU64(val) & 0xFFFFFFFFn);
}
function getPointerId(val) {
  return Number(f64ToU64(val) & 0xFFFFFFFFn);
}
function isUndefined(val) { return f64ToU64(val) === TAG_UNDEFINED; }
function isNull(val) { return f64ToU64(val) === TAG_NULL; }
function isTrue(val) { return f64ToU64(val) === TAG_TRUE; }
function isFalse(val) { return f64ToU64(val) === TAG_FALSE; }

// String table — maps string_id (index) to JS string
const stringTable = [];

// Handle store — maps handle_id to JS objects/arrays/closures/etc.
const handleStore = new Map();
let nextHandleId = 1;

function allocHandle(obj) {
  const id = nextHandleId++;
  handleStore.set(id, obj);
  return id;
}
function getHandle(val) {
  if (!isPointer(val)) return undefined;
  return handleStore.get(getPointerId(val));
}
function getHandleId(val) {
  return getPointerId(val);
}

// Convert a NaN-boxed f64 value to a JS value
function toJsValue(val) {
  const bits = f64ToU64(val);
  if (bits === TAG_UNDEFINED) return undefined;
  if (bits === TAG_NULL) return null;
  if (bits === TAG_TRUE) return true;
  if (bits === TAG_FALSE) return false;
  const tag = bits >> 48n;
  if (tag === STRING_TAG) return stringTable[Number(bits & 0xFFFFFFFFn)];
  if (tag === POINTER_TAG) {
    const obj = handleStore.get(Number(bits & 0xFFFFFFFFn));
    if (obj !== undefined) return obj;
  }
  return val; // plain number
}

// Convert a JS value to a NaN-boxed f64
function fromJsValue(v) {
  if (v === undefined) return u64ToF64(TAG_UNDEFINED);
  if (v === null) return u64ToF64(TAG_NULL);
  if (v === true) return u64ToF64(TAG_TRUE);
  if (v === false) return u64ToF64(TAG_FALSE);
  if (typeof v === 'number') return v;
  if (typeof v === 'string') {
    const id = stringTable.length;
    stringTable.push(v);
    return nanboxString(id);
  }
  // Object/Array/Function — store as handle
  const id = allocHandle(v);
  return nanboxPointer(id);
}

// Get string from NaN-boxed value (supports both string tag and pointer to string)
function getString(val) {
  if (isString(val)) return stringTable[getStringId(val)];
  const js = toJsValue(val);
  return String(js);
}

let wasmMemory = null;
let wasmInstance = null;

// Build the import object for WASM instantiation
function buildImports() {
  return {
    rt: {
      // ===== Core (Phase 0) =====

      string_new: (offset, len) => {
        const bytes = new Uint8Array(wasmMemory.buffer, offset, len);
        stringTable.push(new TextDecoder().decode(bytes));
      },
      console_log: (val) => { console.log(toJsValue(val)); },
      console_warn: (val) => { console.warn(toJsValue(val)); },
      console_error: (val) => { console.error(toJsValue(val)); },

      string_concat: (a, b) => {
        const s = stringTable[getStringId(a)] + stringTable[getStringId(b)];
        stringTable.push(s);
        return nanboxString(stringTable.length - 1);
      },
      js_add: (a, b) => fromJsValue(toJsValue(a) + toJsValue(b)),
      string_eq: (a, b) => stringTable[getStringId(a)] === stringTable[getStringId(b)] ? 1 : 0,
      string_len: (val) => {
        if (isString(val)) return stringTable[getStringId(val)].length;
        // Array length
        const obj = getHandle(val);
        if (Array.isArray(obj)) return obj.length;
        return 0;
      },
      jsvalue_to_string: (val) => {
        stringTable.push(String(toJsValue(val)));
        return nanboxString(stringTable.length - 1);
      },
      is_truthy: (val) => {
        const bits = f64ToU64(val);
        if (bits === TAG_FALSE || bits === TAG_NULL || bits === TAG_UNDEFINED) return 0;
        if (bits === TAG_TRUE) return 1;
        const tag = bits >> 48n;
        if (tag === STRING_TAG) return stringTable[Number(bits & 0xFFFFFFFFn)].length > 0 ? 1 : 0;
        if (tag === POINTER_TAG) return 1; // objects are truthy
        return (val === 0 || Number.isNaN(val)) ? 0 : 1;
      },
      js_strict_eq: (a, b) => toJsValue(a) === toJsValue(b) ? 1 : 0,

      // Math
      math_floor: (x) => Math.floor(x),
      math_ceil: (x) => Math.ceil(x),
      math_round: (x) => Math.round(x),
      math_abs: (x) => Math.abs(x),
      math_sqrt: (x) => Math.sqrt(x),
      math_pow: (base, exp) => Math.pow(base, exp),
      math_random: () => Math.random(),
      math_log: (x) => Math.log(x),
      date_now: () => Date.now(),
      js_typeof: (val) => {
        const t = typeof toJsValue(val);
        stringTable.push(t);
        return nanboxString(stringTable.length - 1);
      },
      math_min: (a, b) => Math.min(a, b),
      math_max: (a, b) => Math.max(a, b),
      parse_int: (val) => {
        const s = isString(val) ? stringTable[getStringId(val)] : String(toJsValue(val));
        return parseInt(s, 10);
      },
      parse_float: (val) => {
        const s = isString(val) ? stringTable[getStringId(val)] : String(toJsValue(val));
        return parseFloat(s);
      },

      // Phase 0 fixes
      js_mod: (a, b) => a % b,
      is_null_or_undefined: (val) => {
        const bits = f64ToU64(val);
        return (bits === TAG_NULL || bits === TAG_UNDEFINED) ? 1 : 0;
      },

      // ===== Phase 1: Objects =====

      object_new: () => nanboxPointer(allocHandle({})),

      // object_set(handle, key_str, value) -> handle (for chaining)
      object_set: (handle, key, value) => {
        const obj = getHandle(handle);
        if (obj) obj[getString(key)] = toJsValue(value);
        return handle;
      },
      object_get: (handle, key) => {
        const obj = getHandle(handle);
        if (!obj) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(obj[getString(key)]);
      },
      object_get_dynamic: (handle, key) => {
        const obj = getHandle(handle);
        if (!obj) return u64ToF64(TAG_UNDEFINED);
        const k = toJsValue(key);
        return fromJsValue(obj[k]);
      },
      object_set_dynamic: (handle, key, value) => {
        const obj = getHandle(handle);
        if (obj) obj[toJsValue(key)] = toJsValue(value);
      },
      object_delete: (handle, key) => {
        const obj = getHandle(handle);
        if (obj) delete obj[getString(key)];
      },
      object_delete_dynamic: (handle, key) => {
        const obj = getHandle(handle);
        if (obj) delete obj[toJsValue(key)];
      },
      object_keys: (handle) => {
        const obj = getHandle(handle);
        return nanboxPointer(allocHandle(obj ? Object.keys(obj) : []));
      },
      object_values: (handle) => {
        const obj = getHandle(handle);
        return nanboxPointer(allocHandle(obj ? Object.values(obj) : []));
      },
      object_entries: (handle) => {
        const obj = getHandle(handle);
        return nanboxPointer(allocHandle(obj ? Object.entries(obj) : []));
      },
      object_has_property: (handle, key) => {
        const obj = getHandle(handle);
        if (!obj) return 0;
        const k = toJsValue(key);
        return (k in obj) ? 1 : 0;
      },
      // object_assign(target, source) -> target handle
      object_assign: (target, source) => {
        const t = getHandle(target);
        const s = getHandle(source);
        if (t && s) Object.assign(t, s);
        return target;
      },

      // ===== Phase 1: Arrays =====

      array_new: () => nanboxPointer(allocHandle([])),

      // array_push(handle, value) -> handle (for chaining)
      array_push: (handle, value) => {
        const arr = getHandle(handle);
        if (arr) arr.push(toJsValue(value));
        return handle;
      },
      array_pop: (handle) => {
        const arr = getHandle(handle);
        if (!arr || arr.length === 0) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(arr.pop());
      },
      array_get: (handle, index) => {
        const arr = getHandle(handle);
        if (!arr) return u64ToF64(TAG_UNDEFINED);
        const i = typeof index === 'number' ? index : toJsValue(index);
        return fromJsValue(arr[i]);
      },
      array_set: (handle, index, value) => {
        const arr = getHandle(handle);
        if (arr) arr[typeof index === 'number' ? index : toJsValue(index)] = toJsValue(value);
      },
      array_length: (handle) => {
        const arr = getHandle(handle);
        return arr ? arr.length : 0;
      },
      array_slice: (handle, start, end) => {
        const arr = getHandle(handle);
        if (!arr) return nanboxPointer(allocHandle([]));
        const s = typeof start === 'number' ? start : 0;
        const e = isUndefined(end) ? undefined : (typeof end === 'number' ? end : toJsValue(end));
        return nanboxPointer(allocHandle(arr.slice(s, e)));
      },
      array_splice: (handle, start, deleteCount) => {
        const arr = getHandle(handle);
        if (!arr) return nanboxPointer(allocHandle([]));
        const s = typeof start === 'number' ? start : 0;
        const dc = isUndefined(deleteCount) ? arr.length - s : deleteCount;
        return nanboxPointer(allocHandle(arr.splice(s, dc)));
      },
      array_shift: (handle) => {
        const arr = getHandle(handle);
        if (!arr || arr.length === 0) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(arr.shift());
      },
      array_unshift: (handle, value) => {
        const arr = getHandle(handle);
        if (arr) arr.unshift(toJsValue(value));
      },
      array_join: (handle, separator) => {
        const arr = getHandle(handle);
        if (!arr) return nanboxString(allocHandle(''));
        const sep = getString(separator);
        const result = arr.map(v => typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v)).join(sep);
        stringTable.push(result);
        return nanboxString(stringTable.length - 1);
      },
      array_index_of: (handle, value) => {
        const arr = getHandle(handle);
        if (!arr) return -1;
        const v = toJsValue(value);
        return arr.indexOf(v);
      },
      array_includes: (handle, value) => {
        const arr = getHandle(handle);
        if (!arr) return 0;
        return arr.includes(toJsValue(value)) ? 1 : 0;
      },
      array_concat: (h1, h2) => {
        const a1 = getHandle(h1) || [];
        const a2 = getHandle(h2) || [];
        return nanboxPointer(allocHandle(a1.concat(a2)));
      },
      array_reverse: (handle) => {
        const arr = getHandle(handle);
        if (arr) arr.reverse();
        return handle;
      },
      array_flat: (handle) => {
        const arr = getHandle(handle);
        return nanboxPointer(allocHandle(arr ? arr.flat() : []));
      },
      array_is_array: (val) => {
        if (!isPointer(val)) return 0;
        const obj = getHandle(val);
        return Array.isArray(obj) ? 1 : 0;
      },
      array_from: (val) => {
        const v = toJsValue(val);
        return nanboxPointer(allocHandle(Array.from(v)));
      },
      array_push_spread: (target, source) => {
        const t = getHandle(target);
        const s = getHandle(source);
        if (t && s) t.push(...s);
        return target;
      },

      // ===== Phase 1: String methods =====

      string_charAt: (str, idx) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.charAt(idx));
        return nanboxString(stringTable.length - 1);
      },
      string_substring: (str, start, end) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.substring(start, end));
        return nanboxString(stringTable.length - 1);
      },
      string_indexOf: (str, search) => {
        return stringTable[getStringId(str)].indexOf(stringTable[getStringId(search)]);
      },
      string_slice: (str, start, end) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.slice(start, end));
        return nanboxString(stringTable.length - 1);
      },
      string_toLowerCase: (str) => {
        stringTable.push(stringTable[getStringId(str)].toLowerCase());
        return nanboxString(stringTable.length - 1);
      },
      string_toUpperCase: (str) => {
        stringTable.push(stringTable[getStringId(str)].toUpperCase());
        return nanboxString(stringTable.length - 1);
      },
      string_trim: (str) => {
        stringTable.push(stringTable[getStringId(str)].trim());
        return nanboxString(stringTable.length - 1);
      },
      string_includes: (str, search) => {
        return stringTable[getStringId(str)].includes(stringTable[getStringId(search)]) ? 1 : 0;
      },
      string_startsWith: (str, search) => {
        return stringTable[getStringId(str)].startsWith(stringTable[getStringId(search)]) ? 1 : 0;
      },
      string_endsWith: (str, search) => {
        return stringTable[getStringId(str)].endsWith(stringTable[getStringId(search)]) ? 1 : 0;
      },
      string_replace: (str, pattern, replacement) => {
        const s = stringTable[getStringId(str)];
        // pattern might be a regex handle or a string
        let p, r;
        if (isPointer(pattern)) {
          p = getHandle(pattern); // RegExp object
        } else {
          p = getString(pattern);
        }
        r = getString(replacement);
        stringTable.push(s.replace(p, r));
        return nanboxString(stringTable.length - 1);
      },
      string_split: (str, delim) => {
        const s = stringTable[getStringId(str)];
        const d = getString(delim);
        return nanboxPointer(allocHandle(s.split(d)));
      },
      string_fromCharCode: (code) => {
        stringTable.push(String.fromCharCode(code));
        return nanboxString(stringTable.length - 1);
      },
      string_padStart: (str, len, fill) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.padStart(len, getString(fill)));
        return nanboxString(stringTable.length - 1);
      },
      string_padEnd: (str, len, fill) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.padEnd(len, getString(fill)));
        return nanboxString(stringTable.length - 1);
      },
      string_repeat: (str, count) => {
        stringTable.push(stringTable[getStringId(str)].repeat(count));
        return nanboxString(stringTable.length - 1);
      },
      string_match: (str, regex) => {
        const s = stringTable[getStringId(str)];
        const re = isPointer(regex) ? getHandle(regex) : new RegExp(getString(regex));
        const result = s.match(re);
        if (!result) return u64ToF64(TAG_NULL);
        return nanboxPointer(allocHandle(Array.from(result)));
      },
      math_log2: (x) => Math.log2(x),
      math_log10: (x) => Math.log10(x),

      // ===== Phase 2: Closures =====

      // closure_new(func_table_idx, capture_count) -> handle
      closure_new: (funcIdx, captureCount) => {
        const closure = { funcIdx: funcIdx, captures: new Array(captureCount | 0) };
        return nanboxPointer(allocHandle(closure));
      },
      // closure_set_capture(handle, idx, value) -> handle (chaining)
      closure_set_capture: (handle, idx, value) => {
        const c = getHandle(handle);
        if (c) c.captures[idx | 0] = value; // store raw NaN-boxed f64
        return handle;
      },
      // closure_call_N(handle, args...) -> result
      closure_call_0: (handle) => {
        const c = getHandle(handle);
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        return fn(...c.captures);
      },
      closure_call_1: (handle, a0) => {
        const c = getHandle(handle);
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        return fn(...c.captures, a0);
      },
      closure_call_2: (handle, a0, a1) => {
        const c = getHandle(handle);
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        return fn(...c.captures, a0, a1);
      },
      closure_call_3: (handle, a0, a1, a2) => {
        const c = getHandle(handle);
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        return fn(...c.captures, a0, a1, a2);
      },
      // closure_call_spread(handle, args_array_handle) -> result
      closure_call_spread: (handle, argsHandle) => {
        const c = getHandle(handle);
        const args = getHandle(argsHandle) || [];
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        const nanboxedArgs = args.map(v => fromJsValue(v));
        return fn(...c.captures, ...nanboxedArgs);
      },

      // ===== Phase 2: Array higher-order methods =====

      // Helper: call a closure/wasm function with an element
      // The closure is a handle, the callback takes (element, index, array)
      array_map: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return nanboxPointer(allocHandle([]));
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return nanboxPointer(allocHandle([]));
        const result = arr.map((v, i) => {
          const r = fn(...cb.captures, fromJsValue(v), i);
          return toJsValue(r);
        });
        return nanboxPointer(allocHandle(result));
      },
      array_filter: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return nanboxPointer(allocHandle([]));
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return nanboxPointer(allocHandle([]));
        const result = arr.filter((v, i) => {
          const r = fn(...cb.captures, fromJsValue(v), i);
          const bits = f64ToU64(r);
          if (bits === TAG_TRUE) return true;
          if (bits === TAG_FALSE || bits === TAG_NULL || bits === TAG_UNDEFINED) return false;
          return !!toJsValue(r);
        });
        return nanboxPointer(allocHandle(result));
      },
      array_forEach: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return;
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return;
        arr.forEach((v, i) => fn(...cb.captures, fromJsValue(v), i));
      },
      array_reduce: (handle, cbHandle, initial) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        let acc = isUndefined(initial) ? fromJsValue(arr[0]) : initial;
        const startIdx = isUndefined(initial) ? 1 : 0;
        for (let i = startIdx; i < arr.length; i++) {
          acc = fn(...cb.captures, acc, fromJsValue(arr[i]), i);
        }
        return acc;
      },
      array_find: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        for (let i = 0; i < arr.length; i++) {
          const v = fromJsValue(arr[i]);
          const r = fn(...cb.captures, v, i);
          if (toJsValue(r)) return v;
        }
        return u64ToF64(TAG_UNDEFINED);
      },
      array_find_index: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return -1;
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return -1;
        for (let i = 0; i < arr.length; i++) {
          const r = fn(...cb.captures, fromJsValue(arr[i]), i);
          if (toJsValue(r)) return i;
        }
        return -1;
      },
      array_sort: (handle, cbHandle) => {
        const arr = getHandle(handle);
        if (!arr) return handle;
        if (isUndefined(cbHandle) || isNull(cbHandle)) {
          arr.sort();
          return handle;
        }
        const cb = getHandle(cbHandle);
        if (!cb || !wasmInstance) { arr.sort(); return handle; }
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) { arr.sort(); return handle; }
        arr.sort((a, b) => fn(...cb.captures, fromJsValue(a), fromJsValue(b)));
        return handle;
      },
      array_some: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return 0;
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return 0;
        return arr.some((v, i) => toJsValue(fn(...cb.captures, fromJsValue(v), i))) ? 1 : 0;
      },
      array_every: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return 0;
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return 0;
        return arr.every((v, i) => toJsValue(fn(...cb.captures, fromJsValue(v), i))) ? 1 : 0;
      },

      // ===== Phase 3: Classes =====

      // Class registry: class_name -> { methods: Map, statics: Map, parent: string|null }
      // class_new(class_name_str, field_count) -> handle
      class_new: (classNameVal, fieldCount) => {
        const obj = {};
        obj.__class__ = getString(classNameVal);
        return nanboxPointer(allocHandle(obj));
      },
      // class_set_method(class_id_str, method_name_str, func_table_idx)
      class_set_method: (classId, methodName, funcIdx) => {
        const cls = getString(classId);
        if (!classMethodTable[cls]) classMethodTable[cls] = {};
        classMethodTable[cls][getString(methodName)] = funcIdx | 0;
      },
      // class_call_method(handle, method_name_str, args_array_handle) -> result
      class_call_method: (handle, methodName, argsHandle) => {
        const obj = getHandle(handle);
        if (!obj) return u64ToF64(TAG_UNDEFINED);
        const mname = getString(methodName);
        let cls = obj.__class__;
        while (cls) {
          const methods = classMethodTable[cls];
          if (methods && mname in methods) {
            const fn = wasmInstance?.exports.__indirect_function_table?.get(methods[mname]);
            if (fn) {
              const args = getHandle(argsHandle) || [];
              return fn(handle, ...args.map(v => fromJsValue(v)));
            }
          }
          cls = classParentTable[cls] || null;
        }
        return u64ToF64(TAG_UNDEFINED);
      },
      class_get_field: (handle, name) => {
        const obj = getHandle(handle);
        if (!obj) return u64ToF64(TAG_UNDEFINED);
        const fname = getString(name);
        // Check for compiled getter method
        if (obj.__class__) {
          let cls = obj.__class__;
          while (cls) {
            const methods = classMethodTable[cls];
            if (methods && ('__get_' + fname) in methods) {
              const fn = wasmInstance?.exports.__indirect_function_table?.get(methods['__get_' + fname]);
              if (fn) return fn(handle);
            }
            cls = classParentTable[cls] || null;
          }
        }
        return fromJsValue(obj[fname]);
      },
      class_set_field: (handle, name, value) => {
        const obj = getHandle(handle);
        if (!obj) return;
        const fname = getString(name);
        // Check for compiled setter method
        if (obj.__class__) {
          let cls = obj.__class__;
          while (cls) {
            const methods = classMethodTable[cls];
            if (methods && ('__set_' + fname) in methods) {
              const fn = wasmInstance?.exports.__indirect_function_table?.get(methods['__set_' + fname]);
              if (fn) { fn(handle, value); return; }
            }
            cls = classParentTable[cls] || null;
          }
        }
        obj[fname] = toJsValue(value);
      },
      class_set_static: (classId, name, value) => {
        const cls = getString(classId);
        if (!classStaticTable[cls]) classStaticTable[cls] = {};
        classStaticTable[cls][getString(name)] = toJsValue(value);
      },
      class_get_static: (classId, name) => {
        const cls = getString(classId);
        const statics = classStaticTable[cls];
        if (!statics) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(statics[getString(name)]);
      },
      class_instanceof: (handle, classId) => {
        const obj = getHandle(handle);
        if (!obj) return 0;
        let cls = obj.__class__;
        const target = getString(classId);
        while (cls) {
          if (cls === target) return 1;
          cls = classParentTable[cls] || null;
        }
        return 0;
      },

      // ===== Phase 4: JSON =====

      json_parse: (str) => {
        try {
          const s = getString(str);
          return fromJsValue(JSON.parse(s));
        } catch (e) {
          if (tryDepth > 0) { currentException = fromJsValue(e); }
          return u64ToF64(TAG_UNDEFINED);
        }
      },
      json_stringify: (val) => {
        const js = toJsValue(val);
        stringTable.push(JSON.stringify(js));
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 4: Map =====

      map_new: () => nanboxPointer(allocHandle(new Map())),
      map_set: (handle, key, value) => {
        const m = getHandle(handle);
        if (m) m.set(toJsValue(key), toJsValue(value));
      },
      map_get: (handle, key) => {
        const m = getHandle(handle);
        if (!m) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(m.get(toJsValue(key)));
      },
      map_has: (handle, key) => {
        const m = getHandle(handle);
        return m?.has(toJsValue(key)) ? 1 : 0;
      },
      map_delete: (handle, key) => {
        const m = getHandle(handle);
        if (m) m.delete(toJsValue(key));
      },
      map_size: (handle) => {
        const m = getHandle(handle);
        return m ? m.size : 0;
      },
      map_clear: (handle) => {
        const m = getHandle(handle);
        if (m) m.clear();
      },
      map_entries: (handle) => {
        const m = getHandle(handle);
        return nanboxPointer(allocHandle(m ? [...m.entries()] : []));
      },
      map_keys: (handle) => {
        const m = getHandle(handle);
        return nanboxPointer(allocHandle(m ? [...m.keys()] : []));
      },
      map_values: (handle) => {
        const m = getHandle(handle);
        return nanboxPointer(allocHandle(m ? [...m.values()] : []));
      },

      // ===== Phase 4: Set =====

      set_new: () => nanboxPointer(allocHandle(new Set())),
      set_new_from_array: (arrHandle) => {
        const arr = getHandle(arrHandle) || toJsValue(arrHandle);
        return nanboxPointer(allocHandle(new Set(Array.isArray(arr) ? arr : [])));
      },
      set_add: (handle, value) => {
        const s = getHandle(handle);
        if (s) s.add(toJsValue(value));
      },
      set_has: (handle, value) => {
        const s = getHandle(handle);
        return s?.has(toJsValue(value)) ? 1 : 0;
      },
      set_delete: (handle, value) => {
        const s = getHandle(handle);
        if (s) s.delete(toJsValue(value));
      },
      set_size: (handle) => {
        const s = getHandle(handle);
        return s ? s.size : 0;
      },
      set_clear: (handle) => {
        const s = getHandle(handle);
        if (s) s.clear();
      },
      set_values: (handle) => {
        const s = getHandle(handle);
        return nanboxPointer(allocHandle(s ? [...s.values()] : []));
      },

      // ===== Phase 4: Date =====

      date_new_val: (arg) => {
        const d = isUndefined(arg) ? new Date() : new Date(toJsValue(arg));
        return nanboxPointer(allocHandle(d));
      },
      date_get_time: (handle) => {
        const d = getHandle(handle);
        return d instanceof Date ? d.getTime() : 0;
      },
      date_to_iso_string: (handle) => {
        const d = getHandle(handle);
        if (!(d instanceof Date)) return u64ToF64(TAG_UNDEFINED);
        stringTable.push(d.toISOString());
        return nanboxString(stringTable.length - 1);
      },
      date_get_full_year: (h) => { const d = getHandle(h); return d instanceof Date ? d.getFullYear() : 0; },
      date_get_month: (h) => { const d = getHandle(h); return d instanceof Date ? d.getMonth() : 0; },
      date_get_date: (h) => { const d = getHandle(h); return d instanceof Date ? d.getDate() : 0; },
      date_get_hours: (h) => { const d = getHandle(h); return d instanceof Date ? d.getHours() : 0; },
      date_get_minutes: (h) => { const d = getHandle(h); return d instanceof Date ? d.getMinutes() : 0; },
      date_get_seconds: (h) => { const d = getHandle(h); return d instanceof Date ? d.getSeconds() : 0; },
      date_get_milliseconds: (h) => { const d = getHandle(h); return d instanceof Date ? d.getMilliseconds() : 0; },

      // ===== Phase 4: Error =====

      error_new: (msg) => {
        const message = isUndefined(msg) ? undefined : getString(msg);
        return nanboxPointer(allocHandle(new Error(message)));
      },
      error_message: (handle) => {
        const e = getHandle(handle);
        const msg = e instanceof Error ? e.message : '';
        stringTable.push(msg);
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 4: RegExp =====

      regexp_new: (pattern, flags) => {
        try {
          const p = getString(pattern);
          const f = getString(flags);
          return nanboxPointer(allocHandle(new RegExp(p, f)));
        } catch (e) {
          if (tryDepth > 0) { currentException = fromJsValue(e); }
          return u64ToF64(TAG_UNDEFINED);
        }
      },
      regexp_test: (regex, str) => {
        const re = getHandle(regex);
        if (!(re instanceof RegExp)) return 0;
        return re.test(getString(str)) ? 1 : 0;
      },

      // ===== Phase 4: Globals =====

      number_coerce: (val) => {
        const v = toJsValue(val);
        return Number(v);
      },
      is_nan: (val) => Number.isNaN(val) ? 1 : 0,
      is_finite: (val) => Number.isFinite(val) ? 1 : 0,

      // ===== Phase 5: Misc =====

      console_log_multi: (argsHandle) => {
        const args = getHandle(argsHandle);
        if (args) console.log(...args.map(toJsValue));
      },

      // ===== Phase 1 Addition: Class inheritance =====

      class_set_parent: (childId, parentId) => {
        const child = getString(childId);
        const parent = getString(parentId);
        classParentTable[child] = parent;
      },

      // ===== Phase 3: Try/Catch =====

      try_start: () => { tryDepth++; },
      try_end: () => { if (tryDepth > 0) tryDepth--; },
      throw_value: (val) => { currentException = val; },
      has_exception: () => currentException !== null ? 1 : 0,
      get_exception: () => {
        const e = currentException;
        currentException = null;
        return e !== null ? e : u64ToF64(TAG_UNDEFINED);
      },

      // ===== Phase 4: URL =====

      url_parse: (urlStr) => {
        try {
          return nanboxPointer(allocHandle(new URL(getString(urlStr))));
        } catch (e) {
          if (tryDepth > 0) { currentException = fromJsValue(e); }
          return u64ToF64(TAG_UNDEFINED);
        }
      },
      url_get_href: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.href); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_pathname: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.pathname); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_hostname: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.hostname); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_port: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.port); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_search: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.search); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_hash: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.hash); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_origin: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.origin); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_protocol: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.protocol); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_search_params: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) return nanboxPointer(allocHandle(u.searchParams));
        return u64ToF64(TAG_UNDEFINED);
      },
      searchparams_get: (handle, key) => {
        const sp = getHandle(handle);
        if (!sp || typeof sp.get !== 'function') return u64ToF64(TAG_NULL);
        const v = sp.get(getString(key));
        if (v === null) return u64ToF64(TAG_NULL);
        stringTable.push(v);
        return nanboxString(stringTable.length - 1);
      },
      searchparams_has: (handle, key) => {
        const sp = getHandle(handle);
        return (sp && typeof sp.has === 'function' && sp.has(getString(key))) ? 1 : 0;
      },
      searchparams_set: (handle, key, val) => {
        const sp = getHandle(handle);
        if (sp && typeof sp.set === 'function') sp.set(getString(key), getString(val));
      },
      searchparams_append: (handle, key, val) => {
        const sp = getHandle(handle);
        if (sp && typeof sp.append === 'function') sp.append(getString(key), getString(val));
      },
      searchparams_delete: (handle, key) => {
        const sp = getHandle(handle);
        if (sp && typeof sp.delete === 'function') sp.delete(getString(key));
      },
      searchparams_to_string: (handle) => {
        const sp = getHandle(handle);
        stringTable.push(sp ? sp.toString() : '');
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 4: Crypto =====

      crypto_random_uuid: () => {
        stringTable.push(crypto.randomUUID());
        return nanboxString(stringTable.length - 1);
      },
      crypto_random_bytes: (n) => {
        return nanboxPointer(allocHandle(crypto.getRandomValues(new Uint8Array(n))));
      },

      // ===== Phase 4: Path =====

      path_join: (a, b) => {
        const sa = getString(a), sb = getString(b);
        const joined = (sa + '/' + sb).replace(/\/+/g, '/');
        stringTable.push(joined);
        return nanboxString(stringTable.length - 1);
      },
      path_dirname: (str) => {
        const s = getString(str);
        const idx = s.lastIndexOf('/');
        stringTable.push(idx >= 0 ? (s.substring(0, idx) || '/') : '.');
        return nanboxString(stringTable.length - 1);
      },
      path_basename: (str) => {
        const s = getString(str);
        const idx = s.lastIndexOf('/');
        stringTable.push(idx >= 0 ? s.substring(idx + 1) : s);
        return nanboxString(stringTable.length - 1);
      },
      path_extname: (str) => {
        const s = getString(str);
        const base = s.substring(s.lastIndexOf('/') + 1);
        const idx = base.lastIndexOf('.');
        stringTable.push(idx > 0 ? base.substring(idx) : '');
        return nanboxString(stringTable.length - 1);
      },
      path_resolve: (str) => {
        stringTable.push(getString(str));
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 4: Process/OS =====

      os_platform: () => {
        stringTable.push('wasm');
        return nanboxString(stringTable.length - 1);
      },
      process_argv: () => nanboxPointer(allocHandle([])),
      process_cwd: () => {
        stringTable.push('/');
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 6: Buffer/Uint8Array =====

      buffer_alloc: (size) => nanboxPointer(allocHandle(new Uint8Array(size))),
      buffer_from_string: (str, encoding) => {
        const s = getString(str);
        return nanboxPointer(allocHandle(new TextEncoder().encode(s)));
      },
      buffer_to_string: (handle, encoding) => {
        const buf = getHandle(handle);
        if (!buf) { stringTable.push(''); return nanboxString(stringTable.length - 1); }
        stringTable.push(new TextDecoder().decode(buf));
        return nanboxString(stringTable.length - 1);
      },
      buffer_get: (handle, idx) => {
        const buf = getHandle(handle);
        return buf ? buf[idx] : 0;
      },
      buffer_set: (handle, idx, val) => {
        const buf = getHandle(handle);
        if (buf) buf[idx] = val;
      },
      buffer_length: (handle) => {
        const buf = getHandle(handle);
        return buf ? buf.length : 0;
      },
      buffer_slice: (handle, start, end) => {
        const buf = getHandle(handle);
        if (!buf) return nanboxPointer(allocHandle(new Uint8Array(0)));
        const e = isUndefined(end) ? undefined : end;
        return nanboxPointer(allocHandle(buf.slice(start, e)));
      },
      buffer_concat: (arrHandle) => {
        const arr = getHandle(arrHandle);
        if (!arr || !Array.isArray(arr)) return nanboxPointer(allocHandle(new Uint8Array(0)));
        const bufs = arr.map(v => {
          const h = typeof v === 'number' ? getHandle(v) : v;
          return h instanceof Uint8Array ? h : new Uint8Array(0);
        });
        const total = bufs.reduce((s, b) => s + b.length, 0);
        const result = new Uint8Array(total);
        let offset = 0;
        bufs.forEach(b => { result.set(b, offset); offset += b.length; });
        return nanboxPointer(allocHandle(result));
      },
      uint8array_new: (size) => nanboxPointer(allocHandle(new Uint8Array(size))),
      uint8array_from: (val) => {
        const v = toJsValue(val);
        return nanboxPointer(allocHandle(Uint8Array.from(Array.isArray(v) ? v : [])));
      },
      uint8array_length: (handle) => {
        const buf = getHandle(handle);
        return buf ? buf.length : 0;
      },
      uint8array_get: (handle, idx) => {
        const buf = getHandle(handle);
        return buf ? buf[idx] : 0;
      },
      uint8array_set: (handle, idx, val) => {
        const buf = getHandle(handle);
        if (buf) buf[idx] = val;
      },

      // ===== Timers =====

      set_timeout: (closureHandle, delay) => {
        const cb = getHandle(closureHandle);
        if (!cb || !wasmInstance) return 0;
        const id = setTimeout(() => {
          const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
          if (fn) fn(...cb.captures);
        }, delay);
        return id;
      },
      set_interval: (closureHandle, delay) => {
        const cb = getHandle(closureHandle);
        if (!cb || !wasmInstance) return 0;
        const id = setInterval(() => {
          const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
          if (fn) fn(...cb.captures);
        }, delay);
        return id;
      },
      clear_timeout: (id) => { clearTimeout(id); },
      clear_interval: (id) => { clearInterval(id); },

      // ===== Response properties =====

      response_status: (handle) => {
        const r = getHandle(handle);
        return (r && typeof r.status === 'number') ? r.status : 0;
      },
      response_ok: (handle) => {
        const r = getHandle(handle);
        return (r && r.ok) ? 1 : 0;
      },
      response_headers_get: (handle, name) => {
        const r = getHandle(handle);
        if (!r || !r.headers) return u64ToF64(TAG_NULL);
        const v = r.headers.get(getString(name));
        if (v === null) return u64ToF64(TAG_NULL);
        stringTable.push(v);
        return nanboxString(stringTable.length - 1);
      },
      response_url: (handle) => {
        const r = getHandle(handle);
        if (!r || typeof r.url !== 'string') return u64ToF64(TAG_UNDEFINED);
        stringTable.push(r.url);
        return nanboxString(stringTable.length - 1);
      },

      // ===== Buffer extras =====

      buffer_copy: (source, target, targetStart, sourceStart, sourceEnd) => {
        const src = getHandle(source);
        const tgt = getHandle(target);
        if (!src || !tgt) return 0;
        const ts = isUndefined(targetStart) ? 0 : targetStart;
        const ss = isUndefined(sourceStart) ? 0 : sourceStart;
        const se = isUndefined(sourceEnd) ? src.length : sourceEnd;
        let copied = 0;
        for (let i = ss; i < se && (ts + copied) < tgt.length; i++) {
          tgt[ts + copied] = src[i];
          copied++;
        }
        return copied;
      },
      buffer_write: (handle, str, offset, encoding) => {
        const buf = getHandle(handle);
        if (!buf) return 0;
        const s = getString(str);
        const encoded = new TextEncoder().encode(s);
        const off = isUndefined(offset) ? 0 : offset;
        let written = 0;
        for (let i = 0; i < encoded.length && (off + i) < buf.length; i++) {
          buf[off + i] = encoded[i];
          written++;
        }
        return written;
      },
      buffer_equals: (handle, other) => {
        const a = getHandle(handle);
        const b = getHandle(other);
        if (!a || !b) return 0;
        if (a.length !== b.length) return 0;
        for (let i = 0; i < a.length; i++) {
          if (a[i] !== b[i]) return 0;
        }
        return 1;
      },
      buffer_is_buffer: (val) => {
        if (!isPointer(val)) return 0;
        const obj = getHandle(val);
        return (obj instanceof Uint8Array || (obj && obj.constructor && obj.constructor.name === 'Buffer')) ? 1 : 0;
      },
      buffer_byte_length: (val) => {
        if (isString(val)) return new TextEncoder().encode(getString(val)).length;
        if (isPointer(val)) {
          const obj = getHandle(val);
          if (obj instanceof Uint8Array) return obj.length;
        }
        return 0;
      },

      // ===== Crypto extras =====

      crypto_sha256: (data) => {
        const str = getString(data);
        const p = (typeof crypto !== 'undefined' && crypto.subtle)
          ? crypto.subtle.digest('SHA-256', new TextEncoder().encode(str))
              .then(buf => { const hex = [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2, '0')).join(''); stringTable.push(hex); return nanboxString(stringTable.length - 1); })
          : Promise.resolve(u64ToF64(TAG_UNDEFINED));
        return nanboxPointer(allocHandle(p));
      },
      crypto_md5: (data) => {
        // MD5 not available in Web Crypto API; return undefined
        // Users should use SHA-256 instead
        return u64ToF64(TAG_UNDEFINED);
      },

      // ===== Path extras =====

      path_is_absolute: (str) => {
        const s = getString(str);
        return (s.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(s)) ? 1 : 0;
      },

      // ===== Phase 5: Async/Promise/Fetch =====

      fetch_url: (urlStr) => {
        const url = getString(urlStr);
        const p = fetch(url);
        return nanboxPointer(allocHandle(p));
      },
      fetch_with_options: (urlStr, methodVal, bodyVal, headersVal) => {
        const url = getString(urlStr);
        const opts = {};
        if (!isUndefined(methodVal)) opts.method = getString(methodVal);
        if (!isUndefined(bodyVal)) opts.body = getString(bodyVal);
        if (isPointer(headersVal)) {
          const h = getHandle(headersVal);
          if (h && typeof h === 'object') opts.headers = h;
        }
        const p = fetch(url, opts);
        return nanboxPointer(allocHandle(p));
      },
      response_json: (handle) => {
        const resp = getHandle(handle);
        if (!resp || typeof resp.json !== 'function') return u64ToF64(TAG_UNDEFINED);
        const p = resp.json().then(v => fromJsValue(v));
        return nanboxPointer(allocHandle(p));
      },
      response_text: (handle) => {
        const resp = getHandle(handle);
        if (!resp || typeof resp.text !== 'function') return u64ToF64(TAG_UNDEFINED);
        const p = resp.text().then(v => fromJsValue(v));
        return nanboxPointer(allocHandle(p));
      },
      promise_new: () => {
        let resolve;
        const p = new Promise(r => { resolve = r; });
        p.__resolve = resolve;
        return nanboxPointer(allocHandle(p));
      },
      promise_resolve: (handle, value) => {
        const p = getHandle(handle);
        if (p && p.__resolve) p.__resolve(toJsValue(value));
      },
      promise_then: (handle, closureHandle) => {
        const p = getHandle(handle);
        const cb = getHandle(closureHandle);
        if (!p || !(p instanceof Promise)) return handle;
        const newP = p.then(val => {
          if (cb && wasmInstance) {
            const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
            if (fn) return fn(...cb.captures, fromJsValue(val));
          }
          return fromJsValue(val);
        });
        return nanboxPointer(allocHandle(newP));
      },
      await_promise: (val) => {
        // In synchronous WASM context, we can't truly await.
        // If it's not a promise handle, return as-is.
        // If it IS a promise, return the handle (caller gets a promise handle back).
        // True awaiting happens in JS async functions.
        if (!isPointer(val)) return val;
        const obj = getHandle(val);
        if (obj instanceof Promise) {
          // Can't await synchronously; return the promise handle
          return val;
        }
        return val;
      },

      // Async function implementations (merged from generated code)
      ...(typeof __asyncFuncImpls !== 'undefined' ? __asyncFuncImpls : {}),
    }
  };
}

// Class method/static/parent tables
const classMethodTable = {};
const classStaticTable = {};
const classParentTable = {};

// Exception state for try/catch bridge
let tryDepth = 0;
let currentException = null;

// Boot the WASM module
async function bootPerryWasm(wasmBase64) {
  const wasmBytes = Uint8Array.from(atob(wasmBase64), c => c.charCodeAt(0));
  const imports = buildImports();
  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  wasmInstance = instance;
  wasmMemory = instance.exports.memory;
  // Call the entry point
  if (instance.exports._start) {
    instance.exports._start();
  } else if (instance.exports.main) {
    instance.exports.main();
  }
}
