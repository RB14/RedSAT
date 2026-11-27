// Loads a SAT solver compiled to WebAssembly with Emscripten (scripts/build-solvers.mjs) and
// wraps it as a SolverBackend. Every such module exports the C ABI of native/glue.hpp.

/**
 * One incremental solver instance. Literals are DIMACS integers (±var, var ≥ 1).
 *
 * @typedef {object} SatSolver
 * @property {(clauses: Int32Array) => void} add  adds zero-terminated clauses (possibly several
 *   in one call; a clause may also continue in the next call). Clauses accumulate across solves.
 * @property {(assumptions: Int32Array) => number} solve  10 = SAT, 20 = UNSAT, 0 = unknown;
 *   the assumptions hold for this call only
 * @property {(lit: number) => boolean} value  whether lit is true in the last model (false
 *   when the last solve was not SAT, and for variables without a value)
 * @property {(lit: number) => boolean} failed  whether the assumption lit is in the unsat core
 *   of the last solve (false when it was not UNSAT)
 * @property {() => void} release  frees the instance; it must not be used afterwards
 */

/**
 * A loaded solver module: create() makes independent instances that share its memory.
 *
 * @typedef {object} SolverBackend
 * @property {string} id  e.g. 'cadical'
 * @property {string} name  display name with version, e.g. 'CaDiCaL 2.2.1'
 * @property {() => SatSolver} create
 * @property {() => number} memoryBytes  the module's current WebAssembly memory size (it grows
 *   on demand and never shrinks)
 */

/** Where the built modules are served: dist/solvers/, next to the bundled worker. */
const SOLVERS_URL = new URL('./solvers/', import.meta.url);

/** One backend per module URL, until it crashes. */
const backends = new Map();

/**
 * Loads `<id>.mjs` (and its `.wasm`) from `base`, once: later calls return the same backend,
 * unless the module aborted (e.g. out of memory), in which case a fresh one is instantiated.
 *
 * @param {string} id
 * @param {URL | string} [base]  directory URL of the built modules
 * @returns {Promise<SolverBackend>}
 */
export function loadSolverModule(id, base = SOLVERS_URL) {
  const url = new URL(`${id}.mjs`, base);
  let backend = backends.get(url.href);
  if (!backend) {
    const forget = () => {
      if (backends.get(url.href) === backend) backends.delete(url.href);
    };
    backend = instantiate(id, url, forget);
    backend.catch(forget);
    backends.set(url.href, backend);
  }
  return backend;
}

async function instantiate(id, url, forget) {
  // A computed specifier, so that esbuild leaves the module out of the bundle.
  const { default: createModule } = await import(url.href);
  let crash = null;
  const module = await createModule({
    locateFile: (path) => new URL(path, url).href,
    print: (text) => console.log(`[${id}] ${text}`),
    printErr: (text) => console.warn(`[${id}] ${text}`),
    onAbort: (reason) => {
      crash = String(reason ?? 'aborted');
      forget();
    },
  });
  const name = module.UTF8ToString(module._solver_name());

  /** Copies literals into the module's scratch buffer, returning how many there are. */
  const put = (lits) => {
    // Pointers above 2 GiB come back negative from a 32-bit export.
    const pointer = module._solver_buffer(lits.length) >>> 0;
    module.HEAP32.set(lits, pointer >>> 2); // HEAP32 after the call: memory may have grown
    return lits.length;
  };

  const create = () => {
    if (crash) throw new Error(`${name} crashed (${crash}); load it again`);
    let handle = module._solver_new();
    const live = () => {
      if (crash) throw new Error(`${name} crashed (${crash})`);
      if (handle === null) throw new Error(`${name}: the solver instance was released`);
      return handle;
    };
    return {
      add: (clauses) => module._solver_add(live(), put(clauses)),
      solve: (assumptions) => module._solver_solve(live(), put(assumptions)),
      value: (lit) => module._solver_value(live(), lit) !== 0,
      failed: (lit) => module._solver_failed(live(), lit) !== 0,
      release: () => {
        if (handle !== null && !crash) module._solver_release(handle);
        handle = null;
      },
    };
  };

  return { id, name, create, memoryBytes: () => module.HEAP32.buffer.byteLength };
}
