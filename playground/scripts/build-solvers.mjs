// Builds the SAT solvers the playground offers next to its built-in one, compiled to
// WebAssembly with Emscripten, into pkg/solvers/: <id>.mjs + <id>.wasm per solver, its
// licence as <id>-LICENSE.txt, and solvers.json listing what was built. scripts/build.mjs
// copies them to dist/solvers/; src/solvers/<id>.js loads them.
//
//   node scripts/build-solvers.mjs                build every solver
//   node scripts/build-solvers.mjs cadical ...    build only these
//
// Emscripten: uses $EMSDK, else em++ on the PATH, else installs EMSDK_VERSION into
// ~/.cache/emsdk. Solver sources are downloaded once at pinned commits into
// ~/.cache/redsat-solvers/ ($XDG_CACHE_HOME is honoured) and checked against a hash of their
// files; compiled library objects are cached there too, keyed by compiler and flags.

import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { availableParallelism, homedir } from 'node:os';
import { basename, delimiter, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

/** The Emscripten release installed when none is found (and pinned in pages.yml). */
const EMSDK_VERSION = '7.0.0';

/**
 * The solvers. `files` is the SHA-256 of the source tree at `commit` (see treeHash), which
 * pins the sources independently of how GitHub compresses its archives.
 */
const SOLVERS = [
  {
    id: 'cadical',
    name: 'CaDiCaL 2.2.1', // the version RedSAT's native build links (rustsat-cadical)
    repo: 'arminbiere/cadical',
    tag: 'rel-2.2.1',
    commit: '4198d817d0dcde5b1240eefbff70b555b7df2af9',
    files: '4353138a867654ee7a5fc85ffc0e305e56b883fc96eb18e8dadd151d23620ecb',
    license: 'MIT',
    check: (dir) => readFileSync(join(dir, 'VERSION'), 'utf8').trim() === '2.2.1',
    // The library: every source but the two applications (and the unused IPASIR shim).
    sources: (dir) =>
      readdirSync(join(dir, 'src'))
        .filter((file) => /\.(cpp|c)$/.test(file))
        .filter((file) => !['cadical.cpp', 'mobical.cpp', 'ipasir.cpp'].includes(file))
        .map((file) => `src/${file}`),
    include: ['src'],
    // NBUILD: no configure-generated build.hpp. QUIET, NTRACING: no messages or API tracing.
    // NCLOSEFROM: no closefrom() (only used to pipe compressed files through gzip).
    defines: ['NDEBUG', 'NBUILD', 'QUIET', 'NTRACING', 'NCLOSEFROM'],
    flags: [],
    patches: [],
  },
  {
    id: 'minisat',
    name: 'MiniSat 2.2',
    repo: 'niklasso/minisat',
    tag: null, // the last commit on master (2.2.0 plus fixes); there is no later release
    commit: '37dc6c67e2af26379d88ce349eb9c4c6160e8543',
    files: 'a0e01b52ccd2b75942bf6f2a8328e9a9de6e5ee5aed9fefa5a0f31931de5b946',
    license: 'MIT',
    check: () => true,
    sources: () => ['minisat/core/Solver.cc', 'minisat/utils/Options.cc'],
    include: ['.'],
    defines: ['NDEBUG', '__STDC_LIMIT_MACROS', '__STDC_FORMAT_MACROS'],
    // The option parser's headers include zlib.h (for gzipped DIMACS), though the solver
    // never reads files: Emscripten's zlib port provides the header. The other flag accepts
    // the pre-C++11 `"%"PRIi64` string concatenations.
    flags: ['-sUSE_ZLIB', '-Wno-reserved-user-defined-literal'],
    patches: [
      // Clang rejects a default argument on a friend declaration that is not a definition;
      // Lit's field is public, so the friend declaration is unnecessary.
      {
        file: 'minisat/core/SolverTypes.h',
        from: '    friend Lit mkLit(Var var, bool sign = false);\n',
        to: '',
      },
      {
        file: 'minisat/core/SolverTypes.h',
        from: 'inline  Lit  mkLit     (Var var, bool sign) {',
        to: 'inline  Lit  mkLit     (Var var, bool sign = false) {',
      },
    ],
  },
];

/** Compiler flags for the solver library sources and the glue alike. */
const COMPILE_FLAGS = ['-O3', '-fno-rtti'];

/**
 * Linker flags: an ES module factory that runs in browsers, workers and Node.js (for the
 * tests), with memory that grows up to the 4 GiB a 32-bit WebAssembly memory can address.
 */
const LINK_FLAGS = [
  '-O3',
  '-sMODULARIZE',
  '-sEXPORT_ES6',
  '-sENVIRONMENT=web,worker,node',
  '-sALLOW_MEMORY_GROWTH',
  '-sMAXIMUM_MEMORY=4GB',
  '-sSTACK_SIZE=4MB',
  '-sFILESYSTEM=0',
  '-sEXPORTED_FUNCTIONS=' +
    ['name', 'buffer', 'new', 'release', 'option', 'add', 'solve', 'value', 'failed']
      .map((name) => `_solver_${name}`)
      .join(','),
  '-sEXPORTED_RUNTIME_METHODS=HEAP32,UTF8ToString',
  '-sINCOMING_MODULE_JS_API=locateFile,print,printErr,onAbort',
];

const here = (path) => fileURLToPath(new URL(`../${path}`, import.meta.url));
const output = here('pkg/solvers');
const glueDir = here('src/solvers/native');
const cacheHome = process.env.XDG_CACHE_HOME || join(homedir(), '.cache');
const cache = join(cacheHome, 'redsat-solvers');

/** Runs `command`, echoing it with any object files abbreviated. */
function run(command, args, options = {}) {
  const objects = args.filter((arg) => arg.endsWith('.o')).length;
  const shown = args.filter((arg) => !arg.endsWith('.o'));
  if (objects > 0) shown.splice(shown.indexOf('-o'), 0, `<${objects} objects>`);
  console.log(`$ ${basename(command)} ${shown.join(' ')}`);
  execFileSync(command, args, { stdio: 'inherit', ...options });
}

/** Runs `command` without echoing it, rejecting with its output when it fails. */
function spawnQuiet(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'] });
    let log = '';
    child.stdout.on('data', (chunk) => (log += chunk));
    child.stderr.on('data', (chunk) => (log += chunk));
    child.on('error', reject);
    child.on('close', (code) =>
      code === 0 ? resolve(log) : reject(new Error(`${basename(command)} ${args.join(' ')}\n${log}`)),
    );
  });
}

