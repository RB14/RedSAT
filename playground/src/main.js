// The RedSAT playground: write a program or open an example, and run it in the browser.

import { RACE, SOLVERS, solverById } from './backends.js';
import { Compiler } from './compiler.js';
import { createEditor } from './editor.js';
import { renderHome } from './home.js';
import { setupSwitcher } from './switcher.js';
import { markOutput, setupMobile, showTab } from './mobile.js';
import { lexicon } from './rsat-language.js';
import { decodeShare, encodeShare } from './share.js';
import { Store } from './store.js';

const $ = (id) => document.getElementById(id);

/** Output lines kept on screen; a `solve all` can print far more. */
const MAX_LINES = 5000;

const store = new Store();
/** Workers that run programs: the first runs single-solver programs; a race uses one each. */
const runners = [new Compiler()];
const runner = (index) => (runners[index] ??= new Compiler());
const analyzer = new Compiler();
const phone = matchMedia('(max-width: 760px)');

let examples = [];
let editor = null;
/** The open document: {kind: 'example'|'file'|'shared', id, title, description, slow, fileName}. */
let doc = null;
let sharedSource = '';
let running = false;

// --- documents ------------------------------------------------------------------------------

function exampleDoc(example) {
  return { kind: 'example', id: example.name, title: example.title, description: example.description, slow: example.slow, fileName: example.file };
}

function fileDoc(file) {
  return { kind: 'file', id: file.id, title: file.name, description: 'Your file, saved in this browser.', slow: null, fileName: `${file.name}.rsat` };
}

function sharedDoc() {
  return { kind: 'shared', id: 'shared', title: 'Shared program', description: 'Opened from a link. Save a copy to keep your changes.', slow: null, fileName: 'shared.rsat' };
}

const exampleNamed = (name) => examples.find((example) => example.name === name);

function sourceOf(document) {
  if (document.kind === 'example') return store.exampleSource(exampleNamed(document.id));
  if (document.kind === 'file') return store.file(document.id)?.source ?? '';
  return sharedSource;
}

function open(document) {
  doc = document;
  if (document.kind !== 'shared') {
    store.current = { kind: document.kind, id: document.id };
    if (location.hash) history.replaceState(null, '', location.pathname + location.search);
  }
  store.markVisited();
  editor.load(sourceOf(document));
  renderDocument();
  resetOutput();
  showView('editor');
  showTab('code');
  if (!phone.matches) editor.focus();
}

function onEdit(text) {
  if (!doc) return;
  if (doc.kind === 'example') store.setExampleSource(exampleNamed(doc.id), text);
  else if (doc.kind === 'file') store.updateFile(doc.id, { source: text });
  else sharedSource = text;
  renderDocumentActions();
}

function renderDocument() {
  const title = $('doc-title');
  title.value = doc.title;
  title.readOnly = doc.kind !== 'file';
  title.title = doc.kind === 'file' ? 'Rename' : '';
  $('doc-description').textContent = doc.description;
  $('file-name').textContent = doc.kind === 'file' ? `${doc.title}.rsat` : doc.title;
  document.title = `${doc.kind === 'file' ? `${doc.title}.rsat` : doc.title} · RedSAT Playground`;
  renderSlowNote();
  renderDocumentActions();
}

function renderDocumentActions() {
  const modified = doc.kind === 'example' && editor.text !== exampleNamed(doc.id).source;
  $('reset').hidden = !modified;
  $('save-copy').hidden = doc.kind === 'file';
  $('delete-file').hidden = doc.kind !== 'file';
}

function renderSlowNote() {
  const note = $('slow-note');
  const show = Boolean(doc?.slow) && ['builtin', 'minisat'].includes(store.settings.solver);
  note.hidden = !show;
  if (!show) return;
  $('slow-text').textContent = `⏳ ${doc.slow}`;
  $('use-cadical').hidden = !SOLVERS.some((solver) => solver.id === 'cadical');
}

function newFile() {
  const file = store.createFile('untitled');
  open(fileDoc(file));
  if (!phone.matches) {
    $('doc-title').focus();
    $('doc-title').select();
  }
}

function renameFile() {
  if (doc?.kind !== 'file') return;
  const name = $('doc-title').value.trim().replace(/\.rsat$/, '').replace(/[^\w.-]+/g, '-') || 'untitled';
  const unique = name === doc.title ? name : store.freeName(name);
  store.updateFile(doc.id, { name: unique });
  doc = fileDoc(store.file(doc.id));
  renderDocument();
}

function saveCopy() {
  const base = doc.kind === 'example' ? doc.id : 'shared';
  const file = store.createFile(base, editor.text);
  open(fileDoc(file));
  toast(`Saved as ${file.name}.rsat`);
}

