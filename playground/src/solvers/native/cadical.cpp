// CaDiCaL (https://github.com/arminbiere/cadical, MIT) behind the solver module ABI of
// glue.hpp, through its C++ API. Compiled with the library sources by
// scripts/build-solvers.mjs.

#include "cadical.hpp"

struct Engine {
  CaDiCaL::Solver solver;

  void add(int lit) { solver.add(lit); }
  void assume(int lit) { solver.assume(lit); }
  int solve() { return solver.solve(); }
  int vars() { return solver.vars(); }
  int value(int var) { return solver.val(var); }
  bool failed(int lit) { return solver.failed(lit); }
};

#include "glue.hpp"
