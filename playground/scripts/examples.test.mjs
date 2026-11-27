// `npm test`: the example list stays in sync with ../examples.

import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

import { CATEGORIES, CURATED, SLOW, description, loadExamples } from './examples.mjs';

const examplesDir = fileURLToPath(new URL('../../examples', import.meta.url));

test('every curated example exists', () => {
  for (const [name] of CURATED) {
    assert.ok(existsSync(`${examplesDir}/${name}.rsat`), `examples/${name}.rsat (renamed?)`);
  }
  for (const [name, , category] of CURATED) {
    assert.ok(CATEGORIES.includes(category), `${name}: unknown category ${category}`);
  }
  for (const name of Object.keys(SLOW)) {
    assert.ok(existsSync(`${examplesDir}/${name}.rsat`), `examples/${name}.rsat`);
  }
});

test('every example has a title and a description', () => {
  const examples = loadExamples(examplesDir);
  assert.ok(examples.length >= CURATED.length);
  for (const example of examples) {
    assert.ok(example.title, example.file);
    assert.ok(example.description.length > 10, `${example.file} starts with a comment`);
  }
  assert.equal(examples[0].name, CURATED[0][0]);
});

test('the description is the first paragraph of the leading comment', () => {
  const source = '// First line\n// continues.\n//\n// Second paragraph.\nvar x\n';
  assert.equal(description(source), 'First line continues.');
});
