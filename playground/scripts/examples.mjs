// Turns ../examples/*.rsat into the playground's example list.
//
// The curated list below sets the order (gentle to hard) and the titles. An example missing
// from it still appears, at the end, titled after its file name, so new examples never go
// missing from the playground.

import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

/** The categories of the home page, in order. Examples not listed below go to the last one. */
export const CATEGORIES = ['Getting started', 'Puzzles', 'Planning & optimization', 'Numbers', 'Strings & bits', 'More'];

/** [file name without `.rsat`, title, category] in the order the playground lists them. */
export const CURATED = [
  ['chickens_and_rabbits', 'Chickens and rabbits', 'Getting started'],
  ['bigger_of_two', 'Two numbers', 'Getting started'],
  ['digit_puzzle', 'Digit puzzle', 'Getting started'],
  ['send_more_money', 'SEND + MORE = MONEY', 'Puzzles'],
  ['sudoku', 'Sudoku', 'Puzzles'],
  ['n_queens', 'N-Queens', 'Puzzles'],
  ['magic_square', 'Magic square', 'Puzzles'],
  ['knights_and_knaves', 'Knights and knaves', 'Puzzles'],
  ['coins', 'Making change', 'Planning & optimization'],
  ['graph_coloring', 'Graph coloring', 'Planning & optimization'],
  ['scheduling', 'Scheduling', 'Planning & optimization'],
  ['knapsack', 'Knapsack', 'Planning & optimization'],
  ['pythagorean', 'Pythagorean triples', 'Numbers'],
  ['sums_of_two_squares', 'Sums of two squares', 'Numbers'],
  ['factor', 'Factoring', 'Numbers'],
  ['factor100', 'Factoring 100 digits', 'Numbers'],
  ['pell', "Pell's equation", 'Numbers'],
  ['palindromes', 'Palindromes', 'Strings & bits'],
  ['bits', 'Bit tricks', 'Strings & bits'],
];

/** Examples that are slow with the built-in solver, with what to tell the user. */
export const SLOW = {
  factor: 'About 15 s with the built-in solver. Pick CaDiCaL as the solver for an instant answer.',
  pell: 'Very slow with the built-in solver. Pick CaDiCaL as the solver: it answers in milliseconds.',
};

/** Examples that no solver finishes in reasonable time, with what to tell the user (shown
 * whatever the solver). */
export const HARD = {
  factor100:
    'A genuinely hard problem: no SAT solver factors a 100-digit number in any reasonable time. Turn on Verbose to see how it compiles, then press Stop.',
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
  const titles = new Map(CURATED.map(([name, title]) => [name, title]));
  const categories = new Map(CURATED.map(([name, , category]) => [name, category]));
  return readdirSync(directory)
    .filter((file) => file.endsWith('.rsat'))
    .map((file) => {
      const name = file.slice(0, -'.rsat'.length);
      const source = readFileSync(join(directory, file), 'utf8');
      return {
        name,
        file,
        title: titles.get(name) ?? titleFromName(name),
        category: categories.get(name) ?? CATEGORIES.at(-1),
        description: description(source),
        slow: SLOW[name] ?? null,
        hard: HARD[name] ?? null,
        source,
      };
    })
    .sort((a, b) => {
      const rank = (example) => order.get(example.name) ?? CURATED.length;
      return rank(a) - rank(b) || a.name.localeCompare(b.name);
    });
}
