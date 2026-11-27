// A dropdown picker with the look and behaviour of the file switcher: a button showing the
// current choice, and a popover list of options (each with a short description) that opens
// below it. Keyboard: ↑ ↓ to move, Enter to pick, Esc to close.

let nextId = 0;

/** Places a popover `panel` next to the button that opens it: below, unless there is more room
 * above (e.g. at the bottom of the screen); right-aligned with the button near the right edge,
 * else left-aligned. */
export function placePanel(panel, button, maxWidth = 360) {
  const rect = button.getBoundingClientRect();
  const width = Math.min(maxWidth, window.innerWidth - 16);
  panel.style.width = `${width}px`;
  panel.style.maxHeight = '';
  const height = panel.offsetHeight;
  const below = window.innerHeight - rect.bottom - 14;
  const above = rect.top - 14;
  if (height > below && above > below) {
    const room = Math.min(height, above);
    panel.style.maxHeight = `${room}px`;
    panel.style.top = `${rect.top - 6 - room}px`;
  } else {
    panel.style.maxHeight = `${Math.max(120, below)}px`;
    panel.style.top = `${rect.bottom + 6}px`;
  }
  const left = rect.left + width > window.innerWidth - 8 ? rect.right - width : rect.left;
  panel.style.left = `${Math.max(8, left)}px`;
}

/**
 * @param {object} options
 * @param {{value: string, label: string, detail?: string}[]} options.choices
 * @param {string} options.value          the initial choice
 * @param {string} options.label          an accessible name for the button
 * @param {(value: string) => void} options.onChange
 * @returns {{button: HTMLButtonElement, value: string, setValue(value: string): void}}
 */
export function createDropdown({ choices, value, label, onChange }) {
  const id = `dropdown-${nextId++}`;
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'dropdown-button';
  button.setAttribute('aria-haspopup', 'listbox');
  button.setAttribute('aria-expanded', 'false');
  button.setAttribute('aria-label', label);
  button.setAttribute('popovertarget', id);
  const text = document.createElement('span');
  const chevron = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  chevron.setAttribute('viewBox', '0 0 16 16');
  chevron.setAttribute('class', 'chevron');
  chevron.setAttribute('aria-hidden', 'true');
  chevron.innerHTML = '<path d="M4 6l4 4 4-4" />';
  button.append(text, chevron);

  const panel = document.createElement('div');
  panel.id = id;
  panel.className = 'switcher dropdown-panel';
  panel.setAttribute('popover', '');
  panel.setAttribute('role', 'listbox');
  panel.setAttribute('aria-label', label);
  const list = document.createElement('div');
  list.className = 'switcher-list';
  panel.append(list);
  document.body.append(panel);

  let current = value;
  let active = 0;
  const rows = choices.map((choice, index) => {
    const row = document.createElement('button');
    row.type = 'button';
    row.className = 'switcher-row';
    row.setAttribute('role', 'option');
    row.tabIndex = -1;
    const title = Object.assign(document.createElement('span'), { className: 'switcher-title', textContent: choice.label });
    row.append(title);
    if (choice.detail) row.append(Object.assign(document.createElement('span'), { className: 'switcher-detail', textContent: choice.detail }));
    row.addEventListener('click', () => {
      panel.hidePopover();
      if (choice.value !== current) {
        setValue(choice.value);
        onChange(choice.value);
      }
    });
    row.addEventListener('pointermove', () => highlight(index));
    return row;
  });
  list.append(...rows);

  function highlight(index) {
    rows[active]?.classList.remove('active');
    active = Math.max(0, Math.min(rows.length - 1, index));
    rows[active].classList.add('active');
    rows[active].focus({ preventScroll: true });
    rows[active].scrollIntoView({ block: 'nearest' });
  }

  function setValue(value) {
    current = value;
    const index = Math.max(0, choices.findIndex((choice) => choice.value === value));
    text.textContent = choices[index].label;
    button.title = choices[index].detail ?? '';
    rows.forEach((row, i) => {
      row.classList.toggle('current', i === index);
      row.setAttribute('aria-selected', String(i === index));
    });
  }

  const place = () => placePanel(panel, button);

  panel.addEventListener('toggle', (event) => {
    const open = event.newState === 'open';
    button.setAttribute('aria-expanded', String(open));
    if (open) {
      place();
      highlight(Math.max(0, choices.findIndex((choice) => choice.value === current)));
    }
  });
  panel.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      highlight(active + (event.key === 'ArrowDown' ? 1 : -1));
    } else if (event.key === 'Enter') {
      event.preventDefault();
      rows[active].click();
    }
  });
  window.addEventListener('resize', () => panel.matches(':popover-open') && place());

  setValue(current);
  return {
    button,
    get value() {
      return current;
    },
    setValue,
  };
}