/** Runs the jobs, at most `limit` at a time. */
async function parallel(jobs, limit = availableParallelism()) {
  const queue = [...jobs];
  const worker = async () => {
    while (queue.length > 0) await queue.shift()();
  };
  await Promise.all(Array.from({ length: Math.min(limit, queue.length) }, worker));
}

/** The Emscripten directory holding em++ and emcc, installing the SDK if need be. */
function emscripten() {
  const candidates = [];
  if (process.env.EMSDK) candidates.push(join(process.env.EMSDK, 'upstream', 'emscripten'));
  candidates.push(...(process.env.PATH ?? '').split(delimiter).filter(Boolean));
  const found = candidates.find((dir) => existsSync(join(dir, 'em++')));
  if (found) return found;

  const emsdk = join(cacheHome, 'emsdk');
  const dir = join(emsdk, 'upstream', 'emscripten');
  if (!existsSync(join(dir, 'em++'))) {
    console.log(`Emscripten not found: installing ${EMSDK_VERSION} into ${emsdk}`);
    if (!existsSync(join(emsdk, 'emsdk'))) {
      run('git', ['clone', '--quiet', 'https://github.com/emscripten-core/emsdk.git', emsdk]);
    }
    run(join(emsdk, 'emsdk'), ['install', EMSDK_VERSION], { cwd: emsdk });
    run(join(emsdk, 'emsdk'), ['activate', EMSDK_VERSION], { cwd: emsdk });
  }
  return dir;
}

/** A hash of every file under `dir` (paths and contents, symlinks as their targets). */
function treeHash(dir) {
  const files = [];
  const walk = (relative) => {
    for (const entry of readdirSync(join(dir, relative)).sort()) {
      const path = relative ? `${relative}/${entry}` : entry;
      const stats = lstatSync(join(dir, path));
      if (stats.isDirectory()) walk(path);
      else if (stats.isSymbolicLink()) files.push(`${path} -> ${readlinkSync(join(dir, path))}`);
      else files.push(`${path} ${createHash('sha256').update(readFileSync(join(dir, path))).digest('hex')}`);
    }
  };
  walk('');
  return createHash('sha256').update(files.join('\n')).digest('hex');
}

/** The solver's sources at its pinned commit, downloaded, verified and patched once. */
async function sources(solver) {
  const dir = join(cache, 'src', `${solver.id}-${solver.commit}`);
  if (existsSync(join(dir, '.verified'))) return dir;

  const url = `https://github.com/${solver.repo}/archive/${solver.commit}.tar.gz`;
  console.log(`Downloading ${url}`);
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
  const staging = `${dir}.download`;
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  const archive = join(staging, '..', `${solver.id}-${solver.commit}.tar.gz`);
  writeFileSync(archive, Buffer.from(await response.arrayBuffer()));
  execFileSync('tar', ['-xzf', archive, '--strip-components=1', '-C', staging]);
  rmSync(archive);

  const hash = treeHash(staging);
  if (hash !== solver.files || !solver.check(staging)) {
    throw new Error(
      `${solver.repo}@${solver.commit}: the downloaded sources do not match the pinned ones\n` +
        `  expected ${solver.files}\n  found    ${hash}`,
    );
  }
  for (const { file, from, to } of solver.patches) {
    const path = join(staging, file);
    const text = readFileSync(path, 'utf8');
    if (text.split(from).length !== 2) throw new Error(`patch for ${file} does not apply`);
    writeFileSync(path, text.replace(from, to));
  }
  writeFileSync(join(staging, '.verified'), `${solver.repo}@${solver.commit}\n`);
  rmSync(dir, { recursive: true, force: true });
  renameSync(staging, dir);
  return dir;
}

