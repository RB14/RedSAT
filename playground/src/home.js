// The home view: a welcome for newcomers, your files, and the examples by category.

const CATEGORY_ORDER = ['Getting started', 'Puzzles', 'Planning & optimization', 'Numbers', 'Strings & bits', 'More'];

function h(tag, attributes = {}, ...children) {
  const element = document.createElement(tag);
  for (const [key, value] of Object.entries(attributes)) {
    if (key === 'class') element.className = value;
    else if (key.startsWith('on')) element.addEventListener(key.slice(2), value);
    else if (value !== false && value != null) element.setAttribute(key, value === true ? '' : value);
  }
  element.append(...children.flat().filter((child) => child != null && child !== false));
  return element;
}

function timeAgo(timestamp) {
  const seconds = Math.round((Date.now() - timestamp) / 1000);
  if (seconds < 60) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return new Date(timestamp).toLocaleDateString();
}

/** A tiny, pre-highlighted RedSAT program for the welcome banner. */
function teaser() {
  const k = (text) => h('span', { class: 'k' }, text);
  const n = (text) => h('span', { class: 'n' }, text);
  const c = (text) => h('span', { class: 'c' }, text);
  return h(
    'pre',
    { class: 'teaser', 'aria-hidden': 'true' },
    k('var'), ' x, y: ', h('span', { class: 't' }, 'int'), '[', n('1'), '..=', n('100'), ']\n',
    k('require'), ' x * y == ', n('91'), ' ', k('and'), ' x < y\n',
    k('solve'), ' x, y\n',
    c('// x = 7, y = 13'),
  );
}

/**
 * Renders the home view into `root`.
 * @param {HTMLElement} root
 * @param {object} state  {examples, files, current, firstVisit}
 * @param {object} actions  {newFile, openExample(name), openFile(id), deleteFile(id), close}
 */
export function renderHome(root, state, actions) {
  const { examples, files, current, firstVisit } = state;

  const hero = h(
    'section',
    { class: `hero${firstVisit ? '' : ' compact'}` },
    h(
      'div',
      { class: 'hero-text' },
      h('h1', {}, 'Describe the answer. ', h('span', {}, 'RedSAT finds it.')),
      h(
        'p',
        {},
        'Write ordinary-looking code with unknowns in it. State what must hold, and the compiler ',
        'turns your program into a SAT problem, solves it, and prints the values that work.',
      ),
      h(
        'div',
        { class: 'hero-actions' },
        h('button', { class: 'primary big', onclick: actions.newFile }, '+ New file'),
        h('button', { class: 'big', onclick: () => actions.openExample(examples[0].name) }, 'Start with an example'),
      ),
      h('p', { class: 'hero-note' }, 'Runs entirely in your browser · ', h('a', { href: 'guide/' }, 'Read the guide →')),
    ),
    teaser(),
  );

  const fileCards = files.map((file) =>
    h(
      'article',
      { class: `card file${current?.kind === 'file' && current.id === file.id ? ' current' : ''}` },
      h(
        'button',
        { class: 'card-main', onclick: () => actions.openFile(file.id) },
        h('span', { class: 'card-title' }, `${file.name}.rsat`),
        h('span', { class: 'card-text' }, `Edited ${timeAgo(file.updated)}`),
      ),
      h(
        'button',
        {
          class: 'card-delete',
          title: `Delete ${file.name}.rsat`,
          'aria-label': `Delete ${file.name}.rsat`,
          onclick: () => actions.deleteFile(file.id),
        },
        '✕',
      ),
    ),
  );
  const newCard = h(
    'article',
    { class: 'card new' },
    h('button', { class: 'card-main', onclick: actions.newFile }, h('span', { class: 'card-title' }, '+ New file'), h('span', { class: 'card-text' }, 'Start from a small template')),
  );
  const yourFiles = h('section', { class: 'shelf' }, h('h2', {}, 'Your files'), h('div', { class: 'cards' }, newCard, ...fileCards));

  const byCategory = new Map();
  for (const example of examples) {
    const category = example.category ?? 'More';
    if (!byCategory.has(category)) byCategory.set(category, []);
    byCategory.get(category).push(example);
  }
  const categories = [...byCategory.keys()].sort((a, b) => CATEGORY_ORDER.indexOf(a) - CATEGORY_ORDER.indexOf(b));
  const exampleShelves = categories.map((category) =>
    h(
      'section',
      { class: 'shelf' },
      h('h2', {}, category),
      h(
        'div',
        { class: 'cards' },
        byCategory.get(category).map((example) =>
          h(
            'article',
            { class: `card${current?.kind === 'example' && current.id === example.name ? ' current' : ''}` },
            h(
              'button',
              { class: 'card-main', onclick: () => actions.openExample(example.name) },
              h('span', { class: 'card-title' }, example.title, example.slow ? h('span', { class: 'tag', title: example.slow }, 'slow') : null),
              h('span', { class: 'card-text' }, example.description),
            ),
          ),
        ),
      ),
    ),
  );

  const close = current
    ? h('button', { class: 'ghost home-close', onclick: actions.close, title: 'Back to the editor (Esc)' }, '← Back to the editor')
    : null;

  root.replaceChildren(h('div', { class: 'home-inner' }, close, hero, yourFiles, h('div', { class: 'examples-title' }, h('h2', {}, 'Examples')), exampleShelves));
}
