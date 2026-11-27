// The phone layout: Code/Output tabs, and a bottom bar with the run options (verbose, strict,
// solver) and Run, which rides above the on-screen keyboard.

/**
 * @param {object} options
 * @param {MediaQueryList} options.query     matches on phones
 * @param {HTMLElement} options.options      the options panel (verbose, strict, solver)
 * @param {HTMLElement} options.desktopSlot  where it lives on wide screens (the output pane)
 * @param {HTMLElement} options.mobileSlot   where it lives on phones (the bottom bar)
 */
export function setupMobile({ query, options, desktopSlot, mobileSlot }) {
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