function deleteFile(id) {
  const file = store.file(id);
  if (!file || !confirm(`Delete ${file.name}.rsat? This cannot be undone.`)) return;
  store.deleteFile(id);
  if (doc?.kind === 'file' && doc.id === id) {
    doc = null;
    store.current = null;
    showView('home');
  } else if (document.body.dataset.view === 'home') {
    showView('home');
  }
}

function resetExample() {
  const example = exampleNamed(doc.id);
  store.setExampleSource(example, example.source);
  editor.load(example.source);
  renderDocumentActions();
}

// --- views ----------------------------------------------------------------------------------

function showView(view) {
  if (view === 'editor' && !doc) view = 'home';
  document.body.dataset.view = view;
  $('home').hidden = view !== 'home';
  if (view === 'home') {
    renderHome(
      $('home'),
      { examples, files: store.files, current: doc && { kind: doc.kind, id: doc.id }, firstVisit: !store.visited },
      {
        newFile,
        openExample: (name) => open(exampleDoc(exampleNamed(name))),
        openFile: (id) => open(fileDoc(store.file(id))),
        deleteFile,
        close: () => showView('editor'),
      },
    );
    $('home').scrollTop = 0;
  }
}

// --- output panel ---------------------------------------------------------------------------

const output = { queue: [], lines: 0, dropped: 0, scheduled: false };

function resetOutput() {
  clearOutput();
  $('output').append(placeholder());
  setStatus('idle', 'Ready');
  $('stats').textContent = '';
}

function clearOutput() {
  $('output').replaceChildren();
  output.queue = [];
  output.lines = 0;
  output.dropped = 0;
}

function placeholder() {
  const div = document.createElement('div');
  div.className = 'placeholder';
  div.innerHTML = phone.matches
    ? `<p>Tap <b>Run</b> to compile the program to SAT, solve it and print the result.</p>
       <p>Everything runs on your device: the RedSAT compiler and its SAT solvers are compiled to WebAssembly.</p>`
    : `<p><kbd>Ctrl</kbd> + <kbd>Enter</kbd> or <b>Run</b> compiles the program to SAT, solves it and prints the result.</p>
       <p>Everything runs in your browser: the RedSAT compiler and its SAT solvers are compiled to WebAssembly.</p>
       <p>Hover a name for its type or inferred domain, <kbd>Ctrl</kbd>+click to jump to its declaration.</p>`;
  return div;
}

function emit(kind, text) {
  output.queue.push([kind, text]);
  markOutput();
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
  const block = document.createElement('button');
  block.className = `diagnostic ${diagnostic.severity}`;
  block.textContent = diagnostic.rendered;
  block.title = 'Show in the editor';
  block.addEventListener('click', () => {
    showTab('code');
    editor.reveal(diagnostic.from, diagnostic.to);
  });
  $('output').append(block);
}

// --- status bar -----------------------------------------------------------------------------

function formatTime(ms) {
  return ms < 1000 ? `${ms.toFixed(ms < 10 ? 1 : 0)} ms` : `${(ms / 1000).toFixed(2)} s`;
}

function plural(count, word) {
  return `${count.toLocaleString()} ${word}${count === 1 ? '' : 's'}`;
}

function setStatus(kind, text) {
  const status = $('status');
  status.className = `status-text ${kind}`;
  status.textContent = text;
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
    plural(Math.max(...solves.map((solve) => solve.decisionBits)), 'decision bit'),
    plural(sum('cnfVars'), 'variable'),
    plural(sum('cnfClauses'), 'clause'),
    plural(sum('satCalls'), 'SAT call'),
  ].join(' · ');
}

// --- running --------------------------------------------------------------------------------

function config() {
  return store.settings.strict ? '[compiler]\nstrict = true\n' : '';
}

function setRunning(value) {
  running = value;
  document.body.classList.toggle('running', value);
}

/**
 * Runs the program with every solver at once, one worker each. The first to finish wins: its
 * output is shown and the others are stopped. Resolves to {result, solver}.
 */
function race(request, onStatus) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const failures = [];
    SOLVERS.forEach((solver, index) => {
      const output = [];
      runner(index)
        .request(
          { ...request, solver: solver.id },
          {
            onOutput: (text) => output.push(['out', text]),
            onTrace: (text) => output.push(['trace', `${text}\n`]),
            onStatus: (text) => onStatus(`${solver.label}: ${text}`),
          },
        )
        .then(({ result }) => {
          if (settled) return;
          settled = true;
          SOLVERS.forEach((_, other) => other !== index && runner(other).restart('lost the race'));
          for (const [kind, text] of output) emit(kind, text);
          resolve({ result, solver });
        })
        .catch((error) => {
          if (settled || error.message === 'lost the race') return;
          failures.push(error);
          if (failures.length === SOLVERS.length) {
            settled = true;
            reject(failures[0]);
          }
        });
    });
  });
}

