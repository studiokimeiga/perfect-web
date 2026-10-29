// Test-only faults at the real renderer's Wasm boundary. Keep its memory,
// other exports and request ABI; a selected export executes unreachable.
const require = (ok, why) => { if (!ok) throw new Error(`renderer fault fixture: ${why}`); };

function unsigned(bytes, cursor) {
  let value = 0;
  for (let shift = 0; shift < 35; shift += 7) {
    require(cursor.at < bytes.length, "truncated unsigned integer");
    const byte = bytes[cursor.at++];
    require(shift < 28 || byte < 16, "unsigned integer overflows u32");
    value += (byte & 127) * 2 ** shift;
    if (!(byte & 128)) return value;
  }
  throw new Error("renderer fault fixture: unterminated unsigned integer");
}

function encoded(value) {
  const bytes = [];
  do {
    const byte = value & 127;
    value = Math.floor(value / 128);
    bytes.push(byte | (value ? 128 : 0));
  } while (value);
  return Uint8Array.from(bytes);
}

function joined(chunks) {
  const bytes = new Uint8Array(chunks.reduce((total, chunk) => total + chunk.length, 0));
  let at = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, at);
    at += chunk.length;
  }
  return bytes;
}

function sections(bytes) {
  require(bytes.length >= 8 && [...bytes.subarray(0, 8)].join() === "0,97,115,109,1,0,0,0", "expected Wasm v1");
  const found = [];
  const cursor = { at: 8 };
  while (cursor.at < bytes.length) {
    const id = bytes[cursor.at++];
    const length = unsigned(bytes, cursor);
    require(cursor.at + length <= bytes.length, "truncated section");
    found.push({ id, bytes: bytes.subarray(cursor.at, cursor.at + length) });
    cursor.at += length;
  }
  return found;
}

function exportedFunctions(bytes) {
  const cursor = { at: 0 };
  const count = unsigned(bytes, cursor);
  const functions = new Map();
  for (let i = 0; i < count; i++) {
    const length = unsigned(bytes, cursor);
    require(cursor.at + length < bytes.length, "truncated export name");
    const name = new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(cursor.at, cursor.at + length));
    cursor.at += length;
    const kind = bytes[cursor.at++];
    const index = unsigned(bytes, cursor);
    if (kind === 0) functions.set(name, index);
  }
  require(cursor.at === bytes.length, "extra export bytes");
  return functions;
}

function trappedCode(bytes, selected) {
  const cursor = { at: 0 };
  const count = unsigned(bytes, cursor);
  require([...selected].every((index) => index < count), "export is not a defined function");
  const chunks = [encoded(count)];
  for (let index = 0; index < count; index++) {
    const length = unsigned(bytes, cursor);
    require(length > 0 && cursor.at + length <= bytes.length, "truncated function body");
    const body = selected.has(index) ? Uint8Array.of(0, 0, 11) : bytes.subarray(cursor.at, cursor.at + length);
    chunks.push(encoded(body.length), body);
    cursor.at += length;
  }
  require(cursor.at === bytes.length, "extra code bytes");
  return joined(chunks);
}

export function rendererWithTraps(original, names) {
  const bytes = new Uint8Array(original);
  const module = new WebAssembly.Module(bytes);
  require(WebAssembly.Module.imports(module).length === 0, "imports are outside this fixture");
  require(names.length > 0 && new Set(names).size === names.length, "select distinct renderer exports");
  const parts = sections(bytes);
  const exports = parts.filter((part) => part.id === 7);
  const code = parts.filter((part) => part.id === 10);
  require(exports.length === 1 && code.length === 1, "expected one export and code section");
  const functions = exportedFunctions(exports[0].bytes);
  require(names.every((name) => functions.has(name)), "selected renderer export is absent");
  const selected = new Set(names.map((name) => functions.get(name)));
  const changed = trappedCode(code[0].bytes, selected);
  const result = joined([bytes.subarray(0, 8), ...parts.flatMap((part) => {
    const payload = part.id === 10 ? changed : part.bytes;
    return [Uint8Array.of(part.id), encoded(payload.length), payload];
  })]);
  require(WebAssembly.validate(result), "fault is not a valid executable Wasm module");
  return result;
}
