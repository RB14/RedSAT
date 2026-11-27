// The RedSAT playground: pick an example or write a program, run it in the browser.

import { createEditor } from './editor.js';
import { lexicon } from './rsat-language.js';
import { decodeShare, encodeShare } from './share.js';

const $ = (id) => document.getElementById(id);

const STORAGE = {
  example: 'redsat.example',
  edits: 'redsat.edits',
  verbose: 'redsat.verbose',
  strict: 'redsat.strict',
  split: 'redsat.split',
};
/** Output lines kept on screen; a `solve all` can print far more. */
const MAX_LINES = 5000;
const SHARED = 'shared';

/** A worker running the compiler. Replies are matched to requests by id. */
class Compiler {
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
      this.pending.delete(data.id);
      if (data.type === 'crash') {
        entry.reject(new Error(data.message));
        this.restart('the compiler crashed');
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

// --- state ----------------------------------------------------------------------------------

const runner = new Compiler();
const analyzer = new Compiler();
let examples = [];
let current = null; // the example (or shared program) in the editor
let editor = null;
let running = false;
const edits = JSON.parse(localStorage.getItem(STORAGE.edits) ?? '{}');

// --- output panel ---------------------------------------------------------------------------

const output = { queue: [], lines: 0, dropped: 0, scheduled: false };

function clearOutput() {
  $('output').replaceChildren();
  output.queue = [];
  output.lines = 0;
  output.dropped = 0;
}

function emit(kind, text) {
  output.queue.push([kind, text]);
  if (!output.scheduled) {
    output.scheduled = true;
    requestAnimationFrame(flushOutput);
  }
}

function flushOutput() {
  output.scheduled = false;
  const panel = $('output');
  const atBottom = panel.scrollHeight - panel.scrollTop - panel.clientHeight < 40;
  const fragment = document.createDocumentFragment();
  for (const [kind, text] of output.queue) {
    const lines = (text.match(/\n/g) ?? []).length || 1;
    if (output.lines + lines > MAX_LINES && kind === 'out') {
      output.dropped += lines;
      continue;
    }
    output.lines += lines;
    fragment.append(Object.assign(document.createElement('span'), { className: kind, textContent: text }));
  }
  output.queue = [];
  panel.append(fragment);
  if (atBottom) panel.scrollTop = panel.scrollHeight;
}

function emitDiagnostic(diagnostic) {
  flushOutput();
  const block = document.createElement('div');
  block.className = `diagnostic ${diagnostic.severity}`;
  block.textContent = diagnostic.rendered;
  block.title = 'Show in the editor';
  block.addEventListener('click', () => editor.reveal(diagnostic.from, diagnostic.to));
  $('output').append(block);
}

// --- status bar -----------------------------------------------------------------------------

function formatTime(ms) {
  return ms < 1000 ? `${ms.toFixed(ms < 10 ? 1 : 0)} ms` : `${(ms / 1000).toFixed(2)} s`;
}

function setStatus(kind, text) {
  const status = $('status');
  status.className = `status-text ${kind}`;
  status.textContent = text;
}

function plural(count, word) {
  return `${count.toLocaleString()} ${word}${count === 1 ? '' : 's'}`;
}

function showStats(result) {
  const solves = result.solves;
  if (!solves.length) {
    $('stats').textContent = '';
    return;
  }
  const sum = (key) => solves.reduce((total, solve) => total + solve[key], 0);
  $('stats').textContent = [
    plural(solves.length, 'solve'),
    `${plural(Math.max(...solves.map((s) => s.decisionBits)), 'decision bit')}`,
    `${plural(sum('cnfVars'), 'variable')}`,
    `${plural(sum('cnfClauses'), 'clause')}`,
    `${plural(sum('satCalls'), 'SAT call')}`,
  ].join(' · ');
}

// --- running --------------------------------------------------------------------------------

function config() {
  return $('strict').checked ? '[compiler]\nstrict = true\n' : '';
}

function setRunning(value) {
  running = value;
  $('run').hidden = value;
  $('stop').hidden = !value;
  document.body.classList.toggle('running', value);
}

async function runProgram() {
  if (running || !editor) return;
  clearOutput();
  $('stats').textContent = '';
  setRunning(true);
  const started = performance.now();
  const tick = () => setStatus('running', `Running… ${formatTime(performance.now() - started)}`);
  tick();
  const timer = setInterval(tick, 100);
  try {
    const { result } = await runner.request(
      {
        type: 'run',
        name: current?.file ?? 'program.rsat',
        source: editor.text,
        config: config(),
        verbose: $('verbose').checked,
      },
      { onOutput: (text) => emit('out', text), onTrace: (text) => emit('trace', `${text}\n`) },
    );
    flushOutput();
    for (const diagnostic of result.diagnostics) emitDiagnostic(diagnostic);
    editor.showDiagnostics(result.diagnostics);
    if (output.dropped) emit('system', `… ${plural(output.dropped, 'more line')} not shown\n`);
    if (!$('output').childElementCount && !output.queue.length) emit('system', '(the program printed nothing)\n');
    const elapsed = formatTime(performance.now() - started);
    const errors = result.diagnostics.filter((d) => d.severity === 'error').length;
    if (result.ok) setStatus('ok', `✓ Finished in ${elapsed}`);
    else setStatus('error', `✗ ${plural(errors, 'error')} · ${elapsed}`);
    showStats(result);
  } catch (error) {
    flushOutput();
    const elapsed = formatTime(performance.now() - started);
    if (error.message === 'stopped') {
      emit('system', `\n⏹ Stopped after ${elapsed}.\n`);
      setStatus('stopped', `⏹ Stopped after ${elapsed}`);
    } else {
      emit('crash', `Internal compiler error: ${error.message}\nPlease report this program!\n`);
      setStatus('error', '✗ The compiler crashed');
    }
  } finally {
    clearInterval(timer);
    setRunning(false);
  }
}

function stopProgram() {
  if (running) runner.restart();
}

/** Diagnostics and inferred domains for the editor (the latest request wins). */
let analysisGeneration = 0;
async function analyze(source) {
  const generation = ++analysisGeneration;
  try {
    const { result } = await analyzer.request({ type: 'analyze', name: current?.file ?? 'program.rsat', source, config: config() });
    return generation === analysisGeneration ? result : null;
  } catch {
    return null;
  }
}

// --- examples -------------------------------------------------------------------------------

function saveEdits() {
  localStorage.setItem(STORAGE.edits, JSON.stringify(edits));
}

function showExampleInfo() {
  $('example-title').textContent = current.title;
  $('example-description').textContent = current.description;
  $('slow-note').hidden = !current.slow;
  $('slow-note').textContent = current.slow ? `⏳ ${current.slow}` : '';
  $('reset').hidden = current.name === SHARED || editor.text === current.source;
}

function selectExample(name, { keepEdits = true } = {}) {
  const example = name === SHARED ? current : examples.find((e) => e.name === name) ?? examples[0];
  current = example;
  $('examples').value = example.name;
  if (example.name !== SHARED) localStorage.setItem(STORAGE.example, example.name);
  editor.text = (keepEdits && edits[example.name]) || example.source;
  showExampleInfo();
  clearOutput();
  $('output').append(placeholder());
  setStatus('idle', 'Ready');
  $('stats').textContent = '';
}

function placeholder() {
  const div = document.createElement('div');
  div.className = 'placeholder';
  div.innerHTML = `
    <p><kbd>Ctrl</kbd> + <kbd>Enter</kbd> or <b>Run</b> compiles the program to SAT, solves it and prints the result.</p>
    <p>Everything runs in your browser: the RedSAT compiler and its SAT solver are Rust, compiled to WebAssembly.</p>
    <p>Hover a variable to see the domain the compiler inferred for it.</p>`;
  return div;
}

function fillExamplePicker() {
  const select = $('examples');
  select.replaceChildren();
  if (current?.name === SHARED) select.append(new Option('Shared program', SHARED));
  for (const example of examples) {
    select.append(new Option(`${example.title}${example.slow ? '  ⏳' : ''}`, example.name));
  }
}

function onEdit(text) {
  if (!current) return;
  if (current.name !== SHARED) {
    if (text === current.source) delete edits[current.name];
    else edits[current.name] = text;
    saveEdits();
  }
  $('reset').hidden = current.name === SHARED || text === current.source;
}

// --- sharing --------------------------------------------------------------------------------

function toast(message) {
  const element = $('toast');
  element.textContent = message;
  element.classList.add('visible');
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => element.classList.remove('visible'), 2200);
}

async function share() {
  const hash = await encodeShare(editor.text);
  const url = `${location.origin}${location.pathname}${hash}`;
  history.replaceState(null, '', hash);
  try {
    await navigator.clipboard.writeText(url);
    toast('Link copied to the clipboard ✓');
  } catch {
    toast('Link ready in the address bar');
  }
}

// --- layout ---------------------------------------------------------------------------------

function setupDivider() {
  const workspace = $('workspace');
  const divider = $('divider');
  const saved = Number(localStorage.getItem(STORAGE.split));
  if (saved) workspace.style.setProperty('--split', `${saved}%`);
  const move = (event) => {
    const rect = workspace.getBoundingClientRect();
    const vertical = getComputedStyle(workspace).gridTemplateRows.split(' ').length > 1 && rect.width < 900;
    const ratio = vertical ? (event.clientY - rect.top) / rect.height : (event.clientX - rect.left) / rect.width;
    const percent = Math.min(80, Math.max(20, ratio * 100));
    workspace.style.setProperty('--split', `${percent}%`);
    localStorage.setItem(STORAGE.split, percent.toFixed(1));
  };
  divider.addEventListener('pointerdown', (event) => {
    divider.setPointerCapture(event.pointerId);
    document.body.classList.add('dragging');
    divider.addEventListener('pointermove', move);
  });
  divider.addEventListener('pointerup', (event) => {
    divider.releasePointerCapture(event.pointerId);
    document.body.classList.remove('dragging');
    divider.removeEventListener('pointermove', move);
  });
}

function setupToggles() {
  for (const id of ['verbose', 'strict']) {
    const box = $(id);
    box.checked = localStorage.getItem(STORAGE[id]) === 'true';
    box.addEventListener('change', () => {
      localStorage.setItem(STORAGE[id], String(box.checked));
      if (id === 'strict') editor?.view.dispatch({}); // re-lint with the new options
    });
  }
}

// --- start-up -------------------------------------------------------------------------------

async function start() {
  setupToggles();
  setupDivider();
  $('run').addEventListener('click', runProgram);
  $('stop').addEventListener('click', stopProgram);
  $('share').addEventListener('click', share);
  $('reset').addEventListener('click', () => {
    delete edits[current.name];
    saveEdits();
    selectExample(current.name, { keepEdits: false });
  });
  $('examples').addEventListener('change', (event) => selectExample(event.target.value));
  document.addEventListener('keydown', (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
      event.preventDefault();
      runProgram();
    } else if (event.key === 'Escape' && running) {
      stopProgram();
    }
  });