async function runProgram() {
  if (running || !editor || !doc) return;
  clearOutput();
  $('stats').textContent = '';
  setRunning(true);
  if (phone.matches) showTab('output');
  const racing = store.settings.solver === RACE.id;
  let solver = solverById(store.settings.solver);
  const started = performance.now();
  let status = racing ? `Racing ${SOLVERS.length} solvers` : 'Running';
  const tick = () => setStatus('running', `${status}… ${formatTime(performance.now() - started)}`);
  tick();
  const timer = setInterval(tick, 100);
  const onStatus = (text) => {
    status = text.replace(/…$/, '');
    tick();
  };
  try {
    const request = { type: 'run', name: doc.fileName, source: editor.text, config: config(), verbose: store.settings.verbose };
    let result;
    if (racing) {
      ({ result, solver } = await race(request, onStatus));
    } else {
      ({ result } = await runner(0).request(
        { ...request, solver: solver.id },
        { onOutput: (text) => emit('out', text), onTrace: (text) => emit('trace', `${text}\n`), onStatus },
      ));
    }
    flushOutput();
    for (const diagnostic of result.diagnostics) emitDiagnostic(diagnostic);
    editor.showDiagnostics(result.diagnostics);
    if (output.dropped) emit('system', `… ${plural(output.dropped, 'more line')} not shown\n`);
    if (!$('output').childElementCount && !output.queue.length) emit('system', '(the program printed nothing)\n');
    const elapsed = formatTime(performance.now() - started);
    const errors = result.diagnostics.filter((d) => d.severity === 'error').length;
    const used = !result.solves.length ? '' : racing ? ` · ${solver.label} won the race` : ` · ${solver.label}`;
    if (result.ok) setStatus('ok', `✓ Finished in ${elapsed}${used}`);
    else setStatus('error', `✗ ${plural(errors, 'error')} · ${elapsed}`);
    showStats(result);
  } catch (error) {
    flushOutput();
    const elapsed = formatTime(performance.now() - started);
    if (error.message === 'stopped') {
      emit('system', `\n⏹ Stopped after ${elapsed}.\n`);
      setStatus('stopped', `⏹ Stopped after ${elapsed}`);
    } else if (error.message === 'the compiler crashed') {
      emit('crash', 'Internal compiler error: the compiler crashed on this program. Please report it!\n');
      setStatus('error', '✗ The compiler crashed');
    } else {
      emit('crash', `${error.message}\n`);
      setStatus('error', '✗ Could not run the program');
    }
  } finally {
    clearInterval(timer);
    setRunning(false);
  }
}

function stopProgram() {
  if (running) runners.forEach((worker) => worker.restart());
}

/** The compiler's analysis for the editor (the latest request wins). */
let analysisGeneration = 0;
async function analyze(source) {
  const generation = ++analysisGeneration;
  try {
    const { result } = await analyzer.request({ type: 'analyze', name: doc?.fileName ?? 'program.rsat', source, config: config() });
    return generation === analysisGeneration ? result : null;
  } catch {
    return null;
  }
}

// --- sharing, toasts, layout ----------------------------------------------------------------

function toast(message) {
  const element = $('toast');
  element.textContent = message;
  element.classList.add('visible');
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => element.classList.remove('visible'), 2400);
}

async function share() {
  const hash = await encodeShare(editor.text);
  const url = `${location.origin}${location.pathname}${hash}`;
  history.replaceState(null, '', hash);
  if (phone.matches && navigator.share) {
    try {
      await navigator.share({ title: `${doc.title} · RedSAT`, url });
      return;
    } catch {
      // Cancelled or unsupported: fall back to the clipboard.
    }
  }
  try {
    await navigator.clipboard.writeText(url);
    toast('Link copied to the clipboard ✓');
  } catch {
    toast('Link ready in the address bar');
  }
}

