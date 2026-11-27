// What the playground remembers in this browser: your own files, your edits to the examples,
// the open document, settings, and whether you have been here before.

const KEYS = {
  files: 'redsat.files',
  edits: 'redsat.edits',
  current: 'redsat.current',
  visited: 'redsat.visited',
  settings: 'redsat.settings',
  split: 'redsat.split',
};

function load(key, fallback) {
  try {
    return JSON.parse(localStorage.getItem(key)) ?? fallback;
  } catch {
    return fallback;
  }
}

function save(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage full or disabled: the playground still works, it just forgets.
  }
}

/** The starting point of a new file. */
export const NEW_FILE_TEMPLATE = `// A new RedSAT program.
//
// Declare unknowns with \`var\`, state what must hold with \`require\`, and ask for values
// with \`solve\`. Run it with Ctrl+Enter.

var x, y: int[1..=20]

require x + y == 20
require x * y == 96

solve all x, y
`;

export class Store {
  constructor() {
    /** @type {{id: string, name: string, source: string, created: number, updated: number}[]} */
    this.files = load(KEYS.files, []);
    /** Your edits to the examples, by example name. */
    this.edits = load(KEYS.edits, {});
    this.settings = { verbose: false, strict: false, solver: 'builtin', ...load(KEYS.settings, {}) };
  }

  // --- your files ---

  file(id) {
    return this.files.find((file) => file.id === id) ?? null;
  }

  /** A name not used by another file: `untitled`, `untitled-2`, … */
  freeName(base = 'untitled') {
    const taken = new Set(this.files.map((file) => file.name));
    if (!taken.has(base)) return base;
    for (let n = 2; ; n++) if (!taken.has(`${base}-${n}`)) return `${base}-${n}`;
  }

  createFile(name, source = NEW_FILE_TEMPLATE) {
    const now = Date.now();
    const file = { id: `f${now.toString(36)}${Math.random().toString(36).slice(2, 6)}`, name: this.freeName(name), source, created: now, updated: now };
    this.files.unshift(file);
    save(KEYS.files, this.files);
    return file;
  }

  updateFile(id, changes) {
    const file = this.file(id);
    if (!file) return;
    Object.assign(file, changes, { updated: Date.now() });
    save(KEYS.files, this.files);
  }

  deleteFile(id) {
    this.files = this.files.filter((file) => file.id !== id);
    save(KEYS.files, this.files);
  }

  // --- the examples ---

  exampleSource(example) {
    return this.edits[example.name] ?? example.source;
  }

  setExampleSource(example, source) {
    if (source === example.source) delete this.edits[example.name];
    else this.edits[example.name] = source;
    save(KEYS.edits, this.edits);
  }

  // --- session ---

  get current() {
    return load(KEYS.current, null);
  }

  set current(reference) {
    save(KEYS.current, reference);
  }

  get visited() {
    return load(KEYS.visited, false);
  }

  markVisited() {
    save(KEYS.visited, true);
  }

  saveSettings() {
    save(KEYS.settings, this.settings);
  }

  get split() {
    return load(KEYS.split, null);
  }

  set split(percent) {
    save(KEYS.split, percent);
  }
}
