// CaDiCaL (https://github.com/arminbiere/cadical, MIT) behind the solver module ABI of
// glue.hpp, through its C++ API. Compiled with the library sources by
// scripts/build-solvers.mjs.

#include "cadical.hpp"
#include "stop.hpp"

/// Polled by CaDiCaL while it searches.
struct Stop : CaDiCaL::Terminator {
  bool terminate() override { return glue_should_stop() != 0; }
};

struct Engine {
  CaDiCaL::Solver solver;
  Stop stop;

  Engine() { solver.connect_terminator(&stop); }
  Engine(const Engine &) = delete;
  Engine &operator=(const Engine &) = delete;
  ~Engine() { solver.disconnect_terminator(); }

  void add(int lit) { solver.add(lit); }
  void assume(int lit) { solver.assume(lit); }
  int solve() { return solver.solve(); }
  int vars() { return solver.vars(); }
  int value(int var) { return solver.val(var); }
  bool failed(int lit) { return solver.failed(lit); }

  /// Diversification for portfolios; call before adding clauses.
  void option(int option, int value) {
    switch (option) {
      case kSeed: solver.set("seed", value); break;
      case kPhase: solver.set("phase", value); break;
      case kShuffle:
        solver.set("shuffle", value);
        solver.set("shufflerandom", value);
        break;
      case kPreset: solver.configure(value == 1 ? "sat" : value == 2 ? "unsat" : "default"); break;
      default: break;
    }
  }
};

#include "glue.hpp"