function setupDivider() {
  const workspace = $('workspace');
  const divider = $('divider');
  if (store.split) workspace.style.setProperty('--split', `${store.split}%`);
  const move = (event) => {
    const rect = workspace.getBoundingClientRect();
    const percent = Math.min(80, Math.max(20, ((event.clientX - rect.left) / rect.width) * 100));
    workspace.style.setProperty('--split', `${percent}%`);
    store.split = Number(percent.toFixed(1));
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

function setupOptions() {
  for (const id of ['verbose', 'strict']) {
    const box = $(id);
    box.checked = store.settings[id];
    box.addEventListener('change', () => {
      store.settings[id] = box.checked;
      store.saveSettings();
      if (id === 'strict') editor?.view.dispatch({}); // re-check with the new options
    });
  }
  const select = $('solver');
  select.replaceChildren(
    ...[...SOLVERS, RACE].map((solver) => Object.assign(new Option(solver.label, solver.id), { title: solver.detail })),
  );
  select.value = store.settings.solver === RACE.id ? RACE.id : solverById(store.settings.solver).id;
  select.addEventListener('change', () => {
    store.settings.solver = select.value;
    store.saveSettings();
    if (doc) renderSlowNote();
  });
  $('use-cadical').addEventListener('click', () => {
    select.value = 'cadical';
    select.dispatchEvent(new Event('change'));
    toast('Solver: CaDiCaL');
  });
}

function setupMenu() {
  const menu = $('menu');
  $('menu-button').addEventListener('click', () => menu.showModal());
  menu.addEventListener('click', (event) => {
    if (event.target === menu || event.target.closest('[data-close]')) menu.close();
  });
  $('menu-new').addEventListener('click', () => (menu.close(), newFile()));
  $('menu-home').addEventListener('click', () => (menu.close(), showView('home')));
  $('menu-share').addEventListener('click', () => (menu.close(), share()));
}

// --- start-up -------------------------------------------------------------------------------

async function start() {
  // iOS zooms into text fields with small fonts as they get focus; it still allows pinch zoom.
  if (/iP(hone|ad|od)/.test(navigator.userAgent)) {
    const viewport = document.querySelector('meta[name=viewport]');
    viewport.content += ', maximum-scale=1';
  }
  for (const button of document.querySelectorAll('.run-button')) button.addEventListener('click', runProgram);
  for (const button of document.querySelectorAll('.stop-button')) button.addEventListener('click', stopProgram);
  $('share').addEventListener('click', share);
  $('new-file').addEventListener('click', newFile);
  setupSwitcher({
    button: $('file-button'),
    panel: $('switcher'),
    state: () => ({ files: store.files, examples, current: doc && { kind: doc.kind, id: doc.id } }),
    actions: {
      newFile,
      openFile: (id) => open(fileDoc(store.file(id))),
      openExample: (name) => open(exampleDoc(exampleNamed(name))),
      browse: () => showView('home'),
    },
  });
  $('brand').addEventListener('click', (event) => {
    event.preventDefault();
    showView('home');
  });
  $('reset').addEventListener('click', resetExample);
  $('save-copy').addEventListener('click', saveCopy);
  $('delete-file').addEventListener('click', () => deleteFile(doc.id));
  $('doc-title').addEventListener('change', renameFile);
  $('doc-title').addEventListener('keydown', (event) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      editor.focus();
    }
  });
  document.addEventListener('keydown', (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
      event.preventDefault();
      runProgram();
    } else if (event.key === 'Escape') {
      if (running) stopProgram();
      else if (document.body.dataset.view === 'home' && doc) showView('editor');
    }
  });
  setupOptions();
  setupDivider();
  setupMenu();

  setStatus('running', 'Loading the compiler…');
  let hello;
  try {
    [hello, examples] = await Promise.all([
      runner(0).request({ type: 'hello' }),
      fetch('examples.json').then((response) => response.json()),
    ]);
  } catch (error) {
    setStatus('error', `✗ Could not load the compiler: ${error.message}`);
    return;
  }
  $('version').textContent = `RedSAT ${hello.version}`;

  editor = createEditor({ parent: $('editor'), words: lexicon(hello.vocabulary), analyze, onRun: runProgram, onChange: onEdit });
  setupMobile({ editor, query: phone, options: $('options'), desktopSlot: $('options-slot'), mobileSlot: $('menu-options') });
  setStatus('idle', 'Ready');

  const shared = await decodeShare(location.hash);
  if (shared !== null) {
    sharedSource = shared;
    open(sharedDoc());
  } else {
    const last = store.current;
    const lastDoc =
      last?.kind === 'example' && exampleNamed(last.id)
        ? exampleDoc(exampleNamed(last.id))
        : last?.kind === 'file' && store.file(last.id)
          ? fileDoc(store.file(last.id))
          : null;
    if (lastDoc && store.visited) open(lastDoc);
    else showView('home');
  }

  // A share link opened in a tab that already shows the playground.
  window.addEventListener('hashchange', async () => {
    const source = await decodeShare(location.hash);
    if (source === null || (doc?.kind === 'shared' && source === editor.text)) return;
    sharedSource = source;
    open(sharedDoc());
  });
}

start();