  setStatus('running', 'Loading the compiler…');
  let hello;
  try {
    [hello, examples] = await Promise.all([
      runner.request({ type: 'hello' }),
      fetch('examples.json').then((response) => response.json()),
    ]);
  } catch (error) {
    setStatus('error', `✗ Could not load the compiler: ${error.message}`);
    return;
  }
  $('version').textContent = `RedSAT ${hello.version}`;

  const shared = await decodeShare(location.hash);
  if (shared !== null) current = sharedProgram(shared);
  fillExamplePicker();

  editor = createEditor({
    parent: $('editor'),
    words: lexicon(hello.vocabulary),
    doc: '',
    analyze,
    onRun: runProgram,
    onChange: onEdit,
  });
  selectExample(shared !== null ? SHARED : localStorage.getItem(STORAGE.example) ?? examples[0].name);
  editor.focus();

  // A share link opened in a tab that already shows the playground.
  window.addEventListener('hashchange', async () => {
    const source = await decodeShare(location.hash);
    if (source === null || source === editor.text) return;
    current = sharedProgram(source);
    fillExamplePicker();
    selectExample(SHARED);
  });
}

function sharedProgram(source) {
  return {
    name: SHARED,
    file: 'shared.rsat',
    title: 'Shared program',
    description: 'Opened from a link. Edit and run it, or pick an example.',
    slow: null,
    source,
  };
}

start();
