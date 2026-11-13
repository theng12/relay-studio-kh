// Control room for the real Chrome windows Relay started. Nothing here fakes a browser.
import { sortProfiles, needsAttention, openableIds, closableIds, processTier, activityTier } from './accounts.mjs';

const LIVE = new Set(['opening', 'open', 'closing', 'queued']);
const text = (input) => {
  if (input === null || input === undefined) return null;
  const trimmed = String(input).trim();
  return trimmed && trimmed.toLowerCase() !== 'unknown' ? trimmed : null;
};
const humanize = (input) => String(input).replace(/[_-]+/g, ' ').trim().toLowerCase();

export function browserProfiles(profiles = []) {
  return sortProfiles(profiles).filter((profile) =>
    profile.process_id !== null && profile.process_id !== undefined
      || LIVE.has(text(profile.process_state))
      || Boolean(profile.close_pending));
}

export function browserSummary(profiles = []) {
  const rows = browserProfiles(profiles);
  if (!rows.length) return 'No browsers open.';
  const open = rows.filter((profile) => ['open', 'opening'].includes(text(profile.process_state))).length;
  const queued = rows.filter((profile) => text(profile.process_state) === 'queued').length;
  const closing = rows.filter((profile) => profile.close_pending || text(profile.process_state) === 'closing').length;
  const parts = [`${open} ${open === 1 ? 'browser open' : 'browsers open'}`];
  if (queued) parts.push(`${queued} waiting for memory`);
  if (closing) parts.push(`${closing} waiting to close`);
  return parts.join(' · ');
}

export function closeElapsedSeconds(profile = {}, now = Date.now()) {
  if (!profile.close_pending) return null;
  const started = Number(profile.close_requested_at);
  if (!Number.isFinite(started) || started <= 0) return null;
  return Math.max(0, Math.round(now / 1000 - started));
}

export function elapsedLabel(seconds) {
  if (seconds === null || seconds === undefined) return null;
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return `${hours} h ${minutes % 60} min`;
}

// The UI blocks the offer; the native command re-checks ownership and Gateway safety itself.
export function forceBlockReason(profile = {}) {
  if (profile.process_id === null || profile.process_id === undefined) return 'This account is not open, so there is nothing to force close.';
  const activity = humanize(text(profile.gateway?.activity) || '');
  if (['generating', 'recovering'].includes(activity)) {
    return 'Gateway reports active or uncertain work for this account. Relay will not force it.';
  }
  return null;
}

export function residentIds(profiles = [], ids = []) {
  const selected = new Set(ids);
  return browserProfiles(profiles)
    .filter((profile) => selected.has(profile.id) && profile.process_id !== null && profile.process_id !== undefined)
    .map((profile) => profile.id);
}

