// MiniSat (https://github.com/niklasso/minisat, MIT licence), Niklas Eén and Niklas
// Sörensson's classic CDCL solver, compiled to WebAssembly: dist/solvers/minisat.{mjs,wasm}.

import { loadSolverModule } from './emscripten.js';

/**
 * Loads MiniSat once per worker (see loadSolverModule).
 *
 * @param {URL | string} [base]  directory URL of the built modules; defaults to dist/solvers/
 * @returns {Promise<import('./emscripten.js').SolverBackend>}
 */
export function loadMinisat(base) {
  return loadSolverModule('minisat', base);
}
