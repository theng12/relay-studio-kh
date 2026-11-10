// Saved Chrome account manager. Every action calls the native Relay contract; nothing is simulated here.
export const CREDITS_STALE_MS = 15 * 60 * 1000;
const ACTIVITIES = new Set(['idle', 'generating', 'cooling', 'resting', 'waiting', 'recovering']);
const LIVE_STATES = new Set(['opening', 'open', 'queued', 'closing']);

const value = (input) => {
  if (input === null || input === undefined) return null;
  const text = String(input).trim();
  return text && text.toLowerCase() !== 'unknown' ? text : null;
};
const humanize = (input) => {
  const text = String(input).replace(/[_-]+/g, ' ').trim();
  return text ? text[0].toUpperCase() + text.slice(1).toLowerCase() : text;
};
const number = (input) => {
  if (input === null || input === undefined || input === '') return null;
  const parsed = Number(input);
  return Number.isFinite(parsed) ? parsed : null;
};

export function sortProfiles(profiles = []) {
  return [...profiles].sort((left, right) => (number(left.number) ?? 0) - (number(right.number) ?? 0));
}

export function needsAttention(profile = {}) {
  return Boolean(profile.attention);
}

export function parseLabels(text) {
  return String(text ?? '').split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
}

// Ordinal numbers are immutable native values; this only resolves user input into their stable ids.
export function parseRange(text, profiles = []) {
  const raw = String(text ?? '').trim();
  if (!raw) throw new Error('Enter an account number or range, like 3 or 2-5.');
  const wanted = new Set();
  for (const token of raw.split(',')) {
    const part = token.trim();
    if (!part) continue;
    const match = part.match(/^(\d+)(?:\s*[-–]\s*(\d+))?$/);
    if (!match) throw new Error('Use whole account numbers or ranges, like 3 or 2-5.');
    const first = Number(match[1]);
    const last = match[2] === undefined ? first : Number(match[2]);
    if (last < first) throw new Error('Put the lower number first, like 2-5.');
    if (last - first > 9999) throw new Error('Use a smaller range.');
    for (let current = first; current <= last; current++) wanted.add(current);
  }
  if (!wanted.size) throw new Error('Enter an account number or range, like 3 or 2-5.');
  const matched = sortProfiles(profiles).filter((profile) => wanted.has(number(profile.number)));
  if (!matched.length) throw new Error(`No saved account matches ${raw}.`);
  return matched.map((profile) => profile.id);
}

export function connectionTier(gateway = {}) {
  const source = gateway || {};
  const connection = value(source.connection);
  const signIn = value(source.sign_in ?? source.signIn);
  const readiness = value(source.readiness);
  const detail = `Sign-in ${signIn ? humanize(signIn).toLowerCase() : 'unknown'} · Readiness ${readiness ? humanize(readiness).toLowerCase() : 'unknown'}`;
  if (value(source.error)) return { tone: 'bad', label: 'Error', detail: String(source.error) };
  if (connection === 'error') return { tone: 'bad', label: 'Error', detail };
  if (connection === 'offline') return { tone: 'warn', label: 'Offline', detail };
  if (connection === 'disconnected') return { tone: 'warn', label: 'Disconnected', detail };
  if (connection === 'paused' || source.paused === true) return { tone: 'warn', label: 'Paused', detail };
  if (connection === 'connected') return { tone: 'ok', label: 'Connected', detail };
  if (!connection) return { tone: 'off', label: 'Connection unknown', detail: signIn || readiness ? detail : 'No authoritative readback' };
  return { tone: 'off', label: humanize(connection), detail };
}

export function activityTier(gateway = {}) {
  const activity = value((gateway || {}).activity);
  if (!activity) return { tone: 'off', label: 'Activity unknown' };
  const key = activity.toLowerCase();
  if (ACTIVITIES.has(key)) return { tone: 'info', label: humanize(key) };
  if (key === 'error') return { tone: 'bad', label: 'Error' };
  return { tone: 'off', label: humanize(activity) };
}

