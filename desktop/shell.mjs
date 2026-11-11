// Sidebar navigation for the desktop shell. Every page owns its own scroll and keeps its position.
const PAGES = ['overview', 'accounts', 'browsers', 'memory', 'gateway', 'extensions', 'settings', 'whats-new'];

export function mountShell({ onShow = () => {} } = {}) {
  const buttons = [...document.querySelectorAll('.nav-row[data-page]')];
  const pages = new Map(PAGES.map((page) => [page, document.getElementById(`page-${page}`)]));
  const notes = document.getElementById('whats-new-link');
  let current = 'overview';

  function show(page) {
    if (!pages.has(page) || page === current) return;
    current = page;
    for (const [name, node] of pages) if (node) node.hidden = name !== page;
    for (const button of buttons) {
      if (button.dataset.page === page) button.setAttribute('aria-current', 'page');
      else button.removeAttribute('aria-current');
    }
    if (page === 'whats-new') notes.setAttribute('aria-current', 'page');
    else notes.removeAttribute('aria-current');
    const heading = pages.get(page)?.querySelector('h2');
    if (heading) {
      // Move keyboard focus to the page title so the pane change is announced once.
      heading.tabIndex = -1;
      heading.focus({ preventScroll: true });
    }
    onShow(page);
  }

  for (const button of buttons) button.addEventListener('click', () => show(button.dataset.page));
  for (const [name, node] of pages) if (node) node.hidden = name !== 'overview';
  return { show, current: () => current };
}
