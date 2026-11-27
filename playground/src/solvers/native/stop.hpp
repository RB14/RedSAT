// Asking the page whether to stop the current solve. ../emscripten.js sets Module.shouldStop to
// a function that reads a flag in a SharedArrayBuffer (when the page is cross-origin isolated),
// so the page can stop a solve without terminating the worker.

#pragma once

#include <emscripten/emscripten.h>

EM_JS(int, glue_should_stop, (), { return Module['shouldStop'] ? Module['shouldStop']() : 0; });

/// Options a solver module understands, by number (see solver_option in glue.hpp).
enum Option { kSeed = 1, kPhase = 2, kShuffle = 3, kPreset = 4 };