export function processTier(profile = {}) {
  if (profile.close_pending) {
    return { tone: 'warn', label: 'Waiting to close', detail: 'Gateway has not confirmed a safe stop yet. Relay will not force it.' };
  }
  const state = value(profile.process_state) || 'closed';
  const labels = { closed: 'Closed', opening: 'Opening', open: 'Open', queued: 'Waiting for memory', closing: 'Closing', error: 'Error' };
  const tones = { open: 'ok', opening: 'info', queued: 'warn', closing: 'warn', error: 'bad', closed: 'off' };
  const detail = state === 'error' ? String(profile.error || 'Process error') : null;
  return { tone: tones[state] || 'off', label: labels[state] || humanize(state), detail };
}

const bucketValue = (item) => value(item.value ?? item.remaining ?? item.credits ?? item.count);

export function toMillis(input) {
  const text = value(input);
  if (!text) return null;
  if (/^\d+(?:\.\d+)?$/.test(text)) {
    const epoch = Number(text);
    return epoch < 100000000000 ? epoch * 1000 : epoch;
  }
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : null;
}

// Gateway sends epoch seconds; observedAt stays the authority for freshness, never the fetch time.
export function creditsView(gateway = {}, now = Date.now()) {
  const credits = (gateway || {}).credits;
  if (credits === null || credits === undefined) return { known: false, text: 'Not reported' };
  if (typeof credits !== 'object') return { known: true, balance: null, source: null, observedAt: null, resetAt: null, stale: null, buckets: [{ label: 'Reported credits', value: String(credits) }] };
  const balance = credits.balance && typeof credits.balance === 'object' ? credits.balance : null;
  const source = value(balance?.source ?? credits.source);
  const observedAt = value(balance?.observedAt ?? credits.observed_at ?? credits.observedAt);
  const resetAt = value(credits.reset_at ?? credits.resetAt);
  const raw = credits.credit_buckets ?? credits.creditBuckets ?? credits.buckets;
  const buckets = [];
  if (Array.isArray(raw)) {
    for (const item of raw) {
      if (!item || typeof item !== 'object') continue;
      const id = value(item.id ?? item.label ?? item.name);
      const amount = bucketValue(item);
      if (amount === null) continue;
      const unit = value(item.unit);
      const reset = value(item.resetAt ?? item.reset_at);
      buckets.push({
        label: id ? humanize(id) : 'Observed bucket',
        value: `${amount}${unit ? ` ${unit}` : ''}${reset ? ` · reset ${timeLabel(reset) || 'time unavailable'}` : ''}`,
      });
    }
  }
  const fresh = typeof credits.fresh === 'boolean' ? credits.fresh : null;
  const observedMs = toMillis(observedAt);
  const stale = fresh === false ? true : fresh === true ? false : observedMs === null ? null : now - observedMs > CREDITS_STALE_MS;
  const balanceView = balance && value(balance.value) !== null
    ? { value: String(balance.value), unit: value(balance.unit) || '', source: value(balance.source), observedAt: value(balance.observedAt) }
    : null;
  return { known: true, balance: balanceView, source, observedAt, resetAt, stale, fresh, buckets };
}

export function gatewaySpend(gateway = {}) {
  const source = gateway || {};
  if (source.gateway_enabled === true) return 'Gateway account spending is enabled by Gateway.';
  if (source.gateway_enabled === false) return 'Gateway account spending is disabled by Gateway.';
  return 'Not authorized. Gateway account spending stays disabled until Gateway readback.';
}

export function timeLabel(input) {
  const parsedMs = toMillis(input);
  return parsedMs === null ? null : new Date(parsedMs).toLocaleString();
}

