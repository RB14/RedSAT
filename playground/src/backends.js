// The SAT solvers the playground offers. The built-in one is part of the compiler; the others
// are separate WebAssembly modules (compiled with Emscripten, see src/solvers/), loaded on first
// use. "Race" runs the program with every solver in parallel workers and keeps the fastest.

import { loadCadical } from './solvers/cadical.js';
import { loadMinisat } from './solvers/minisat.js';

export const SOLVERS = [
  { id: 'cadical', label: 'CaDiCaL', detail: 'state of the art, C++', load: () => loadCadical() },
  { id: 'builtin', label: 'Built-in CDCL', detail: "RedSAT's own, Rust", load: null },
  { id: 'minisat', label: 'MiniSat', detail: 'the classic, C++', load: () => loadMinisat() },
];

/** Not a solver: runs all of them at once (one Web Worker each) and keeps the first to finish. */
export const RACE = { id: 'race', label: 'Race all solvers', detail: 'in parallel, fastest wins' };

export function solverById(id) {
  return SOLVERS.find((solver) => solver.id === id) ?? SOLVERS[0];
}