/** Compiles and links one solver into pkg/solvers/, returning its solvers.json entry. */
async function build(solver, emdir, emVersion) {
  const src = await sources(solver);
  const flags = [
    ...COMPILE_FLAGS,
    ...solver.flags,
    ...solver.defines.map((define) => `-D${define}`),
    ...solver.include.map((include) => `-I${join(src, include)}`),
  ];
  const key = createHash('sha256')
    .update(JSON.stringify([emVersion, flags, solver.commit, solver.patches]))
    .digest('hex')
    .slice(0, 16);
  const objDir = join(cache, 'obj', `${solver.id}-${key}`);
  mkdirSync(objDir, { recursive: true });

  const files = solver.sources(src);
  const objects = files.map((file) => join(objDir, `${file.replaceAll('/', '_')}.o`));
  const todo = files.filter((_, i) => !existsSync(objects[i]));
  console.log(
    `${solver.name}: compiling ${todo.length} of ${files.length} library sources` +
      (todo.length < files.length ? ' (the rest are cached)' : ''),
  );
  await parallel(
    todo.map((file) => async () => {
      const object = join(objDir, `${file.replaceAll('/', '_')}.o`);
      const compiler = file.endsWith('.c') ? 'emcc' : 'em++';
      await spawnQuiet(join(emdir, compiler), [...flags, '-c', join(src, file), '-o', `${object}.tmp`]);
      renameSync(`${object}.tmp`, object);
    }),
  );

  const glue = join(objDir, 'glue.o');
  await spawnQuiet(join(emdir, 'em++'), [
    ...flags,
    `-DSOLVER_NAME=${JSON.stringify(solver.name)}`,
    '-c',
    join(glueDir, `${solver.id}.cpp`),
    '-o',
    glue,
  ]);
  run(join(emdir, 'em++'), [...LINK_FLAGS, ...objects, glue, '-o', join(output, `${solver.id}.mjs`)]);
  copyFileSync(join(src, 'LICENSE'), join(output, `${solver.id}-LICENSE.txt`));

  const wasm = readFileSync(join(output, `${solver.id}.wasm`));
  const js = readFileSync(join(output, `${solver.id}.mjs`));
  const kib = (bytes) => `${(bytes / 1024).toFixed(0)} KiB`;
  console.log(
    `pkg/solvers/${solver.id}.wasm: ${kib(wasm.length)} (${kib(gzipSync(wasm, { level: 9 }).length)} gzipped), ` +
      `${solver.id}.mjs: ${kib(js.length)} (${kib(gzipSync(js, { level: 9 }).length)} gzipped)`,
  );
  return {
    id: solver.id,
    name: solver.name,
    module: `${solver.id}.mjs`,
    license: solver.license,
    source: `https://github.com/${solver.repo}/tree/${solver.tag ?? solver.commit}`,
    wasmBytes: wasm.length,
  };
}

const wanted = process.argv.slice(2);
const unknown = wanted.filter((id) => !SOLVERS.some((solver) => solver.id === id));
if (unknown.length > 0) {
  console.error(`unknown solver ${unknown.join(', ')}: expected ${SOLVERS.map((s) => s.id).join(', ')}`);
  process.exit(1);
}

const emdir = emscripten();
const emVersion = execFileSync(join(emdir, 'emcc'), ['--version']).toString().split('\n')[0];
console.log(emVersion);
if (!emVersion.includes(` ${EMSDK_VERSION} `)) {
  console.warn(`(warning: the pinned Emscripten is ${EMSDK_VERSION}; output may differ)`);
}

mkdirSync(output, { recursive: true });
const started = performance.now();
const manifestPath = join(output, 'solvers.json');
const manifest = existsSync(manifestPath) ? JSON.parse(readFileSync(manifestPath, 'utf8')) : [];
for (const solver of SOLVERS.filter((solver) => wanted.length === 0 || wanted.includes(solver.id))) {
  const entry = await build(solver, emdir, emVersion);
  const index = manifest.findIndex((other) => other.id === entry.id);
  if (index >= 0) manifest[index] = entry;
  else manifest.push(entry);
}
manifest.sort((a, b) => SOLVERS.findIndex((s) => s.id === a.id) - SOLVERS.findIndex((s) => s.id === b.id));
writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`built ${manifest.map((entry) => entry.id).join(', ')} in ${((performance.now() - started) / 1000).toFixed(1)} s`);
