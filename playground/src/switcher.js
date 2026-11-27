// The file switcher: a dropdown under the file button to open your files and the examples,
// with type-to-filter and keyboard navigation (↑ ↓ Enter Esc).

function h(tag, attributes = {}, ...children) {
  const element = document.createElement(tag);
  for (const [key, value] of Object.entries(attributes)) {
    if (key === 'class') element.className = value;
    else if (key.startsWith('on')) element.addEventListener(key.slice(2), value);
    else if (value != null && value !== false) element.setAttribute(key, value === true ? '' : value);
  }
  element.append(...children.flat().filter((child) => child != null && child !== false));
  return element;
}

function timeAgo(timestamp) {
  const minutes = Math.round((Date.now() - timestamp) / 60000);
  if (minutes < 1) return 'edited just now';
  if (minutes < 60) return `edited ${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `edited ${hours} h ago`;
  return `edited ${new Date(timestamp).toLocaleDateString()}`;
}

/**
 * @param {object} options
 * @param {HTMLElement} options.button     the file button that opens the switcher
 * @param {HTMLElement} options.panel      an element with the `popover` attribute
 * @param {() => object} options.state     {files, examples, current}
 * @param {object} options.actions         {openFile(id), openExample(name), newFile(), browse()}
 */
export function setupSwitcher({ button, panel, state, actions }) {
  const search = h('input', {
    class: 'switcher-search',
    type: 'search',
    placeholder: 'Search your files and the examples…',
    'aria-label': 'Search',
    autocomplete: 'off',
    spellcheck: 'false',
  });
  const list = h('div', { class: 'switcher-list', role: 'listbox' });
  panel.append(search, list);

  let rows = [];
  let active = 0;

  const close = () => panel.hidePopover();
  const run = (action) => {
    close();
    action();
  };

  function render() {
    const { files, examples, current } = state();
    const words = search.value.toLowerCase().split(/\s+/).filter(Boolean);
    const matches = (...texts) => words.every((word) => texts.some((text) => text.toLowerCase().includes(word)));
    const isCurrent = (kind, id) => current?.kind === kind && current.id === id;
    rows = [];
    const sections = [];

    const row = (label, detail, action, { current: selected = false, kind = 'item' } = {}) => {
      const element = h(
        'button',
        { class: `switcher-row ${kind}${selected ? ' current' : ''}`, role: 'option', tabindex: '-1', onclick: () => run(action) },
        h('span', { class: 'switcher-title' }, label),
        detail ? h('span', { class: 'switcher-detail' }, detail) : null,
      );
      element.addEventListener('pointermove', () => highlight(rows.indexOf(element)));
      rows.push(element);
      return element;
    };

    const yours = files.filter((file) => matches(file.name));
    const own = [];
    if (!words.length || matches('new file')) own.push(row('+ New file', 'start from a small template', actions.newFile, { kind: 'command' }));
    for (const file of yours) {
      own.push(row(`${file.name}.rsat`, timeAgo(file.updated), () => actions.openFile(file.id), { current: isCurrent('file', file.id), kind: 'file' }));
    }
    if (own.length) sections.push(section('Your files', yours.length, own));

    const byCategory = new Map();
    for (const example of examples) {
      if (!matches(example.title, example.description, example.name, example.category ?? '')) continue;
      if (!byCategory.has(example.category)) byCategory.set(example.category, []);
      byCategory.get(example.category).push(example);
    }
    for (const [category, group] of byCategory) {
      sections.push(
        section(
          category,
          group.length,
          group.map((example) =>
            row(example.title, example.description, () => actions.openExample(example.name), {
              current: isCurrent('example', example.name),
              kind: 'example',
            }),
          ),
        ),
      );
    }
    if (!rows.length) sections.push(h('p', { class: 'switcher-empty' }, 'Nothing matches.'));
    sections.push(h('div', { class: 'switcher-footer' }, row('Browse the examples gallery →', null, actions.browse, { kind: 'command' })));
    list.replaceChildren(...sections);
    highlight(words.length ? 0 : Math.max(0, rows.findIndex((r) => r.classList.contains('current'))));
  }

  /** A titled group of rows; its header stays in view while the group scrolls past. */
  function section(title, count, rows) {
    const header = h('h3', {}, h('span', {}, title), count ? h('span', { class: 'switcher-count' }, String(count)) : null);
    return h('div', { class: 'switcher-section' }, header, rows);
  }

  function highlight(index) {
    if (index < 0 || index >= rows.length) return;
    rows[active]?.classList.remove('active');
    active = index;
    rows[active].classList.add('active');
    rows[active].scrollIntoView({ block: 'nearest' });
  }

  function place() {
    const rect = button.getBoundingClientRect();
    const narrow = window.innerWidth <= 760;
    panel.style.top = `${rect.bottom + 6}px`;
    panel.style.left = narrow ? '8px' : `${Math.max(8, rect.left)}px`;
  }

  search.addEventListener('input', render);
  panel.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      highlight(Math.min(rows.length - 1, Math.max(0, active + (event.key === 'ArrowDown' ? 1 : -1))));
    } else if (event.key === 'Enter') {
      event.preventDefault();
      rows[active]?.click();
    }
  });
  panel.addEventListener('toggle', (event) => {
    button.setAttribute('aria-expanded', String(event.newState === 'open'));
    if (event.newState === 'open') {
      search.value = '';
      render();
      place();
      if (window.innerWidth > 760) search.focus();
    }
  });
  window.addEventListener('resize', () => panel.matches(':popover-open') && place());
  // The browser toggles the panel (and keeps a click on the open button from reopening it).
  button.setAttribute('popovertarget', panel.id);
}
