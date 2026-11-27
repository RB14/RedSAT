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

/** Not a solver: runs several at once (one Web Worker each) and keeps the first to finish. */
export const RACE = { id: 'race', label: 'Race solvers', detail: 'all solvers and CaDiCaL variants in parallel, one per core; the fastest wins' };

export function solverById(id) {
  return SOLVERS.find((solver) => solver.id === id) ?? SOLVERS[0];
}

/** Differently configured CaDiCaL instances, the usual ways to diversify a portfolio. */
const CADICAL_VARIANTS = [
  { label: 'CaDiCaL (sat preset)', options: { preset: 'sat', seed: 1, phase: 0 } },
  { label: 'CaDiCaL (unsat preset)', options: { preset: 'unsat', seed: 2 } },
  { label: 'CaDiCaL (shuffled)', options: { seed: 3, phase: 0, shuffle: true } },
  { label: 'CaDiCaL (sat, shuffled)', options: { preset: 'sat', seed: 4, shuffle: true } },
  { label: 'CaDiCaL (seed 5)', options: { seed: 5, phase: 0 } },
];

/**
 * Who races, one Web Worker each: every solver, then differently configured CaDiCaL instances,
 * as many as the machine has cores for (at least the three solvers, at most 8 workers).
 */
export function raceMembers(cores = globalThis.navigator?.hardwareConcurrency ?? 4) {
  const members = SOLVERS.map((solver) => ({ id: solver.id, label: solver.label, options: undefined }));
  const extra = Math.max(0, Math.min(cores, 8) - members.length);
  for (const variant of CADICAL_VARIANTS.slice(0, extra)) members.push({ id: 'cadical', ...variant });
  return members;
}
