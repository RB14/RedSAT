// The SAT solvers the playground offers. The built-in one is part of the compiler; the others
// are separate WebAssembly modules (compiled with Emscripten), loaded on first use.

export const SOLVERS = [
  {
    id: 'builtin',
    label: 'Built-in CDCL',
    detail: "RedSAT's own solver (Rust)",
    load: null,
  },
];

export function solverById(id) {
  return SOLVERS.find((solver) => solver.id === id) ?? SOLVERS[0];
}
