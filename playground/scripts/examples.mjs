// Turns ../examples/*.rsat into the playground's example list.
//
// The curated list below sets the order (gentle to hard) and the titles. An example missing
// from it still appears, at the end, titled after its file name, so new examples never go
// missing from the playground.

import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

/** [file name without `.rsat`, title] in the order the playground lists them. */
export const CURATED = [
  ['chickens_and_rabbits', 'Chickens and rabbits'],
  ['bigger_of_two', 'Two numbers'],
  ['digit_puzzle', 'Digit puzzle'],
  ['coins', 'Making change'],
  ['send_more_money', 'SEND + MORE = MONEY'],
  ['sudoku', 'Sudoku'],
  ['n_queens', 'N-Queens'],
  ['magic_square', 'Magic square'],
  ['knights_and_knaves', 'Knights and knaves'],
  ['graph_coloring', 'Graph coloring'],
  ['scheduling', 'Scheduling'],
  ['knapsack', 'Knapsack'],
  ['pythagorean', 'Pythagorean triples'],
  ['sums_of_two_squares', 'Sums of two squares'],
  ['palindromes', 'Palindromes'],
  ['bits', 'Bit tricks'],
  ['factor', 'Factoring'],
  ['pell', "Pell's equation"],
];

/** Examples that take long on the in-browser solver, with the reason shown to the user. */
export const SLOW = {
  factor:
    'Takes about 15 s with the built-in solver in the browser (CaDiCaL on the command line: 0.04 s).',
  pell: 'Very slow with the built-in solver in the browser; CaDiCaL on the command line solves it in milliseconds.',
};

/** The first paragraph of the comment block at the top of a program. */
export function description(source) {
  const lines = [];
  for (const line of source.split('\n')) {
    if (!line.startsWith('//')) break;
    const text = line.replace(/^\/\/\s?/, '').trim();
    if (text === '') break;
    lines.push(text);
  }
  return lines.join(' ');
}

function titleFromName(name) {
  const words = name.replace(/_/g, ' ');
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** The examples in `directory`, in playground order. */
export function loadExamples(directory) {
  const order = new Map(CURATED.map(([name], index) => [name, index]));
  const titles = new Map(CURATED);
  return readdirSync(directory)
    .filter((file) => file.endsWith('.rsat'))
    .map((file) => {
      const name = file.slice(0, -'.rsat'.length);
      const source = readFileSync(join(directory, file), 'utf8');
      return {
        name,
        file,
        title: titles.get(name) ?? titleFromName(name),
        description: description(source),
        slow: SLOW[name] ?? null,
        source,
      };
    })
    .sort((a, b) => {
      const rank = (example) => order.get(example.name) ?? CURATED.length;
      return rank(a) - rank(b) || a.name.localeCompare(b.name);
    });
}
