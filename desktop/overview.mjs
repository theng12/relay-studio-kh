// Overview reads the same native snapshot as Accounts; it never asks for a second data source.
import { needsAttention, processTier, connectionTier } from './accounts.mjs';

const $ = (id) => document.getElementById(id);
const number = (input) => {
  if (input === null || input === undefined || input === '') return null;
  const parsed = Number(input);
  return Number.isFinite(parsed) ? parsed : null;
};
const mb = (input) => {
  const parsed = number(input);
  return parsed === null ? null : `${Math.round(parsed).toLocaleString()} MB`;
};
const value = (input) => {
  const text = input === null || input === undefined ? '' : String(input).trim();
  return text && text.toLowerCase() !== 'unknown' ? text : null;
};

function gatewayState(snapshot) {
  const support = snapshot.support || {};
  if (support.manager_paired === false) return { label: 'Not paired', note: 'Pair a manager token in Gateway.' };
  const profiles = snapshot.profiles || [];
  const readings = profiles.map((profile) => connectionTier(profile.gateway));
  const connected = readings.filter((reading) => reading.label === 'Connected').length;
  const problem = readings.find((reading) => reading.tone === 'bad' || reading.tone === 'warn');
  if (connected) return { label: 'Connected', note: `${connected} of ${profiles.length} saved ${profiles.length === 1 ? 'account' : 'accounts'} report connected.` };
  if (problem) return { label: problem.label, note: problem.detail || 'Gateway reports a problem.' };
  if (!profiles.length) return { label: 'No accounts yet', note: 'Save an account before pairing workers.' };
  return { label: 'Not checked', note: 'Gateway has not reported a connection state yet.' };
}

function render(snapshot, show) {
  const profiles = snapshot.profiles || [];
  const open = profiles.filter((profile) => ['open', 'opening'].includes(value(profile.process_state))).length;
  const waiting = profiles.filter((profile) => ['queued', 'closing'].includes(value(profile.process_state)) || profile.close_pending).length;
  const memory = snapshot.memory || {};
  const used = mb(memory.used_mb);
  const budget = mb(memory.budget_mb ?? snapshot.settings?.memory_budget_mb);
  const available = mb(memory.available_mb);

  $('overview-summary').textContent = profiles.length
    ? `${profiles.length} saved ${profiles.length === 1 ? 'account' : 'accounts'} · ${open} open${waiting ? ` · ${waiting} waiting` : ''}`
    : 'No saved accounts yet.';
  $('overview-accounts').textContent = profiles.length ? String(profiles.length) : 'None yet';
  $('overview-accounts-note').textContent = profiles.length
    ? 'Saved in this Mac’s Relay data folder.'
    : 'Add an account to create its own fresh Chrome profile.';
  $('overview-open').textContent = String(open);
  $('overview-open-note').textContent = waiting ? `${waiting} waiting to open or close.` : 'Only Chrome windows Relay started.';
  $('overview-memory').textContent = used === null ? 'Not measured' : `${used} used`;
  $('overview-memory-note').textContent = used === null
    ? 'Memory is measured when a Chrome process is open.'
    : `${budget === null ? 'No budget set' : `Budget ${budget}`} · ${available === null ? 'available not read' : `${available} available`}`;

  const gateway = gatewayState(snapshot);
  $('overview-gateway').textContent = gateway.label;
  $('overview-gateway-note').textContent = gateway.note;

  const attention = profiles.filter(needsAttention).slice(0, 5);
  const list = $('overview-attention');
  list.replaceChildren();
  for (const profile of attention) {
    const item = document.createElement('li');
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'attention-row';
    const title = document.createElement('span');
    title.className = 'attention-title';
    title.textContent = `#${profile.number} · ${value(profile.label) || 'Untitled account'}`;
    const reason = document.createElement('span');
    reason.className = 'attention-reason';
    reason.textContent = value(profile.error) || processTier(profile).detail || 'Needs attention.';
    button.append(title, reason);
    button.addEventListener('click', () => show('accounts'));
    item.append(button);
    list.append(item);
  }
  list.hidden = attention.length === 0;
  $('overview-attention-empty').hidden = attention.length !== 0;
  if (profiles.length && !attention.length) $('overview-attention-empty').textContent = 'No saved account needs attention.';
  else if (!profiles.length) $('overview-attention-empty').textContent = 'Nothing needs attention yet.';
}

export function mountOverview(accounts, show) {
  accounts.subscribe((snapshot) => render(snapshot, show));
  $('overview-accounts-link').addEventListener('click', () => show('accounts'));
}
