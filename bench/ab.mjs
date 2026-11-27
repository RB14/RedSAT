// A/B comparison of two redsat builds on the benchmark programs.
//
//   node bench/ab.mjs [--runs N] [--threads J] OLD NEW [programs…]
//
// Runs every program N times (default 5) with each binary, alternating between them so that
// changes in machine load affect both alike, and prints the median wall time of each, their
// ratio, and the geometric mean of the ratios (below 1: NEW is faster). Outputs are compared as
// sorted lines, so a different order of the same solutions is fine; a real difference (another
// valid solution of an under-constrained problem, or a bug) is flagged for review. Without
// programs, every bench/*.rsat is used.

import { spawnSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { basename, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const at = args.indexOf(name);
  if (at < 0) return fallback;
  const [, value] = args.splice(at, 2);
  return value;
};
const runs = Number(option('--runs', '5'));
const threads = option('--threads', '1');
const [old, neu, ...given] = args;
if (!old || !neu) {
  console.error('usage: node bench/ab.mjs [--runs N] [--threads J] OLD NEW [programs…]');
  process.exit(2);
}
const here = fileURLToPath(new URL('.', import.meta.url));
const programs = given.length ? given : readdirSync(here).filter((f) => f.endsWith('.rsat')).sort().map((f) => join(here, f));

function run(binary, program) {
  const start = performance.now();
  const result = spawnSync(binary, ['-j', threads, program], { encoding: 'utf8', timeout: 300_000 });
  const seconds = (performance.now() - start) / 1000;
  const output = `${result.stdout}${result.stderr}`.split('\n').map((line) => line.trimEnd()).filter(Boolean).sort().join('\n');
  return { seconds: result.status === null ? Infinity : seconds, output };
}

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
};

console.log(`${'program'.padEnd(26)}${'old (s)'.padStart(9)}${'new (s)'.padStart(9)}${'new/old'.padStart(9)}`);
let logSum = 0;
let counted = 0;
for (const program of programs) {
  const times = { old: [], new: [] };
  let outputs = { old: null, new: null };
  for (let i = 0; i < runs; i++) {
    for (const [key, binary] of i % 2 ? [['new', neu], ['old', old]] : [['old', old], ['new', neu]]) {
      const { seconds, output } = run(binary, program);
      times[key].push(seconds);
      outputs[key] ??= output;
    }
  }
  const [a, b] = [median(times.old), median(times.new)];
  const ratio = b / a;
  if (Number.isFinite(ratio) && ratio > 0) {
    logSum += Math.log(ratio);
    counted += 1;
  }
  const note = outputs.old === outputs.new ? '' : '  (outputs differ: review)';
  const fmt = (s) => (Number.isFinite(s) ? s.toFixed(3) : 'timeout').padStart(9);
  console.log(`${basename(program, '.rsat').padEnd(26)}${fmt(a)}${fmt(b)}${ratio.toFixed(2).padStart(9)}${note}`);
}
console.log(`${'geometric mean'.padEnd(44)}${Math.exp(logSum / Math.max(counted, 1)).toFixed(2).padStart(9)}`);
