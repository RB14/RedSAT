// A Web Worker running the RedSAT compiler. Replies are matched to requests by id.
//
// Stopping a run: when the page is cross-origin isolated, a flag in a SharedArrayBuffer asks the
// worker to stop (it checks between statements and inside the SAT solvers), which keeps its
// compiler and solvers loaded. Otherwise, or if it does not stop in time, the worker is
// terminated and a fresh one started.

/** How long a run may take to stop when asked before its worker is terminated. */
const STOP_GRACE_MS = 1500;

export class Compiler {
  constructor() {
    this.nextId = 1;
    this.pending = new Map();
    this.spawn();
  }

  spawn() {
    this.worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
    this.stopFlag = globalThis.crossOriginIsolated ? new Int32Array(new SharedArrayBuffer(4)) : null;
    if (this.stopFlag) this.worker.postMessage({ id: 0, type: 'init', stop: this.stopFlag });
    this.worker.onmessage = ({ data }) => {
      const entry = this.pending.get(data.id);
      if (!entry) return;
      if (data.type === 'output') return entry.onOutput?.(data.text);
      if (data.type === 'trace') return entry.onTrace?.(data.text);
      if (data.type === 'status') return entry.onStatus?.(data.text);
      this.pending.delete(data.id);
      if (data.type === 'crash') {
        entry.reject(new Error(data.message));
        if (data.fatal) this.restart('the compiler crashed');
      } else {
        entry.resolve(data);
      }
    };
    this.worker.onerror = (event) => {
      event.preventDefault();
      this.failAll(event.message || 'the compiler could not be loaded');
    };
  }

  request(message, handlers = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject, ...handlers });
      this.worker.postMessage({ id, ...message });
    });
  }

  failAll(reason) {
    for (const entry of this.pending.values()) entry.reject(new Error(reason));
    this.pending.clear();
  }

  /** Whether a request is in progress. */
  get busy() {
    return this.pending.size > 0;
  }

  /**
   * Stops whatever the worker is running: politely if it can (the request then fails with
   * "stopped"), else by restarting the worker.
   */
  stop() {
    if (!this.busy) return;
    if (!this.stopFlag) {
      this.restart();
      return;
    }
    Atomics.store(this.stopFlag, 0, 1);
    const worker = this.worker;
    setTimeout(() => {
      if (this.worker === worker && this.busy) this.restart();
    }, STOP_GRACE_MS);
  }

  /** Kills the worker (and whatever it is running) and starts a fresh one. */
  restart(reason = 'stopped') {
    this.worker.terminate();
    this.failAll(reason);
    this.spawn();
  }
}
