/* The browser side of E7V.
 *
 * This is NOT a JavaScript implementation of the compatibility decision. It
 * loads `runtime/pw-resume` compiled to wasm and calls it — one implementation,
 * so the browser and the deployment matrix cannot disagree. A JS port would be
 * two things that can differ, and the difference would be silent: both return a
 * boolean and only one of them is right.
 *
 * There is no `attach` in this file. The wasm module takes the authorisation
 * inside its own call, so a page has no way to attach a handler except through
 * a decision that returned Resume or Migrate.
 */
const REFUSAL = {
  2: "unsupported hash scheme",
  3: "unsupported platform ABI",
  4: "unknown handler",
  5: "privacy widened",
  6: "document schema mismatch",
  7: "no migration",
  8: "migration failed",
  9: "malformed manifest",
  10: "mixed build",
};

const RECOVERY = [
  "refetch-region",
  "rerender-private-slot",
  "reload-document",
  "retry-interaction",
  "require-user-confirmation",
  "reject-irrecoverable",
];

/* Loaded EAGERLY, at module import, so `decide` is synchronous.
 *
 * That is not a performance choice. The generated handler is
 * `onClick() { cart = add_to_cart(..) }`, so an async gate makes the handler
 * assign a PROMISE and the rendered cart goes blank — which is what the
 * browser tests found, in all three engines, on the first run. A compatibility
 * check that changes what the handler returns has changed the program.
 */
let wasm = null;

export async function load(url = "/pw-resume.wasm") {
  if (wasm) return wasm;
  const { instance } = await WebAssembly.instantiateStreaming(fetch(url), {});
  wasm = instance.exports;
  return wasm;
}

/** True once the decision can be made synchronously. */
export function ready() {
  return wasm !== null;
}

/** Decide whether a manifest may attach. Synchronous; see `load`. */
export function decide(manifest) {
  const w = wasm;
  if (!w) throw new Error("pw-resume wasm not loaded");
  const bytes = new TextEncoder().encode(manifest);
  const ptr = w.alloc(bytes.length);
  new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);

  const code = w.decide_manifest(ptr, bytes.length);
  if (code === 0) return { attached: true, kind: "resume" };
  if (code === 1) return { attached: true, kind: "migrate" };

  // The decision keeps no trace in the browser (ADR-0400): a refusal is
  // named by its code, `PWR04`, which pleris.dev explains.
  return {
    attached: false,
    why: REFUSAL[code] ?? `refusal ${code}`,
    recovery: RECOVERY[w.last_recovery()] ?? "unknown",
    code: `PWR${String(code).padStart(2, "0")}`,
  };
}
