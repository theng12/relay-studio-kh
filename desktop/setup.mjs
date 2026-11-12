import bundled from './extension-info.mjs';

export async function setupExtensions(invoke, showError) {
  const $ = id => document.getElementById(id);
  const list = $('extensions');
  // Chrome's own registry, read through the control pipe Relay keeps on each browser it opened.
  const check = document.createElement('button');
  check.type = 'button';
  check.className = 'ghost';
  check.textContent = 'Check loaded in open browsers';
  check.addEventListener('click', async () => {
    check.disabled = true;
    try {
      const rows = await invoke('manager_loaded_extensions');
      $('setup-state').textContent = rows.length
        ? rows.map(row => `#${row.number} ${row.label}: ${row.extensions.filter(entry => entry.enabled).map(entry => `${entry.name} ${entry.version}`).join(', ') || 'no Relay extension is loaded'}`).join(' · ')
        : 'No browser Relay opened is running.';
    } catch (error) { showError(error); }
    finally { check.disabled = false; }
  });
  list.after(check);
  // The bundled list renders even if native detection fails. Availability is never installation proof.
  function render(extensions) {
    list.replaceChildren();
    for (const extension of extensions) {
      const row = document.createElement('li');
      const title = document.createElement('h3'); title.textContent = extension.name;
      const status = document.createElement('p');
      status.textContent = extension.available
        ? `Bundled v${extension.version}. Browsers opened from Accounts load it automatically.`
        : extension.required === false
        ? `Optional, off. ${extension.reason}`
        : `Not bundled${extension.version ? ` (candidate v${extension.version})` : ''}. ${extension.reason}`;
      row.append(title, status);
      const toggle = document.createElement('label');
      toggle.className = 'check';
      const box = document.createElement('input');
      box.type = 'checkbox';
      box.checked = extension.enabled !== false;
      box.disabled = !extension.available;
      box.setAttribute('aria-label', `Load ${extension.name} in browsers Relay opens`);
      box.addEventListener('change', async () => {
        box.disabled = true;
        try {
          const next = await invoke('set_extension_enabled', { extensionId: extension.id, enabled: box.checked });
          $('setup-state').textContent = `${extension.name} will ${box.checked ? '' : 'not '}load in the next browser Relay opens.`;
          render(next.extensions);
        } catch (error) { box.checked = !box.checked; box.disabled = false; showError(error); }
      });
      const toggleText = document.createElement('span');
      toggleText.textContent = 'Load in Relay’s browsers';
      toggle.append(box, toggleText);
      row.append(toggle);
      if (extension.available) {
        const show = document.createElement('button');
        show.type = 'button';
        show.textContent = 'Show folder';
        show.addEventListener('click', async () => {
          show.disabled = true;
          try {
            const path = await invoke('prepare_extension', { extensionId: extension.id });
            await invoke('show_extension_folder', { extensionId: extension.id });
            const field = document.createElement('input');
            field.readOnly = true;
            field.value = path;
            field.setAttribute('aria-label', `${extension.name} folder`);
            const copy = document.createElement('button');
            copy.type = 'button';
            copy.className = 'ghost';
            copy.textContent = 'Copy path';
            copy.addEventListener('click', async () => {
              try { await navigator.clipboard.writeText(path); $('setup-state').textContent = 'Folder path copied.'; }
              catch { showError('Could not copy. Select and copy the path shown here.'); }
            });
            show.replaceWith(field, copy);
            $('setup-state').textContent = 'Folder shown in Finder. Browsers Relay opens load it automatically.';
          } catch (error) { showError(error); show.disabled = false; }
        });
        row.append(show);
      }
      list.append(row);
    }
  }
  render(bundled.extensions);
  try {
    const setup = await invoke('get_extension_setup');
    render(setup.extensions);
    const installed = setup.browsers.filter(browser => browser.detected);
    for (const browser of installed) {
      const option = document.createElement('option'); option.value = browser.id; option.textContent = browser.name;
      $('browser').append(option);
    }
    $('browser').disabled = installed.length === 0;
    $('open-extensions').disabled = installed.length === 0;
    function instructions() { $('browser-help').textContent = installed.find(browser => browser.id === $('browser').value)?.instructions || 'No supported browser was detected. Chrome is the first target; reopen Relay after installing it. Detection is not a compatibility check.'; }
    $('browser').addEventListener('change', instructions); instructions();
    $('open-extensions').addEventListener('click', () => invoke('open_extension_settings', { browserId: $('browser').value }).catch(showError));
    $('setup-state').textContent = 'Browser installation and worker connection are not checked. Connected does not mean provider ready.';
  } catch (error) { $('setup-state').textContent = `Browser setup could not be checked. ${String(error)}`; }
}
