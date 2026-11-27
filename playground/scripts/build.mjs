// Bundles the playground into dist/: the app and its worker (with CodeMirror), the WebAssembly
// compiler from pkg/ (see build-wasm.mjs), and examples.json from ../examples.
//
//   node scripts/build.mjs           build once (minified)
//   node scripts/build.mjs --serve   build, watch and serve on http://localhost:8000

import { copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import * as esbuild from 'esbuild';

import { loadExamples } from './examples.mjs';

const here = (path) => fileURLToPath(new URL(`../${path}`, import.meta.url));
const dist = here('dist');
const serve = process.argv.includes('--serve');

if (!existsSync(here('pkg/redsat_wasm.js'))) {
  console.error('pkg/ is missing: run `npm run wasm` first');
  process.exit(1);
}

rmSync(dist, { recursive: true, force: true });
mkdirSync(dist, { recursive: true });

/** Copies the static files and regenerates examples.json (on every rebuild when serving). */
function copyStatic() {
  for (const file of ['index.html', 'style.css', 'favicon.svg']) {
    copyFileSync(here(`src/${file}`), `${dist}/${file}`);
  }
  copyFileSync(here('pkg/redsat_wasm_bg.wasm'), `${dist}/redsat_wasm_bg.wasm`);
  const examples = loadExamples(here('../examples'));
  writeFileSync(`${dist}/examples.json`, JSON.stringify(examples));
  return examples.length;
}
console.log(`examples.json: ${copyStatic()} examples`);

const options = {
  entryPoints: { app: here('src/main.js'), worker: here('src/worker.js') },
  outdir: dist,
  bundle: true,
  format: 'esm',
  target: 'es2022',
  minify: !serve,
  sourcemap: true,
  logLevel: 'info',
  plugins: [{ name: 'static', setup: (build) => build.onEnd(() => void copyStatic()) }],
};

if (serve) {
  const context = await esbuild.context(options);
  await context.watch();
  const { port } = await context.serve({ servedir: dist, port: 8000 });
  console.log(`playground: http://localhost:${port}/`);
} else {
  await esbuild.build(options);
}
