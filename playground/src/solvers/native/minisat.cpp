// MiniSat (https://github.com/niklasso/minisat, MIT) behind the solver module ABI of glue.hpp.
// Uses the core solver, not SimpSolver: variable elimination would need every variable that
// may appear in a later clause or assumption frozen, which an incremental caller cannot know.
// Compiled with the library sources by scripts/build-solvers.mjs.

#include <cstdlib>

#include "minisat/core/Solver.h"
#include "stop.hpp"

/// Conflicts per slice of a solve: between slices the page is asked whether to stop.
constexpr int kSlice = 10000;

struct Engine {
  Minisat::Solver solver;
  Minisat::vec<Minisat::Lit> clause;
  Minisat::vec<Minisat::Lit> assumptions;

  /// DIMACS variable v is MiniSat variable v - 1, created on first use.
  Minisat::Lit lit(int dimacs) {
    const int var = std::abs(dimacs);
    while (solver.nVars() < var) solver.newVar();
    return Minisat::mkLit(var - 1, dimacs < 0);
  }

  void add(int dimacs) {
    if (dimacs != 0) {
      clause.push(lit(dimacs));
    } else {
      solver.addClause_(clause);  // false once the formula is UNSAT; solve() then says so
      clause.clear();
    }
  }

  void assume(int dimacs) { assumptions.push(lit(dimacs)); }

  int solve() {
    // MiniSat has no terminator callback: solve in slices of conflicts (learnt clauses carry
    // over) and ask the page between them.
    Minisat::lbool result = Minisat::l_Undef;
    while (result == Minisat::l_Undef && !glue_should_stop()) {
      solver.setConfBudget(kSlice);
      result = solver.solveLimited(assumptions);
    }
    solver.budgetOff();
    assumptions.clear();
    return result == Minisat::l_True ? 10 : result == Minisat::l_False ? 20 : 0;
  }

  /// Diversification for portfolios: a seed randomises the initial activities.
  void option(int option, int value) {
    if (option == kSeed) {
      solver.random_seed = 91648253.0 + value;
      solver.rnd_init_act = true;
    }
  }

  int vars() { return solver.model.size(); }

  int value(int var) {
    const Minisat::lbool value = solver.model[var - 1];
    return value == Minisat::l_True ? 1 : value == Minisat::l_False ? -1 : 0;
  }

  /// `conflict` is the final conflict clause over the negated failed assumptions.
  bool failed(int dimacs) { return solver.conflict.has(~lit(dimacs)); }
};

#include "glue.hpp"
