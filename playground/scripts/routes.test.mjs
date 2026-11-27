// `npm test`: the playground's addresses (src/routes.js).

import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

import { loadExamples } from './examples.mjs';
import { exampleBySlug, exampleRoute, fileRoute, parseRoute, slugOf } from '../src/routes.js';

const examples = loadExamples(fileURLToPath(new URL('../../examples', import.meta.url)));

test('titles become readable slugs', () => {
  assert.equal(slugOf("Pell's equation"), 'pells-equation');
  assert.equal(slugOf('SEND + MORE = MONEY'), 'send-more-money');
  assert.equal(slugOf('Factoring 100 digits'), 'factoring-100-digits');
});

test('every example has its own address, and the address finds it again', () => {
  const routes = examples.map(exampleRoute);
  assert.equal(new Set(routes).size, routes.length, 'two examples share an address');
  for (const example of examples) {
    const { section, key } = parseRoute(exampleRoute(example));
    assert.equal(section, 'examples');
    assert.equal(exampleBySlug(examples, key), example, key);
    // The file name works too, with underscores or dashes.
    assert.equal(exampleBySlug(examples, example.name), example);
    assert.equal(exampleBySlug(examples, slugOf(example.name)), example);
  }
});

test('addresses parse into a section and a key', () => {
  assert.deepEqual(parseRoute(''), { section: '', key: '' });
  assert.deepEqual(parseRoute('#/'), { section: '', key: '' });
  assert.deepEqual(parseRoute('#/examples'), { section: 'examples', key: '' });
  assert.deepEqual(parseRoute(fileRoute('my puzzle')), { section: 'files', key: 'my puzzle' });
  assert.deepEqual(parseRoute('#/files/%E0%A4%A'), { section: 'files', key: '%E0%A4%A' });
});