export function pairingStatus(support = {}) {
  const source = support || {};
  const direct = source.manager_paired ?? source.gateway_manager_paired ?? source.pairing_paired;
  const nested = typeof source.pairing === 'object' ? source.pairing?.paired : source.pairing;
  return direct === true || nested === true || nested === 'paired' ? { paired: true, label: 'Paired' } : { paired: false, label: 'Not paired' };
}

export function openableIds(profiles = [], ids = []) {
  const selected = new Set(ids);
  return sortProfiles(profiles)
    .filter((profile) => selected.has(profile.id) && !profile.close_pending && !profile.desired_open && !LIVE_STATES.has(value(profile.process_state)))
    .map((profile) => profile.id);
}

export function closableIds(profiles = [], ids = []) {
  const selected = new Set(ids);
  const closable = new Set(['opening', 'open', 'queued']);
  return sortProfiles(profiles)
    .filter((profile) => {
      if (!selected.has(profile.id) || profile.close_pending) return false;
      const state = value(profile.process_state);
      return closable.has(state) || (profile.desired_open === true && state !== 'closing');
    })
    .map((profile) => profile.id);
}

export function rowAction(profile = {}) {
  if (profile.close_pending) return { kind: 'close', label: 'Waiting to close', disabled: true };
  if (value(profile.process_state) === 'closing') return { kind: 'close', label: 'Closing', disabled: true };
  if (profile.desired_open || LIVE_STATES.has(value(profile.process_state))) return { kind: 'close', label: 'Close', disabled: false };
  return { kind: 'open', label: 'Open', disabled: false };
}

function wholeNumber(raw, name, minimum) {
  const text = String(raw ?? '').trim();
  if (text === '') throw new Error(`${name} is required.`);
  const parsed = Number(text);
  if (!Number.isInteger(parsed) || parsed < minimum) throw new Error(`${name} must be a whole number of ${minimum} or more.`);
  return parsed;
}
function optionalWholeNumber(raw, name, minimum) {
  if (String(raw ?? '').trim() === '') return null;
  return wholeNumber(raw, name, minimum);
}

export function settingsPayload(fields = {}) {
  const memoryBudgetMb = optionalWholeNumber(fields.memoryBudgetMb, 'Memory budget', 1);
  const reserveMb = wholeNumber(fields.reserveMb, 'Memory reserve', 0);
  const maxActive = optionalWholeNumber(fields.maxActive, 'Maximum open accounts', 1);
  const pauseMinSeconds = wholeNumber(fields.pauseMinSeconds, 'Pause minimum', 0);
  const pauseMaxSeconds = wholeNumber(fields.pauseMaxSeconds, 'Pause maximum', 0);
  if (pauseMaxSeconds < pauseMinSeconds) throw new Error('Pause maximum must be the same as or greater than the pause minimum.');
  return { memoryBudgetMb, reserveMb, maxActive, pauseMinSeconds, pauseMaxSeconds };
}

