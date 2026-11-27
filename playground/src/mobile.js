// The phone layout: Code/Output tabs, a bottom bar with Run and a row of symbol keys that rides
// above the on-screen keyboard, and the options moved into the menu sheet.

import { cursorCharLeft, cursorCharRight, indentMore, undo } from '@codemirror/commands';

/** [label, what to insert or a command]. */
const KEYS = [
  ['⇥', 'indent'],
  ['(', '('],
  [')', ')'],
  ['[', '['],
  [']', ']'],
  ['{', '{'],
  ['}', '}'],
  ['?', '?'],
  ['=', '='],
  ['==', ' == '],
  ['<', '<'],
  ['>', '>'],
  ['+', '+'],
  ['-', '-'],
  ['*', '*'],
  ['/', '/'],
  ['%', '%'],
  ['..', '..'],
  ['..=', '..='],
  [':', ': '],
  [',', ', '],
  ['"', '"'],
  ["'", "'"],
  ['_', '_'],
  ['!=', ' != '],
  ['<=', ' <= '],
  ['>=', ' >= '],
  ['←', 'left'],
  ['→', 'right'],
  ['↶', 'undo'],
];

/**
 * @param {object} options
 * @param {object} options.editor            the editor API (see editor.js)
 * @param {MediaQueryList} options.query     matches on phones
 * @param {HTMLElement} options.options      the options panel (verbose, strict, solver)
 * @param {HTMLElement} options.desktopSlot  where it lives on wide screens
 * @param {HTMLElement} options.mobileSlot   where it lives on phones (in the menu sheet)
 */
export function setupMobile({ editor, query, options, desktopSlot, mobileSlot }) {
  const keys = document.getElementById('keys');
  for (const [label, action] of KEYS) {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = label;
    button.setAttribute('aria-label', action.length > 1 && /^[a-z]+$/.test(action) ? action : `insert ${label}`);
    // Pointer-down with preventDefault keeps the focus (and the keyboard) in the editor.
    button.addEventListener('pointerdown', (event) => {
      event.preventDefault();
      perform(editor, action);
    });
    keys.append(button);
  }

  // Size the app to the visual viewport, so the on-screen keyboard shrinks the editor instead of
  // covering it (iOS does not resize the layout viewport).
  const viewport = window.visualViewport;
  if (viewport) {
    const fit = () => {
      const root = document.documentElement.style;
      root.setProperty('--app-height', `${viewport.height}px`);
      root.setProperty('--app-top', `${viewport.offsetTop}px`);
      document.body.classList.toggle('keyboard-open', window.innerHeight - viewport.height > 120);
    };
    viewport.addEventListener('resize', fit);
    viewport.addEventListener('scroll', fit);
    fit();
  }

  for (const tab of document.querySelectorAll('[data-tab-target]')) {
    tab.addEventListener('click', () => showTab(tab.dataset.tabTarget));
  }
  showTab('code');

  const place = () => (query.matches ? mobileSlot : desktopSlot).append(options);
  query.addEventListener('change', place);
  place();
}

function perform(editor, action) {
  const view = editor.view;
  switch (action) {
    case 'indent':
      indentMore(view);
      break;
    case 'left':
      cursorCharLeft(view);
      break;
    case 'right':
      cursorCharRight(view);
      break;
    case 'undo':
      undo(view);
      break;
    default:
      editor.insert(action);
  }
}

/** Shows the Code or Output tab (on phones). */
export function showTab(name) {
  document.body.dataset.tab = name;
  for (const tab of document.querySelectorAll('[data-tab-target]')) {
    tab.setAttribute('aria-selected', String(tab.dataset.tabTarget === name));
  }
  if (name === 'output') document.getElementById('output-dot').hidden = true;
}

/** Flags new output when the Output tab is not showing. */
export function markOutput() {
  if (document.body.dataset.tab !== 'output') document.getElementById('output-dot').hidden = false;
}
