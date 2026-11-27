// `npm test`: the WebAssembly solvers (built by build-solvers.mjs) honour the SatSolver contract
// that the compiler relies on: models, UNSAT, incremental use, assumptions and their cores, and
// many instances created and released. Skipped when the solvers have not been built.

import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { loadSolverModule } from '../src/solvers/emscripten.js';

const directory = fileURLToPath(new URL('../pkg/solvers/', import.meta.url));
const base = pathToFileURL(directory);
const ids = ['cadical', 'minisat'].filter((id) => existsSync(`${directory}${id}.mjs`));
const clauses = (...list) => Int32Array.from(list.flatMap((clause) => [...clause, 0]));

/** A random 3-SAT formula with `vars` variables at a clause ratio of 4 (satisfiable, mostly). */
function random3Sat(vars, seed) {
  let state = seed;
  const next = () => (state = (state * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
  const formula = [];
  for (let i = 0; i < vars * 4; i++) {
    formula.push([0, 1, 2].map(() => (1 + Math.floor(next() * vars)) * (next() < 0.5 ? -1 : 1)));
  }
  return formula;
}

if (!ids.length) {
  test('WebAssembly solvers', { skip: 'not built: run `npm run solvers`' }, () => {});
}

for (const id of ids) {
  test(`${id}: models, UNSAT, incremental use and cores`, async () => {
    const backend = await loadSolverModule(id, base);
    assert.match(backend.name, /\d/);
    const solver = backend.create();
    solver.add(clauses([1, 2], [-1, 2], [-2, 3]));
    assert.equal(solver.solve(new Int32Array()), 10);
    assert.ok(solver.value(2) && solver.value(3));
    assert.ok(!solver.value(-2));

    assert.equal(solver.solve(Int32Array.of(-3)), 20, 'assuming ¬3 contradicts 2 → 3');
    assert.ok(solver.failed(-3));
    assert.equal(solver.solve(new Int32Array()), 10, 'assumptions hold for one call only');

    solver.add(clauses([-3, 4], [-4]));
    assert.equal(solver.solve(new Int32Array()), 20, 'clauses accumulate');
    solver.release();
  });

  test(`${id}: the core names only the assumptions that matter`, async () => {
    const backend = await loadSolverModule(id, base);
    const solver = backend.create();
    solver.add(clauses([-1, -2], [3, 4]));
    assert.equal(solver.solve(Int32Array.of(1, 2, 5)), 20);
    assert.ok(solver.failed(1) && solver.failed(2));
    assert.ok(!solver.failed(5));
    solver.release();
  });

  test(`${id}: many instances, created and released`, async () => {
    const backend = await loadSolverModule(id, base);
    for (let round = 0; round < 200; round++) {
      const solver = backend.create();
      solver.add(clauses([round + 1], [-(round + 1), round + 2]));
      assert.equal(solver.solve(new Int32Array()), 10);
      assert.ok(solver.value(round + 2));
      solver.release();
    }
  });

  test(`${id}: a random 3-SAT instance with 2000 variables`, async () => {
    const backend = await loadSolverModule(id, base);
    const formula = random3Sat(2000, 7);
    const solver = backend.create();
    solver.add(clauses(...formula));
    const result = solver.solve(new Int32Array());
    assert.ok(result === 10 || result === 20);
    if (result === 10) {
      for (const clause of formula) assert.ok(clause.some((lit) => solver.value(lit)), 'the model satisfies every clause');
    }
    solver.release();
  });
}

for (const id of ids) {
  test(`${id}: a shared stop flag stops a long solve, and the instance stays usable`, async () => {
    const backend = await loadSolverModule(id, base);
    const flag = new Int32Array(new SharedArrayBuffer(4));
    backend.setStop(flag);
    // Pigeonhole 10 into 9: far too hard to finish here.
    const solver = backend.create({ seed: 3, phase: 0, shuffle: true, preset: 'unsat' });
    const pigeons = 10;
    const holes = 9;
    const v = (p, h) => p * holes + h + 1;
    const formula = [];
    for (let p = 0; p < pigeons; p++) formula.push([...Array(holes).keys()].map((h) => v(p, h)));
    for (let h = 0; h < holes; h++) {
      for (let p = 0; p < pigeons; p++) for (let q = p + 1; q < pigeons; q++) formula.push([-v(p, h), -v(q, h)]);
    }
    solver.add(clauses(...formula));
    Atomics.store(flag, 0, 1);
    assert.equal(solver.solve(new Int32Array()), 0, 'stopped: unknown');
    Atomics.store(flag, 0, 0);
    backend.setStop(null);
    solver.release();
    const easy = backend.create();
    easy.add(clauses([1], [-1, 2]));
    assert.equal(easy.solve(new Int32Array()), 10);
    easy.release();
  });
}
