import info from './app-info.mjs';
import { imageCreditsPerImage, mountAccounts } from './accounts.mjs';
import { mountBrowsers } from './browsers.mjs';
import { mountOverview } from './overview.mjs';
import { mountShell } from './shell.mjs';
import { createRecovery } from './recovery.mjs';
import { setupExtensions } from './setup.mjs';

const $ = (id) => document.getElementById(id);
const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
// Failures appear where the person is looking, not only on the Accounts page.
const showError = (error) => { $('app-error').textContent = String(error); $('app-error').hidden = false; };
const accounts = mountAccounts(invoke);
mountBrowsers(invoke, accounts);
// Footer: the only external links Relay opens by itself, and live connection facts.
function renderFooterStatus(snapshot) {
  if (!snapshot || !Array.isArray(snapshot.profiles)) return;
  const support = snapshot.support || {};
  const paired = support.manager_paired === true || support.gateway_manager_paired === true
    || support.pairing_paired === true || (typeof support.pairing === 'object' && support.pairing?.paired === true);
  const connected = snapshot.profiles.filter((profile) => profile.gateway?.connection === 'connected').length;
  $('footer-status').textContent = `${paired ? 'Paired' : 'Not paired'} · ${connected} connected`;
}
accounts.subscribe(renderFooterStatus);
$('support-link').addEventListener('click', () => invoke('open_support_link').catch(showError));
$('check-updates').addEventListener('click', async () => {
  try { await invoke('open_launcher'); $('footer-status').textContent = 'Studio Launcher KH opened — updates install there, never in Relay.'; }
  catch (error) { showError(error); }
});
const shell = mountShell({ onShow: (page) => {
  // A failure belongs to the page where it happened; moving on clears it.
  $('app-error').hidden = true;
  $('app-error').textContent = '';
  if (page === 'accounts' || page === 'browsers') accounts.refresh();
  // Opening the Gateway page re-runs pairing, so a fresh binding does not wait for a restart.
  if (page === 'gateway') loadPairingStatus(true);
} });
mountOverview(accounts, (page) => shell.show(page));
setupExtensions(invoke, showError);

const appearanceChoices = [...document.querySelectorAll('input[name="appearance"]')];
const APPEARANCE = new Set(['system', 'light', 'dark']);
let currentSettings = null;

// In-app only: colour-scheme is scoped to this document, never to macOS.

// Accounts layout: compact is the default, Card is the opt-in. In-app only, saved with settings.
const accountViewChoices = [...document.querySelectorAll('input[name="accounts-view"]')];
function applyAccountsView(value) {
  // `grid` was this view's stored name before it was renamed to Card.
  // Card carries the per-account actions most clearly; Compact stays a choice.
  const choice = value === 'compact' ? 'compact' : value === 'table' ? 'table' : 'card';
  document.documentElement.dataset.accountsView = choice;
  for (const input of accountViewChoices) input.checked = input.value === choice;
}
for (const input of accountViewChoices) {
  input.addEventListener('change', async () => {
    if (!input.checked) return;
    applyAccountsView(input.value);
    try { await saveSettings({ accountsView: input.value }); }
    catch (error) {
      try { await loadSettings(); } catch { /* keep the shown choice */ }
      $('accounts-error').hidden = false;
      $('accounts-error').textContent = `The account list view was not saved. ${String(error)}`;
    }
  });
}

function applyAppearance(value) {
  // A stored `default` came from a build that offered a fourth option; it now means System.
  const choice = value === 'default' || !APPEARANCE.has(value) ? 'system' : value;
  document.documentElement.dataset.appearance = choice;
  document.documentElement.style.colorScheme = choice === 'dark' ? 'dark' : choice === 'system' ? 'light dark' : 'light';
  for (const input of appearanceChoices) input.checked = input.value === choice;
  $('appearance-state').textContent = choice === 'system'
    ? 'Following this Mac.'
    : `${choice[0].toUpperCase()}${choice.slice(1)} appearance.`;
}

const tick = createRecovery({
  probe: () => invoke('gateway_health'),
  report: (message) => { $('connection-state').textContent = message; },
});

