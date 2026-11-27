// The C ABI that every solver module built by scripts/build-solvers.mjs exports, and that
// ../emscripten.js calls. A solver's .cpp file defines `struct Engine` and then includes this
// header; the build defines SOLVER_NAME, the display name (e.g. "CaDiCaL 2.2.1").
//
//   struct Engine {
//     void add(int lit);          // one DIMACS literal; 0 ends the clause
//     void assume(int lit);       // an assumption for the next solve() only
//     int solve();                // 10 = SAT, 20 = UNSAT, 0 = unknown
//     int vars();                 // after SAT: the highest variable with a value
//     int value(int var);         // after SAT: > 0 if var is true, < 0 if false, 0 if unknown
//     bool failed(int lit);       // after UNSAT: whether assumption lit is in the core
//   };
//
// Literals cross the boundary in bulk: JavaScript writes a whole clause or assumption list
// into a shared scratch buffer (solver_buffer) and makes one call, instead of one call per
// literal. The glue snapshots the model after SAT and the failed assumptions after UNSAT, so
// value() and failed() always answer about the last solve() (false when it had another
// result) and never trip over a solver's own state checks, which would abort the module.

#include <algorithm>
#include <vector>

#include <emscripten/emscripten.h>

namespace glue {

/// One solver instance and what its last solve() found.
struct Instance {
  Engine engine;
  /// Indexed by variable after SAT: 1 = true, -1 = false, 0 = unknown (or no model).
  std::vector<signed char> model;
  /// The failed assumptions of the last UNSAT, sorted.
  std::vector<int> core;
};

/// Where JavaScript puts the literals of the next solver_add() or solver_solve() call.
std::vector<int> scratch;

} // namespace glue

extern "C" {

/// The solver's display name, a NUL-terminated UTF-8 string.
EMSCRIPTEN_KEEPALIVE const char *solver_name() { return SOLVER_NAME; }

/// Room for `size` literals, valid until the next call into the module.
EMSCRIPTEN_KEEPALIVE int *solver_buffer(int size) {
  if (glue::scratch.size() < static_cast<size_t>(size)) {
    glue::scratch.clear();
    glue::scratch.shrink_to_fit();
    glue::scratch.resize(size);
  }
  return glue::scratch.data();
}

EMSCRIPTEN_KEEPALIVE glue::Instance *solver_new() { return new glue::Instance; }

EMSCRIPTEN_KEEPALIVE void solver_release(glue::Instance *instance) { delete instance; }

/// Adds the first `size` literals of the buffer: zero-terminated clauses.
EMSCRIPTEN_KEEPALIVE void solver_add(glue::Instance *instance, int size) {
  const int *lits = glue::scratch.data();
  for (int i = 0; i < size; i++) instance->engine.add(lits[i]);
}

/// Solves under the first `size` literals of the buffer as assumptions.
EMSCRIPTEN_KEEPALIVE int solver_solve(glue::Instance *instance, int size) {
  // Copied: the buffer belongs to JavaScript again once this call returns.
  const std::vector<int> assumptions(glue::scratch.begin(), glue::scratch.begin() + size);
  instance->model.clear();
  instance->core.clear();
  for (int lit : assumptions) instance->engine.assume(lit);
  const int result = instance->engine.solve();
  if (result == 10) {
    const int vars = instance->engine.vars();
    instance->model.assign(vars + 1, 0);
    for (int var = 1; var <= vars; var++) {
      const int value = instance->engine.value(var);
      instance->model[var] = value > 0 ? 1 : value < 0 ? -1 : 0;
    }
  } else if (result == 20) {
    for (int lit : assumptions) {
      if (instance->engine.failed(lit)) instance->core.push_back(lit);
    }
    std::sort(instance->core.begin(), instance->core.end());
    instance->core.erase(std::unique(instance->core.begin(), instance->core.end()),
                         instance->core.end());
  }
  return result;
}

/// Whether `lit` is true in the last model; false without one or for unknown variables.
EMSCRIPTEN_KEEPALIVE int solver_value(glue::Instance *instance, int lit) {
  const size_t var = lit < 0 ? -static_cast<long long>(lit) : lit;
  if (lit == 0 || var >= instance->model.size()) return 0;
  const int value = instance->model[var];
  return value != 0 && (value > 0) == (lit > 0);
}

/// Whether the assumption `lit` is in the unsat core of the last solve().
EMSCRIPTEN_KEEPALIVE int solver_failed(glue::Instance *instance, int lit) {
  return std::binary_search(instance->core.begin(), instance->core.end(), lit);
}

} // extern "C"