export function mountAccounts(invoke) {
  const $ = (id) => document.getElementById(id);
  const list = $('profiles');
  const rows = new Map();
  let snapshot = null;
  let selection = new Set();
  let attentionOnly = false;
  let busy = false;
  let refreshing = false;
  const busyIds = new Set();

  const setError = (message) => {
    $('accounts-error').textContent = message ? String(message) : '';
    $('accounts-error').hidden = !message;
  };
  const setPairError = (message, secret) => {
    const text = message ? String(message).split(secret || '\u0000').join('[hidden]') : '';
    $('manager-pair-error').textContent = text;
    $('manager-pair-error').hidden = !text;
  };
  const fmt = (amount) => number(amount) === null ? 'Not reported' : new Intl.NumberFormat().format(number(amount));
  const setInput = (input, next) => {
    if (document.activeElement !== input) input.value = next === null || next === undefined ? '' : String(next);
  };

  function setControls() {
    const disabled = busy || !snapshot;
    for (const id of ['add-labels', 'add-submit', 'range-input', 'select-range', 'select-all', 'select-none']) $(id).disabled = disabled;
    $('refresh-accounts').disabled = disabled || refreshing;
  }
  function empty(message) {
    $('profiles-empty').textContent = message;
    $('profiles-empty').hidden = !message;
  }

  function createRow(profile) {
    const id = profile.id;
    const li = document.createElement('li');
    li.className = 'account';
    const row = document.createElement('div');
    row.className = 'account-row';

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

    const identity = document.createElement('div');
    identity.className = 'account-identity';
    const title = document.createElement('span');
    title.className = 'account-title';
    const ordinal = document.createElement('span');
    ordinal.className = 'ordinal';
    const label = document.createElement('span');
    title.append(ordinal, label);
    const meta = document.createElement('span');
    meta.className = 'account-meta';
    const processState = document.createElement('span');
    processState.className = 'state';
    const metaText = document.createElement('span');
    meta.append(processState, metaText);
    identity.append(title, meta);

    const status = document.createElement('div');
    status.className = 'account-status';
    const connectionState = document.createElement('span');
    connectionState.className = 'state';
    const connectionDetail = document.createElement('span');
    connectionDetail.className = 'account-meta';
    const activityState = document.createElement('span');
    activityState.className = 'state';
    status.append(connectionState, connectionDetail, activityState);

    const action = document.createElement('button');
    action.type = 'button';
    action.addEventListener('click', () => {
      const current = snapshot?.profiles?.find((entry) => entry.id === id);
      if (!current) return;
      const next = rowAction(current);
      if (next.disabled) return;
      runCommand(next.kind === 'open' ? 'manager_open_profiles' : 'manager_close_profiles', { profileIds: [id] }, [id]);
    });
    row.append(pick, identity, status, action);

    const details = document.createElement('details');
    details.className = 'account-details';
    const summary = document.createElement('summary');
    summary.textContent = 'Details';
    const dl = document.createElement('dl');
    const fields = {};
    const addField = (name, term) => {
      const dt = document.createElement('dt'); dt.textContent = term;
      const dd = document.createElement('dd'); fields[name] = dd;
      dl.append(dt, dd);
    };
    for (const [name, term] of [
      ['desired', 'Local profile'], ['process', 'Process'], ['processId', 'Process ID'], ['close', 'Close'],
      ['error', 'Error'], ['provider', 'Provider'], ['gatewayAccount', 'Gateway account'], ['gatewaySpend', 'Gateway spending'],
      ['imageWorker', 'Image worker ID'], ['videoWorker', 'Video worker ID'], ['nextEligible', 'Next eligible'],
      ['credits', 'Credits'], ['memory', 'Measured memory'],
    ]) addField(name, term);

    const form = document.createElement('form');
    form.className = 'profile-edit';
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const nextLabel = labelInput.value.trim();
      if (!nextLabel) { setError('Account label cannot be empty.'); return; }
      setError(null);
      updateProfile(id, {
        label: nextLabel,
        enabled: enabledInput.checked,
        imageWorkerId: imageInput.value.trim() || null,
        videoWorkerId: videoInput.value.trim() || null,
      });
    });
    const grid = document.createElement('div');
    grid.className = 'edit-grid';
    const makeField = (text, input) => {
      const wrap = document.createElement('label');
      wrap.className = 'field';
      wrap.textContent = text;
      wrap.append(input);
      return wrap;
    };
    const labelInput = document.createElement('input');
    labelInput.type = 'text';
    labelInput.autocomplete = 'off';
    labelInput.spellcheck = false;
    const imageInput = document.createElement('input');
    imageInput.type = 'text';
    imageInput.autocomplete = 'off';
    imageInput.spellcheck = false;
    const videoInput = document.createElement('input');
    videoInput.type = 'text';
    videoInput.autocomplete = 'off';
    videoInput.spellcheck = false;
    const enabledWrap = document.createElement('label');
    enabledWrap.className = 'check';
    const enabledInput = document.createElement('input');
    enabledInput.type = 'checkbox';
    const enabledText = document.createElement('span');
    enabledText.textContent = 'Local profile enabled';
    enabledWrap.append(enabledInput, enabledText);
    grid.append(makeField('Label', labelInput), makeField('Image worker ID', imageInput), makeField('Video worker ID', videoInput), enabledWrap);
    const footer = document.createElement('div');
    footer.className = 'edit-footer';
    const save = document.createElement('button');
    save.type = 'submit';
    save.textContent = 'Save account';
    const note = document.createElement('p');
    note.className = 'account-meta';
    note.textContent = 'Local enablement only. Gateway account enablement stays off until Gateway authorizes this worker pairing.';
    footer.append(save, note);
    form.append(grid, footer);
    details.append(summary, dl, form);

    li.append(row, details);
    return { li, checkbox, ordinal, label, metaText, processState, connectionState, connectionDetail, activityState, action, fields, labelInput, imageInput, videoInput, enabledInput };
  }

  function updateRow(refs, profile) {
    refs.checkbox.checked = selection.has(profile.id);
    refs.ordinal.textContent = `#${profile.number}`;
    refs.label.textContent = value(profile.label) || 'Untitled account';
    const process = processTier(profile);
    refs.processState.className = `state ${process.tone}`;
    refs.processState.textContent = process.label;
    const memory = profile.memory_mb === null || profile.memory_mb === undefined ? '' : ` · ${fmt(profile.memory_mb)} MB`;
    refs.metaText.textContent = `${profile.enabled ? 'Local enabled' : 'Local disabled'} · ${profile.desired_open ? 'open requested' : 'open not requested'}${memory}`;
    const connection = connectionTier(profile.gateway);
    refs.connectionState.className = `state ${connection.tone}`;
    refs.connectionState.textContent = connection.label;
    refs.connectionDetail.textContent = connection.detail;
    const activity = activityTier(profile.gateway);
    refs.activityState.className = `state ${activity.tone}`;
    refs.activityState.textContent = activity.label;
    const action = rowAction(profile);
    refs.action.textContent = action.label;
    refs.action.disabled = action.disabled || busyIds.has(profile.id) || busy;
    refs.li.hidden = attentionOnly && !needsAttention(profile);

    const gateway = profile.gateway || {};
    const credits = creditsView(gateway);
    setInput(refs.labelInput, value(profile.label) || '');
    setInput(refs.imageInput, value(gateway.image_worker_id) || '');
    setInput(refs.videoInput, value(gateway.video_worker_id) || '');
    if (document.activeElement !== refs.enabledInput) refs.enabledInput.checked = Boolean(profile.enabled);
    refs.fields.desired.textContent = profile.enabled ? 'Enabled locally; Chrome profile only.' : 'Disabled locally. The saved entry stays.';
    refs.fields.process.textContent = `${process.label}${process.detail ? ` — ${process.detail}` : ''}`;
    refs.fields.processId.textContent = value(profile.process_id) || 'Not running';
    refs.fields.close.textContent = profile.close_pending ? 'Waiting for a fresh safe-stop acknowledgement. No force close or job replay.' : 'Not requested';
    refs.fields.error.textContent = value(profile.error) || 'None reported';
    refs.fields.provider.textContent = value(gateway.provider) || 'Not reported';
    refs.fields.gatewayAccount.textContent = value(gateway.account_id) ? `Bound to ${gateway.account_id}. Enablement still follows Gateway authority.` : 'Not bound. Gateway account stays disabled until Gateway authorizes this worker pairing.';
    refs.fields.gatewaySpend.textContent = `${gatewaySpend(gateway)}${number(gateway.reserved) === null ? '' : ` Reserved: ${fmt(gateway.reserved)}.`}${number(gateway.available) === null ? '' : ` Available: ${fmt(gateway.available)}.`}`;
    refs.fields.imageWorker.textContent = value(gateway.image_worker_id) || 'Not bound';
    refs.fields.videoWorker.textContent = value(gateway.video_worker_id) || 'Not bound';
    refs.fields.nextEligible.textContent = timeLabel(gateway.next_eligible_at) || 'Not reported';
    refs.fields.credits.replaceChildren();
    if (!credits.known) {
      refs.fields.credits.textContent = credits.text;
    } else {
      const lines = [];
      lines.push(`Balance: ${credits.balance ? `${credits.balance.value}${credits.balance.unit ? ` ${credits.balance.unit}` : ''}` : 'Not reported'}`);
      lines.push(`Source: ${credits.source || 'Not reported'}`);
      lines.push(`Last checked: ${credits.observedAt ? (timeLabel(credits.observedAt) || 'Time unavailable') : 'Not reported'}`);
      if (credits.stale === true) lines.push('Stale: the latest observation is out of date');
      else if (credits.stale === null) lines.push('Freshness unknown');
      else lines.push('Freshness: recent');
      lines.push(`Reset: ${credits.resetAt ? (timeLabel(credits.resetAt) || 'Time unavailable') : 'Not reported'}`);
      for (const bucket of credits.buckets) lines.push(`${bucket.label}: ${bucket.value}`);
      if (!credits.buckets.length) lines.push('No observed credit buckets reported');
      const first = document.createElement('span');
      first.textContent = lines[0];
      refs.fields.credits.append(first);
      for (const line of lines.slice(1)) {
        const breakLine = document.createElement('br');
        refs.fields.credits.append(breakLine, line);
      }
    }
    refs.fields.memory.textContent = profile.memory_mb === null || profile.memory_mb === undefined ? 'Not measured' : `${fmt(profile.memory_mb)} MB`;
  }

  function renderList() {
    if (!snapshot) return;
    const profiles = sortProfiles(snapshot.profiles);
    const current = new Set(profiles.map((profile) => profile.id));
    for (const [id, refs] of rows) {
      if (current.has(id)) continue;
      refs.li.remove(); rows.delete(id); selection.delete(id);
    }
    let cursor = list.firstElementChild;
    for (const profile of profiles) {
      let refs = rows.get(profile.id);
      if (!refs) { refs = createRow(profile); rows.set(profile.id, refs); }
      updateRow(refs, profile);
      if (refs.li === cursor) cursor = cursor.nextElementSibling;
      else list.insertBefore(refs.li, cursor);
    }
    for (const id of selection) if (!current.has(id)) selection.delete(id);
    const visibleCount = attentionOnly ? profiles.filter(needsAttention).length : profiles.length;
    list.hidden = visibleCount === 0;
    $('account-head').hidden = visibleCount === 0;
    if (!profiles.length) empty('No saved accounts yet. Add a label above to create the first Chrome profile Relay manages.');
    else if (attentionOnly && !visibleCount) empty('No saved account needs attention.');
    else empty(null);
    renderSelection();
  }

  function renderSelection() {
    const total = snapshot?.profiles?.length ?? 0;
    const selected = [...selection];
    $('selection-state').textContent = `${selection.size} of ${total} selected`;
    const canOpen = openableIds(snapshot?.profiles, selected).length;
    const canClose = closableIds(snapshot?.profiles, selected).length;
    $('open-selected').disabled = busy || !canOpen;
    $('close-selected').disabled = busy || !canClose;
    $('open-selected').textContent = canOpen ? `Open selected (${canOpen})` : 'Open selected';
    $('close-selected').textContent = canClose ? `Close selected (${canClose})` : 'Close selected';
  }

  function renderSummary() {
    if (!snapshot) return;
    const profiles = snapshot.profiles;
    if (!profiles.length) { $('accounts-summary').textContent = 'No saved accounts yet.'; return; }
    const open = profiles.filter((profile) => ['open', 'opening'].includes(value(profile.process_state))).length;
    const queued = profiles.filter((profile) => value(profile.process_state) === 'queued').length;
    const attention = profiles.filter(needsAttention).length;
    const parts = [`${profiles.length} saved ${profiles.length === 1 ? 'account' : 'accounts'}`, `${open} open`];
    if (queued) parts.push(`${queued} waiting to open`);
    if (attention) parts.push(`${attention} need attention`);
    $('accounts-summary').textContent = parts.join(' · ');
  }

  function renderMemory() {
    if (!snapshot) return;
    const memory = snapshot.memory || {};
    const settings = snapshot.settings || {};
    const parts = [];
    parts.push(memory.used_mb === null || memory.used_mb === undefined ? 'Measured use not reported' : `Measured use ${fmt(memory.used_mb)} MB`);
    if (memory.budget_mb === null || memory.budget_mb === undefined) parts.push('budget not set');
    else parts.push(`budget ${fmt(memory.budget_mb)} MB`);
    if (number(memory.used_mb) !== null && number(memory.budget_mb) !== null) parts.push(`${fmt(Math.max(0, number(memory.budget_mb) - number(memory.used_mb)))} MB headroom`);
    if (number(memory.per_profile_mb) !== null) parts.push(`${fmt(memory.per_profile_mb)} MB per open account`);
    if (number(memory.physical_mb) !== null) parts.push(`${fmt(memory.available_mb)} MB available of ${fmt(memory.physical_mb)} MB`);
    if (number(memory.queued)) parts.push(`${fmt(memory.queued)} waiting to open${value(memory.reason) ? `: ${memory.reason}` : ''}`);
    $('memory-summary').textContent = parts.join(' · ');
    setInput($('memory-budget'), settings.memory_budget_mb ?? '');
    setInput($('memory-reserve'), settings.reserve_mb ?? '');
    setInput($('memory-max'), settings.max_active ?? '');
    setInput($('pause-min'), settings.pause_min_seconds ?? '');
    setInput($('pause-max'), settings.pause_max_seconds ?? '');
    const support = snapshot.support || {};
    if (support.pacing_applied === true) $('pacing-state').textContent = 'Pacing values are applied by Gateway.';
    else $('pacing-state').textContent = `Pacing values are a saved preference, not applied by Gateway yet.${value(support.pacing_error) ? ` ${support.pacing_error}` : ''}`;
  }

  function renderPairing() {
    const status = pairingStatus(snapshot?.support || {});
    const state = $('manager-pairing-state');
    state.textContent = status.label;
    state.className = `state ${status.paired ? 'ok' : 'off'}`;
  }

  function render(next) {
    if (!next || !Array.isArray(next.profiles)) {
      setError('Relay did not receive a usable account snapshot from this build.');
      setControls();
      return;
    }
    snapshot = next;
    setError(null);
    renderSummary();
    renderMemory();
    renderPairing();
    renderList();
    setControls();
  }

  function unavailable(error) {
    $('accounts-summary').textContent = 'Native account management is unavailable in this build.';
    setError(`Saved-account management needs the native Relay app. ${String(error)}`);
    if (!snapshot) {
      empty('Accounts cannot be listed or changed until the native manager is available.');
      for (const id of ['add-labels', 'add-submit', 'range-input', 'select-range', 'select-all', 'select-none', 'open-selected', 'close-selected']) $(id).disabled = true;
    }
  }

  async function reload() {
    refreshing = true;
    try { render(await invoke('manager_snapshot', {})); }
    catch (error) { unavailable(error); }
    finally { refreshing = false; }
  }

  async function refresh() {
    if (refreshing || busy) return;
    await reload();
    setControls();
  }

  async function runCommand(name, args, ids = []) {
    if (busy) return;
    busy = true;
    for (const id of ids) busyIds.add(id);
    setError(null);
    renderList();
    try { render(await invoke(name, args)); }
    catch (error) { setError(error); await reload(); }
    finally {
      busy = false;
      for (const id of ids) busyIds.delete(id);
      renderList();
      setControls();
    }
  }

  async function updateProfile(id, fields) {
    if (busy) return;
    busy = true; busyIds.add(id); setError(null); renderList();
    try { render(await invoke('manager_update_profile', { profileId: id, ...fields })); }
    catch (error) { setError(error); await reload(); }
    finally { busy = false; busyIds.delete(id); renderList(); setControls(); }
  }

  $('refresh-accounts').addEventListener('click', refresh);
  $('add-form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const labels = parseLabels($('add-labels').value);
    if (!labels.length) { setError('Add at least one account label.'); return; }
    busy = true; setControls(); setError(null);
    try {
      render(await invoke('manager_add_profiles', { labels }));
      $('add-labels').value = '';
    } catch (error) { setError(error); await reload(); }
    finally { busy = false; setControls(); }
  });
  $('select-range').addEventListener('click', () => {
    try { selection = new Set(parseRange($('range-input').value, snapshot?.profiles || [])); setError(null); }
    catch (error) { setError(error); return; }
    renderList();
  });
  $('select-all').addEventListener('click', () => { selection = new Set((snapshot?.profiles || []).map((profile) => profile.id)); renderList(); });
  $('select-none').addEventListener('click', () => { selection = new Set(); renderList(); });
  $('attention-only').addEventListener('change', () => { attentionOnly = $('attention-only').checked; renderList(); });
  $('open-selected').addEventListener('click', () => {
    const ids = openableIds(snapshot?.profiles || [], [...selection]);
    if (!ids.length) { setError('Selected accounts are already open, opening, waiting, or waiting to close.'); return; }
    runCommand('manager_open_profiles', { profileIds: ids }, ids);
  });
  $('close-selected').addEventListener('click', () => {
    const ids = closableIds(snapshot?.profiles || [], [...selection]);
    if (!ids.length) { setError('Selected accounts are already closed or waiting to close.'); return; }
    runCommand('manager_close_profiles', { profileIds: ids }, ids);
  });
  $('manager-settings').addEventListener('submit', async (event) => {
    event.preventDefault();
    let payload;
    try { payload = settingsPayload({ memoryBudgetMb: $('memory-budget').value, reserveMb: $('memory-reserve').value, maxActive: $('memory-max').value, pauseMinSeconds: $('pause-min').value, pauseMaxSeconds: $('pause-max').value }); }
    catch (error) { setError(error); return; }
    busy = true; setControls(); setError(null);
    try {
      render(await invoke('manager_save_settings', payload));
      $('settings-state').textContent = 'Saved. Values shown here came back from the native manager.';
    } catch (error) { setError(error); await reload(); }
    finally { busy = false; setControls(); }
  });
  $('manager-pair-form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const input = $('manager-token');
    const token = input.value.trim();
    if (!token) { setPairError('Paste the manager token first.'); return; }
    busy = true;
    $('manager-pair').disabled = true;
    setPairError(null);
    $('manager-pairing-state').textContent = 'Saving in this Mac’s Keychain…';
    try {
      render(await invoke('manager_pair_gateway', { token }));
      $('manager-pairing-note').textContent = 'Token saved in this Mac’s Keychain. Gateway account enablement stays off until Gateway authorizes these workers. The token is never shown again.';
    } catch (error) { setPairError(error, token); $('manager-pairing-note').textContent = 'Token was not saved. Nothing was stored in browser storage.'; }
    finally { input.value = ''; busy = false; $('manager-pair').disabled = false; setControls(); }
  });
  $('manager-open').addEventListener('click', () => invoke('open_gateway').catch((error) => setPairError(error)));
  $('manager-token').addEventListener('input', () => setPairError(null));

  return { refresh };
}