async function loadSettings() {
  const snapshot = await invoke('get_settings');
  currentSettings = snapshot.settings || {};
  applyAccountsView(currentSettings.accounts_view || 'card');
  $('gateway-url').value = currentSettings.gateway_url || '';
  $('allow-plain-http').checked = Boolean(currentSettings.allow_plain_http);
  $('start-at-login').checked = Boolean(currentSettings.start_at_login);
  $('start-at-login').disabled = false;
  $('login-state').textContent = snapshot.autostart_error || (snapshot.autostart_observed === null ? 'Start at Login has not been checked.' : `Start at Login is ${snapshot.autostart_observed ? 'on' : 'off'}.`);
  const credits = imageCreditsPerImage(currentSettings.image_credits_per_image ?? 0);
  $('image-price').value = String(credits);
  $('image-price-state').textContent = credits === 0 ? 'Free — 0 credits per image.' : `${credits} credits per image.`;
  applyAppearance(currentSettings.appearance || 'system');
  return snapshot;
}

async function saveSettings(changes) {
  const payload = {
    gatewayUrl: currentSettings?.gateway_url || '',
    startAtLogin: null,
    appearance: currentSettings?.appearance || 'system',
    ...changes,
  };
  await invoke('save_settings', payload);
  await loadSettings();
  return payload;
}

$('settings').addEventListener('submit', async (event) => {
  event.preventDefault();
  $('save').disabled = true;
  $('error').hidden = true;
  try {
    await saveSettings({
      gatewayUrl: $('gateway-url').value.trim(),
      allowPlainHttp: $('allow-plain-http').checked,
    });
    await tick();
  } catch (error) {
    try { await loadSettings(); } catch { /* Preserve the original save error. */ }
    showError(error);
  }
  finally { $('save').disabled = false; }
});
$('reconnect').addEventListener('click', async () => {
  $('reconnect').disabled = true;
  try { await invoke('open_gateway'); await tick(); }
  catch (error) { showError(error); }
  finally { $('reconnect').disabled = false; }
});

for (const input of appearanceChoices) {
  input.addEventListener('change', async () => {
    if (!input.checked) return;
    applyAppearance(input.value);
    try { await saveSettings({ appearance: input.value }); }
    catch (error) {
      try { await loadSettings(); } catch { /* Keep the saved appearance shown. */ }
      $('appearance-state').textContent = `Appearance was not saved. ${String(error)}`;
    }
  });
}
$('start-at-login').addEventListener('change', async () => {
  try {
    await saveSettings({ startAtLogin: $('start-at-login').checked });
  } catch (error) {
    $('login-state').textContent = String(error);
    try { await loadSettings(); } catch { /* Preserve the reported failure. */ }
  }
});
// The image price saves on change, like Start at Login; it is not part of the
// Gateway form, and an invalid value normalizes to free on the next read.
$('image-price').addEventListener('change', async () => {
  try {
    await saveSettings({ imageCreditsPerImage: imageCreditsPerImage($('image-price').value) });
  } catch (error) {
    $('image-price-state').textContent = `The image price was not saved. ${String(error)}`;
    try { await loadSettings(); } catch { /* Preserve the reported failure. */ }
  }
});

