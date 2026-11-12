import info from './app-info.mjs';
import { mountAccounts } from './accounts.mjs';
import { mountBrowsers } from './browsers.mjs';
import { mountOverview } from './overview.mjs';
import { mountShell } from './shell.mjs';
import { createRecovery } from './recovery.mjs';
import { setupExtensions } from './setup.mjs';

const $ = (id) => document.getElementById(id);
const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
const showError = (error) => { $('error').textContent = String(error); $('error').hidden = false; };
const accounts = mountAccounts(invoke);
mountBrowsers(invoke, accounts);
const shell = mountShell({ onShow: (page) => {
  if (page === 'accounts' || page === 'browsers') accounts.refresh();
} });
mountOverview(accounts, (page) => shell.show(page));
setupExtensions(invoke, showError);

const appearanceChoices = [...document.querySelectorAll('input[name="appearance"]')];
const APPEARANCE = new Set(['system', 'light', 'dark']);
let currentSettings = null;

// In-app only: colour-scheme is scoped to this document, never to macOS.

// Accounts layout: compact is the default, grid is the opt-in. In-app only, saved with settings.
const accountViewChoices = [...document.querySelectorAll('input[name="accounts-view"]')];
function applyAccountsView(value) {
  const choice = value === 'grid' ? 'grid' : 'compact';
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
  openGateway: () => invoke('open_gateway'),
  report: (message) => { $('connection-state').textContent = message; },
});

async function loadSettings() {
  const snapshot = await invoke('get_settings');
  currentSettings = snapshot.settings || {};
  applyAccountsView(currentSettings.accounts_view || 'compact');
  $('gateway-url').value = currentSettings.gateway_url || '';
  $('start-at-login').checked = Boolean(currentSettings.start_at_login);
  $('start-at-login').disabled = false;
  $('login-state').textContent = snapshot.autostart_error || (snapshot.autostart_observed === null ? 'Start at Login has not been checked.' : `Start at Login is ${snapshot.autostart_observed ? 'on' : 'off'}.`);
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
    await saveSettings({ gatewayUrl: $('gateway-url').value.trim() });
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

async function loadDataFolder() {
  try {
    const path = await invoke('data_folder');
    // The system path contains the bundle id; keep it out of the visible copy.
    $('data-folder').textContent = 'Relay’s saved accounts and settings live in this folder.';
    $('data-folder').dataset.path = path;
  } catch (error) {
    $('data-folder').textContent = `Relay’s data folder could not be read. ${String(error)}`;
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
  setInterval(tick, 15000);
  setInterval(() => accounts.refresh(), 5000);
} catch (error) {
  applyAppearance('system');
  showError(error);
  $('save').disabled = true;
  $('connection-state').textContent = 'Saved settings could not be read. Existing choices have not been reset.';
}
