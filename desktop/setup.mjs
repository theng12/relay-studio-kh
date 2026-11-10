import bundled from './extension-info.mjs';

export async function setupExtensions(invoke, showError) {
  const $ = id => document.getElementById(id);
  const list = $('extensions');
  // The bundled list renders even if native detection fails. Availability is never installation proof.
  function render(extensions) {
    list.replaceChildren();
    for (const extension of extensions) {
      const row = document.createElement('li');
      const title = document.createElement('h3'); title.textContent = extension.name;
      const status = document.createElement('p');
      status.textContent = extension.available
        ? `Bundled v${extension.version}. Browser installation and connection: not checked.`
        : `Not bundled${extension.version ? ` (candidate v${extension.version})` : ''}. ${extension.reason}`;
      row.append(title, status);
      if (extension.available) {
        const prepare = document.createElement('button'); prepare.type = 'button'; prepare.textContent = 'Prepare version folder';
        prepare.addEventListener('click', async () => {
          prepare.disabled = true;
          try {
            const path = await invoke('prepare_extension', { extensionId: extension.id });
            const field = document.createElement('input'); field.readOnly = true; field.value = path; field.setAttribute('aria-label', `${extension.name} folder`);
            const show = document.createElement('button'); show.type = 'button'; show.textContent = 'Show folder';
            show.addEventListener('click', () => invoke('show_extension_folder', { extensionId: extension.id }).catch(showError));
            const copy = document.createElement('button'); copy.type = 'button'; copy.textContent = 'Copy path';
            copy.addEventListener('click', async () => { try { await navigator.clipboard.writeText(path); $('setup-state').textContent = 'Folder path copied. Browser installation is still not checked.'; } catch { showError('Could not copy. Select and copy the folder path shown here.'); } });
            prepare.replaceWith(field, show, copy);
            $('setup-state').textContent = 'Version folder prepared. Existing browser workers and previous version folders were not changed. Installation is not checked.';
          } catch (error) { showError(error); prepare.disabled = false; }
        });
        row.append(prepare);
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
