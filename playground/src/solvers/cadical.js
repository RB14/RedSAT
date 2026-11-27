// CaDiCaL (https://github.com/arminbiere/cadical, MIT licence), Armin Biere et al.'s
// incremental CDCL solver, compiled to WebAssembly: dist/solvers/cadical.{mjs,wasm}.

import { loadSolverModule } from './emscripten.js';

/**
 * Loads CaDiCaL once per worker (see loadSolverModule).
 *
 * @param {URL | string} [base]  directory URL of the built modules; defaults to dist/solvers/
 * @returns {Promise<import('./emscripten.js').SolverBackend>}
 */
export function loadCadical(base) {
  return loadSolverModule('cadical', base);
}
