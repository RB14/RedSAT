// Runs the RedSAT compiler (Rust compiled to WebAssembly) off the main thread.
//
// Requests: {id, type: 'hello'} → {version, vocabulary};
//           {id, type: 'run', name, source, config, verbose} → 'output'/'trace' messages, then
//             'done' with the result;
//           {id, type: 'analyze', name, source, config} → 'analysis'.
// A run cannot be interrupted from inside: the page stops it by terminating this worker.

import init, { analyze, run, version, vocabulary } from '../pkg/redsat_wasm.js';

const ready = init({ module_or_path: new URL('./redsat_wasm_bg.wasm', import.meta.url) });

/** How long the editor's analysis may elaborate a program before giving up, in ms. */
const ANALYSIS_BUDGET_MS = 400;

self.onmessage = async ({ data: request }) => {
  const { id, type } = request;
  try {
    await ready;
    switch (type) {
      case 'hello':
        postMessage({ id, type, version: version(), vocabulary: JSON.parse(vocabulary()) });
        break;
      case 'run': {
        const onOutput = (text) => postMessage({ id, type: 'output', text });
        const onTrace = request.verbose ? (text) => postMessage({ id, type: 'trace', text }) : undefined;
        const result = run(request.name, request.source, request.config, onOutput, onTrace);
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
    // A Rust panic leaves the WebAssembly instance unusable; the page replaces this worker.
    postMessage({ id, type: 'crash', message: String(error?.message ?? error) });
  }
};