const WORKER_KINDS = { image: 'Image', video: 'Video', audio: 'Audio' };
async function loadWorkerCredentials() {
  const list = $('extension-workers');
  if (!list) return;
  try {
    const rows = await invoke('extension_worker_status');
    list.replaceChildren();
    for (const row of rows) {
      const wrap = document.createElement('div');
      wrap.className = 'worker-credential';
      const title = document.createElement('strong');
      title.textContent = WORKER_KINDS[row.kind] || row.kind;
      const state = document.createElement('span');
      state.className = `state ${row.unreadable ? 'bad' : row.has_token ? 'ok' : 'off'}`;
      state.textContent = row.unreadable
        ? 'The saved worker token could not be read — save it again'
        : row.has_token ? `Saved for worker ${row.worker_id}` : 'No worker token saved';
      const worker = document.createElement('input');
      worker.type = 'text';
      worker.autocomplete = 'off';
      worker.spellcheck = false;
      worker.placeholder = 'Worker id';
      worker.value = row.worker_id || '';
      worker.setAttribute('aria-label', `${title.textContent} worker id`);
      const token = document.createElement('input');
      token.type = 'password';
      token.autocomplete = 'new-password';
      token.spellcheck = false;
      token.placeholder = 'Worker token from Gateway';
      token.setAttribute('aria-label', `${title.textContent} worker token`);
      const reveal = document.createElement('button');
      reveal.type = 'button';
      reveal.className = 'ghost';
      reveal.textContent = 'Show';
      reveal.addEventListener('click', () => {
        const shown = token.type === 'text';
        token.type = shown ? 'password' : 'text';
        reveal.textContent = shown ? 'Show' : 'Hide';
      });
      const save = document.createElement('button');
      save.type = 'button';
      save.textContent = 'Save';
      save.addEventListener('click', async () => {
        save.disabled = true;
        try {
          await invoke('save_extension_worker', { kind: row.kind, workerId: worker.value.trim(), token: token.value });
          token.value = '';
          $('extension-workers-state').textContent = `${title.textContent} worker saved in Relay's data folder. The token is never shown again.`;
          await loadWorkerCredentials();
        } catch (error) { $('extension-workers-state').textContent = String(error); }
        finally { save.disabled = false; }
      });
      const clear = document.createElement('button');
      clear.type = 'button';
      clear.className = 'ghost';
      clear.textContent = 'Remove';
      clear.addEventListener('click', async () => {
        try {
          await invoke('clear_extension_worker', { kind: row.kind });
          $('extension-workers-state').textContent = `${title.textContent} worker token removed.`;
          await loadWorkerCredentials();
        } catch (error) { $('extension-workers-state').textContent = String(error); }
      });
      wrap.append(title, state, worker, token, reveal, save, clear);
      list.append(wrap);
    }
  } catch { $('extension-workers-state').textContent = 'Worker credentials could not be read. Save them again below.'; }
}
async function loadPairingStatus(refresh = false) {
  try {
    const status = await invoke(refresh ? 'pairing_refresh' : 'pairing_status');
    $('pairing-file-state').textContent = status.automatic
      ? `Local pairing · ${status.machine_id} · ${status.url}`
      : status.note;
  } catch {
    $('pairing-file-state').textContent = 'Local pairing could not be checked. Open the Gateway page, or reopen Relay.';
  }
}
async function loadDataFolder() {
  try {
    const path = await invoke('data_folder');
    // The system path contains the bundle id; keep it out of the visible copy.
    $('data-folder').textContent = 'Relay’s saved accounts and settings live in this folder.';
    $('data-folder').dataset.path = path;
    $('show-data-folder').disabled = false;
    $('copy-data-folder').disabled = false;
  } catch {
    $('data-folder').textContent = 'Relay could not read its data folder. Try again, or quit and reopen Relay.';
  }
}
$('show-data-folder').addEventListener('click', () => invoke('open_data_folder').catch(showError));
$('copy-data-folder').addEventListener('click', async () => {
  const path = $('data-folder').dataset.path;
  if (!path) { showError('Relay’s data folder is not available yet.'); return; }
  try { await navigator.clipboard.writeText(path); $('data-folder').textContent = 'Folder path copied.'; }
  catch { showError('Could not copy. Select and copy the folder path shown here.'); }
});

$('version').textContent = `v${info.version}`;
for (const release of info.releases) {
  const heading = document.createElement('h3'); heading.textContent = `v${release.version}`;
  const list = document.createElement('ul');
  for (const item of release.items) { const line = document.createElement('li'); line.textContent = item; list.append(line); }
  $('notes').append(heading, list);
}
$('whats-new-link').addEventListener('click', () => shell.show('whats-new'));
try {
  await window.__TAURI__.event.listen('relay-error', event => showError(event.payload));
  await loadSettings();
  await tick();
  await accounts.refresh();
  await loadDataFolder();
  await loadPairingStatus();
  await loadWorkerCredentials();
  setInterval(tick, 15000);
  setInterval(() => accounts.refresh(), 1000);
} catch (error) {
  applyAppearance('system');
  showError(error);
  $('save').disabled = true;
  $('connection-state').textContent = 'Saved settings could not be read. Existing choices have not been reset.';
}
