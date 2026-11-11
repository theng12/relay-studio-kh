// Sidebar navigation for the desktop shell. Pages are plain sections; the native window decides size.
const PAGES = ['accounts', 'browsers', 'memory', 'gateway', 'extensions', 'whats-new'];

export function mountShell({ onShow = () => {} } = {}) {
  const buttons = [...document.querySelectorAll('.nav-row[data-page]')];
  const pages = new Map(PAGES.map((page) => [page, document.getElementById(`page-${page}`)]));
  let current = 'accounts';

  function show(page) {
    if (!pages.has(page) || page === current) return;
    current = page;
    for (const [name, node] of pages) if (node) node.hidden = name !== page;
    for (const button of buttons) {
      if (button.dataset.page === page) button.setAttribute('aria-current', 'page');
      else button.removeAttribute('aria-current');
    }
    const heading = pages.get(page)?.querySelector('h2');
    if (heading) {
      // Move keyboard focus to the page title so the pane change is announced once.
      heading.tabIndex = -1;
      heading.focus({ preventScroll: true });
    }
    onShow(page);
  }

  for (const button of buttons) button.addEventListener('click', () => show(button.dataset.page));
  for (const [name, node] of pages) if (node) node.hidden = name !== 'accounts';
  return { show, current: () => current };
}
