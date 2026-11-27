// Builds the RedSAT compiler for WebAssembly and generates its JavaScript bindings in pkg/.
//
// Needs the Rust target (`rustup target add wasm32-unknown-unknown`) and a wasm-bindgen CLI
// matching the crate's wasm-bindgen version (`cargo install wasm-bindgen-cli --version …`).
// Runs `wasm-opt` (from binaryen) too when it is installed.

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../..', import.meta.url));
const pkg = fileURLToPath(new URL('../pkg', import.meta.url));
const env = { ...process.env, PATH: [join(homedir(), '.cargo', 'bin'), process.env.PATH].join(delimiter) };

function run(command, args) {
  console.log(`$ ${command} ${args.join(' ')}`);
  execFileSync(command, args, { cwd: root, env, stdio: 'inherit' });
}

function installed(command) {
  try {
    execFileSync(command, ['--version'], { env, stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

const manifest = readFileSync(join(root, 'crates/redsat-wasm/Cargo.toml'), 'utf8');
const wanted = manifest.match(/wasm-bindgen = "=([\d.]+)"/)?.[1];
const have = installed('wasm-bindgen')
  ? execFileSync('wasm-bindgen', ['--version'], { env }).toString().trim().split(' ')[1]
  : null;
if (have !== wanted) {
  console.error(
    `wasm-bindgen ${wanted} is needed (found ${have ?? 'none'}):\n` +
      `  cargo install wasm-bindgen-cli --version ${wanted} --locked`,
  );
  process.exit(1);
}

run('cargo', ['build', '--release', '--target', 'wasm32-unknown-unknown', '-p', 'redsat-wasm']);
const wasm = join(root, 'target/wasm32-unknown-unknown/release/redsat_wasm.wasm');
run('wasm-bindgen', ['--target', 'web', '--no-typescript', '--out-dir', pkg, wasm]);

const output = join(pkg, 'redsat_wasm_bg.wasm');
if (installed('wasm-opt')) {
  run('wasm-opt', ['-O3', '--all-features', output, '-o', output]);
} else {
  console.log('(wasm-opt not found: skipping size optimization)');
}
if (!existsSync(output)) throw new Error(`missing ${output}`);
console.log(`pkg/redsat_wasm_bg.wasm: ${(statSync(output).size / 1024).toFixed(0)} KiB`);