export function mountBrowsers(invoke, accounts) {
  const $ = (id) => document.getElementById(id);
  const list = $('browser-cards');
  const rows = new Map();
  let snapshot = null;
  let selection = new Set();
  let attentionOnly = false;
  let busy = false;
  const busyIds = new Set();

  const setError = (message) => {
    $('browsers-error').textContent = message ? String(message) : '';
    $('browsers-error').hidden = !message;
  };

  function providerLine(profile) {
    const provider = text(profile.gateway?.provider);
    return provider ? `Provider ${humanize(provider)}` : 'Provider not reported';
  }

  function createCard(profile) {
    const id = profile.id;
    const li = document.createElement('li');
    li.className = 'browser-card';

    const head = document.createElement('div');
    head.className = 'browser-head';
    const pick = document.createElement('label');
    pick.className = 'pick';
    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    checkbox.setAttribute('aria-label', `Select account ${profile.number}`);
    checkbox.addEventListener('change', () => {
      if (checkbox.checked) selection.add(id); else selection.delete(id);
      renderSelection();
    });
    pick.append(checkbox);
    const title = document.createElement('span');
    title.className = 'browser-title';
    const ordinal = document.createElement('span');
    ordinal.className = 'ordinal';
    const label = document.createElement('span');
    title.append(ordinal, label);
    const state = document.createElement('span');
    state.className = 'state';
    head.append(pick, title, state);

    // Diagnostics stay behind a visible button; the card face keeps state and reason only.
    const meta = document.createElement('p');
    meta.className = 'browser-meta';
    meta.hidden = true;

    const reason = document.createElement('p');
    reason.className = 'browser-reason';
    reason.hidden = true;

    const actions = document.createElement('div');
    actions.className = 'browser-actions';
    const focus = document.createElement('button');
    focus.type = 'button';
    focus.textContent = 'Focus';
    focus.addEventListener('click', () => runCommand('manager_show_profiles', { profileIds: [id] }, [id]));
    const close = document.createElement('button');
    close.type = 'button';
    close.textContent = 'Close';
    close.addEventListener('click', () => runCommand('manager_close_profiles', { profileIds: [id] }, [id]));
    const force = document.createElement('button');
    force.type = 'button';
    force.className = 'danger';
    force.textContent = 'Force close';
    force.addEventListener('click', () => askForce([id]));
    const details = document.createElement('button');
    details.type = 'button';
    details.className = 'ghost';
    details.textContent = 'Show details';
    details.setAttribute('aria-expanded', 'false');
    details.addEventListener('click', () => {
      meta.hidden = !meta.hidden;
      details.textContent = meta.hidden ? 'Show details' : 'Hide details';
      details.setAttribute('aria-expanded', String(!meta.hidden));
    });
    actions.append(focus, close, force, details);

    li.append(head, meta, reason, actions);
    return { li, checkbox, ordinal, label, state, meta, reason, focus, close, force };
  }

  function updateCard(refs, profile) {
    refs.checkbox.checked = selection.has(profile.id);
    refs.ordinal.textContent = `#${profile.number}`;
    refs.label.textContent = text(profile.label) || 'Untitled account';
    const tier = processTier(profile);
    refs.state.className = `state ${tier.tone}`;
    refs.state.textContent = tier.label;

    const parts = [];
    parts.push(profile.process_id === null || profile.process_id === undefined ? 'No process ID' : `PID ${profile.process_id}`);
    parts.push(profile.memory_mb === null || profile.memory_mb === undefined ? 'memory not measured' : `${profile.memory_mb} MB`);
    parts.push(providerLine(profile));
    const activity = activityTier(profile.gateway);
    parts.push(activity.label);
    refs.meta.textContent = parts.join(' · ');

    const reason = text(profile.error);
    const seconds = closeElapsedSeconds(profile);
    const elapsed = elapsedLabel(seconds);
    const blockedReason = forceBlockReason(profile);
    if (profile.close_pending) {
      refs.reason.hidden = false;
      refs.reason.textContent = seconds >= 3
        ? `Chrome did not exit${elapsed ? ` after ${elapsed}` : ''}. ${reason || 'Force close stops only this exact Chrome process.'}`
        : `Waiting to close${elapsed ? ` for ${elapsed}` : ''}. ${reason || 'Chrome is shutting down.'}`;
    } else if (reason) {
      refs.reason.hidden = false;
      refs.reason.textContent = reason;
    } else if (blockedReason) {
      // Visible on the card: a disabled button must not be the only place a refusal lives.
      refs.reason.hidden = false;
      refs.reason.textContent = blockedReason;
    } else {
      refs.reason.hidden = true;
      refs.reason.textContent = '';
    }

    const disabled = busy || busyIds.has(profile.id);
    refs.focus.disabled = disabled || !['open', 'opening'].includes(text(profile.process_state));
    refs.close.disabled = disabled || Boolean(profile.close_pending) || !['open', 'opening', 'queued'].includes(text(profile.process_state));
    // The dialog names the reason; the native command re-checks ownership and Gateway safety.
    const waitingToExit = profile.close_pending && (seconds ?? 0) < 3;
    const blocked = forceBlockReason(profile);
    // An unavailable destructive action is not offered at all; the reason is on the card.
    refs.force.disabled = disabled || profile.process_id === null || profile.process_id === undefined || waitingToExit || Boolean(blocked);
    refs.force.title = blocked || (waitingToExit ? 'Force close is available after 3 seconds.' : '');
  }

  function paint() {
    if (!snapshot?.profiles) return;
    const profiles = browserProfiles(snapshot.profiles).filter((profile) => !attentionOnly || needsAttention(profile));
    const current = new Set(profiles.map((profile) => profile.id));
    for (const [id, refs] of rows) {
      if (current.has(id)) continue;
      refs.li.remove(); rows.delete(id); selection.delete(id);
    }
    let cursor = list.firstElementChild;
    for (const profile of profiles) {
      let refs = rows.get(profile.id);
      if (!refs) { refs = createCard(profile); rows.set(profile.id, refs); }
      updateCard(refs, profile);
      if (refs.li === cursor) cursor = cursor.nextElementSibling;
      else list.insertBefore(refs.li, cursor);
    }
    list.hidden = profiles.length === 0;
    $('browser-empty').hidden = profiles.length !== 0;
    $('browser-empty').textContent = snapshot.profiles.length && attentionOnly
      ? 'No open browser needs attention.'
      : 'No browsers open.';
    $('browsers-summary').textContent = browserSummary(snapshot.profiles);
    renderSelection();
  }

  function renderSelection() {
    const ids = [...selection];
    const blocked = ids.map((id) => snapshot?.profiles?.find((entry) => entry.id === id)).find((profile) => profile && forceBlockReason(profile));
    $('browser-selection').textContent = `${ids.length} selected${blocked ? ` · Force close refused: ${forceBlockReason(blocked)}` : ''}`;
    const canOpen = openableIds(snapshot?.profiles || [], ids).length;
    const canClose = closableIds(snapshot?.profiles || [], ids).filter((id) => {
      const profile = snapshot.profiles.find((entry) => entry.id === id);
      return profile && !profile.close_pending;
    }).length;
    const canForce = residentIds(snapshot?.profiles || [], ids).filter((id) => {
      const profile = snapshot.profiles.find((entry) => entry.id === id);
      // A blocked account is not offered at all, in bulk exactly as on its card.
      return profile && (!profile.close_pending || (closeElapsedSeconds(profile) ?? 0) >= 3) && !forceBlockReason(profile);
    }).length;
    $('browser-open').disabled = busy || !canOpen;
    $('browser-close').disabled = busy || !canClose;
    $('browser-force').disabled = busy || !canForce;
    $('browser-open').textContent = canOpen ? `Open selected (${canOpen})` : 'Open selected';
    $('browser-close').textContent = canClose ? `Close selected (${canClose})` : 'Close selected';
    $('browser-force').textContent = canForce ? `Force close selected (${canForce})` : 'Force close selected';
  }

  function render(next) {
    if (!next?.profiles) return;
    snapshot = next;
    render();
  }

  async function runCommand(name, args, ids = []) {
    if (busy) return;
    busy = true;
    for (const id of ids) busyIds.add(id);
    setError(null);
    paint();
    try { render(await invoke(name, args)); }
    catch (error) { setError(error); }
    finally {
      busy = false;
      for (const id of ids) busyIds.delete(id);
      paint();
    }
  }

  function askForce(ids) {
    const profiles = (snapshot?.profiles || []).filter((profile) => ids.includes(profile.id));
    const blocked = profiles.find((profile) => forceBlockReason(profile));
    const dialog = $('force-close');
    $('force-blocked').hidden = !blocked;
    $('force-blocked').textContent = blocked ? forceBlockReason(blocked) : '';
    $('force-confirm').disabled = Boolean(blocked) || !profiles.length;
    $('force-body').textContent = profiles.length === 1
      ? `Open tabs in ${text(profiles[0].label) || `account #${profiles[0].number}`} will be lost. Chrome can restore the session the next time it starts. Relay stops only the exact Chrome process it opened.`
      : `Open tabs in ${profiles.length} browsers will be lost. Chrome can restore each session the next time it starts. Relay stops only the exact Chrome processes it opened.`;
    dialog.dataset.ids = JSON.stringify(ids);
    dialog.showModal();
  }

  $('force-cancel').addEventListener('click', () => $('force-close').close());
  $('force-confirm').addEventListener('click', () => {
    const ids = JSON.parse($('force-close').dataset.ids || '[]');
    $('force-close').close();
    if (ids.length) runCommand('manager_force_close_profiles', { profileIds: ids }, ids);
  });
  $('browser-attention-only').addEventListener('change', () => {
    attentionOnly = $('browser-attention-only').checked;
    paint();
  });
  $('browser-open').addEventListener('click', () => {
    const ids = openableIds(snapshot?.profiles || [], [...selection]);
    if (!ids.length) { setError('Selected accounts are already open, opening, waiting, or waiting to close.'); return; }
    runCommand('manager_open_profiles', { profileIds: ids }, ids);
  });
  $('browser-close').addEventListener('click', () => {
    const ids = closableIds(snapshot?.profiles || [], [...selection]).filter((id) => {
      const profile = snapshot.profiles.find((entry) => entry.id === id);
      return profile && !profile.close_pending;
    });
    if (!ids.length) { setError('Selected accounts are already closed or waiting to close.'); return; }
    runCommand('manager_close_profiles', { profileIds: ids }, ids);
  });
  $('browser-force').addEventListener('click', () => {
    const ids = residentIds(snapshot?.profiles || [], [...selection]);
    if (!ids.length) { setError('Select an open browser first.'); return; }
    askForce(ids);
  });

  // The close age ticks while a card is waiting; closing rows refresh every 500 ms until they settle.
  setInterval(() => { if (!document.getElementById('page-browsers').hidden) paint(); }, 1000);
  setInterval(() => {
    if (!document.getElementById('page-browsers').hidden && snapshot?.profiles?.some((profile) => profile.close_pending)) accounts.refresh();
  }, 500);

  accounts.subscribe((next) => render(next));
  return { refresh: () => accounts.refresh() };
}
