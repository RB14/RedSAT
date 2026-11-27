// A Web Worker running the RedSAT compiler. Replies are matched to requests by id; a run is
// stopped by terminating the worker and starting a fresh one.

export class Compiler {
  constructor() {
    this.nextId = 1;
    this.pending = new Map();
    this.spawn();
  }

  spawn() {
    this.worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
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

  /** Kills the worker (and whatever it is running) and starts a fresh one. */
  restart(reason = 'stopped') {
    this.worker.terminate();
    this.failAll(reason);
    this.spawn();
  }
}
