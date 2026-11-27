// Runs the RedSAT compiler (Rust compiled to WebAssembly) off the main thread.
//
// Requests: {id, type: 'hello'} → {version, vocabulary};
//           {id, type: 'run', name, source, config, verbose, solver} → 'status', 'output' and
//             'trace' messages, then 'done' with the result;
//           {id, type: 'analyze', name, source, config} → 'analysis'.
// A run cannot be interrupted from inside: the page stops it by terminating this worker.

import init, { analyze, run, version, vocabulary } from '../pkg/redsat_wasm.js';
import { solverById } from './backends.js';

const ready = init({ module_or_path: new URL('./redsat_wasm_bg.wasm', import.meta.url) });

/** How long the editor's analysis may elaborate a program before giving up, in ms. */
const ANALYSIS_BUDGET_MS = 400;

/** Solver backends loaded so far, by id. */
const backends = new Map();

async function backend(id, post) {
  const solver = solverById(id);
  if (!solver.load) return null;
  if (!backends.has(id)) {
    post('status', `Loading ${solver.label}…`);
    backends.set(id, solver.load());
  }
  try {
    return await backends.get(id);
  } catch (error) {
    backends.delete(id);
    throw new Error(`could not load the ${solver.label} solver: ${error.message ?? error}`);
  }
}

self.onmessage = async ({ data: request }) => {
  const { id, type } = request;
  const post = (kind, text) => postMessage({ id, type: kind, text });
  try {
    await ready;
    switch (type) {
      case 'hello':
        postMessage({ id, type, version: version(), vocabulary: JSON.parse(vocabulary()) });
        break;
      case 'run': {
        const solver = await backend(request.solver, post);
        const onOutput = (text) => post('output', text);
        const onTrace = request.verbose ? (text) => post('trace', text) : undefined;
        const result = run(
          request.name,
          request.source,
          request.config,
          onOutput,
          onTrace,
          solver ? () => solver.create() : undefined,
          solver ? solver.name : undefined,
        );
        postMessage({ id, type: 'done', result: JSON.parse(result) });
        break;
      }
      case 'analyze': {
        const result = analyze(request.name, request.source, request.config, ANALYSIS_BUDGET_MS);
        postMessage({ id, type: 'analysis', result: JSON.parse(result) });
        break;
      }
      default:
        throw new Error(`unknown request ${type}`);
    }
  } catch (error) {
    // A trap (a Rust panic) leaves the WebAssembly instance unusable: the page replaces this
    // worker. Other errors (a bad configuration, a solver that failed to load) are ordinary.
    const fatal = error instanceof WebAssembly.RuntimeError;
    postMessage({ id, type: 'crash', fatal, message: String(error?.message ?? error) });
  }
};
