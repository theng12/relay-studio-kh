import info from './app-info.mjs';
import { createRecovery } from './recovery.mjs';

const $ = (id) => document.getElementById(id);
const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
const showError = (error) => { $('error').textContent = String(error); $('error').hidden = false; };
let loginChanged = false;
$('start-at-login').addEventListener('change', () => { loginChanged = true; });
const tick = createRecovery({
  probe: () => invoke('gateway_health'),
  openGateway: () => invoke('open_gateway'),
  report: (message) => { $('connection-state').textContent = message; },
});
async function loadSettings() {
  const snapshot = await invoke('get_settings');
  $('gateway-url').value = snapshot.settings.gateway_url || '';
  $('start-at-login').checked = snapshot.settings.start_at_login;
  $('start-at-login').disabled = false;
  loginChanged = false;
  $('login-state').textContent = snapshot.autostart_error || (snapshot.autostart_observed === null ? 'Start at Login has not been checked.' : `Start at Login is ${snapshot.autostart_observed ? 'on' : 'off'}.`);
}
$('settings').addEventListener('submit', async (event) => {
  event.preventDefault();
  $('save').disabled = true;
  $('error').hidden = true;
  try {
    await invoke('save_settings', { gatewayUrl: $('gateway-url').value.trim(), startAtLogin: loginChanged ? $('start-at-login').checked : null });
    await loadSettings();
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
$('quit').addEventListener('click', () => invoke('quit_app').catch(showError));
$('version').textContent = `v${info.version} · What’s New`;
for (const release of info.releases) {
  const heading = document.createElement('h3'); heading.textContent = `v${release.version}`;
  const list = document.createElement('ul');
  for (const item of release.items) { const line = document.createElement('li'); line.textContent = item; list.append(line); }
  $('notes').append(heading, list);
}
$('version').addEventListener('click', () => $('whats-new').showModal());
$('close-notes').addEventListener('click', () => $('whats-new').close());
try {
  await window.__TAURI__.event.listen('relay-error', event => showError(event.payload));
  await loadSettings();
  await tick();
  setInterval(tick, 15000);
} catch (error) {
  showError(error);
  $('save').disabled = true;
  $('connection-state').textContent = 'Saved settings could not be read. Existing choices have not been reset.';
}
