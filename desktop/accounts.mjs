// Saved Chrome account manager. Every action calls the native Relay contract; nothing is simulated here.
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
  const ranges = [];
  for (const token of raw.split(',')) {
    const part = token.trim();
    if (!part) continue;
    const match = part.match(/^(\d+)(?:\s*[-–]\s*(\d+))?$/);
    if (!match) throw new Error('Use whole account numbers or ranges, like 3 or 2-5.');
    const first = Number(match[1]);
    const last = match[2] === undefined ? first : Number(match[2]);
    if (!Number.isSafeInteger(first) || !Number.isSafeInteger(last) || first < 1) throw new Error('Account numbers must be safe positive whole numbers.');
    if (last < first) throw new Error('Put the lower number first, like 2-5.');
    ranges.push({ first, last });
  }
  if (!ranges.length) throw new Error('Enter an account number or range, like 3 or 2-5.');
  const matched = sortProfiles(profiles).filter((profile) => {
    const ordinal = Number(profile.number);
    return Number.isSafeInteger(ordinal) && ranges.some((range) => ordinal >= range.first && ordinal <= range.last);
  });
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
    return { tone: 'warn', label: 'Waiting to close', detail: 'Chrome has not exited yet. Force close becomes available after 3 seconds.' };
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
  const observedAt = value(balance?.observed_at ?? balance?.observedAt ?? credits.observed_at ?? credits.observedAt);
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
      const reset = value(item.reset_at ?? item.resetAt);
      buckets.push({
        label: id ? humanize(id) : 'Observed bucket',
        value: `${amount}${unit ? ` ${unit}` : ''}${reset ? ` · reset ${timeLabel(reset) || 'time unavailable'}` : ''}`,
      });
    }
  }
  const fresh = typeof credits.fresh === 'boolean' ? credits.fresh : null;
  const observedMs = toMillis(observedAt);
  // Gateway is the freshness authority. No local age window; a future or invalid observation is never recent.
  const timeUsable = observedMs !== null && observedMs <= now;
  const stale = fresh === false ? true : fresh === true && timeUsable ? false : null;
  const balanceView = balance && value(balance.value) !== null
    ? { value: String(balance.value), unit: value(balance.unit) || '', source: value(balance.source), observedAt: value(balance.observed_at ?? balance.observedAt) }
    : null;
  return { known: true, balance: balanceView, source, observedAt, resetAt, stale, fresh, buckets };
}

export function creditsLine(gateway = {}, now = Date.now()) {
  const credits = creditsView(gateway, now);
  if (!credits.known) return 'Credits not reported';
  if (!credits.balance) return 'Balance not reported';
  const amount = `${credits.balance.value}${credits.balance.unit ? ` ${credits.balance.unit}` : ''}`;
  if (credits.stale === true) return `${amount} · stale`;
  if (credits.stale === null) return `${amount} · freshness unknown`;
  return `${amount} · fresh`;
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

// Show only targets resident profiles; queued accounts still obey memory admission.
export function showAction(profile = {}) {
  const state = value(profile.process_state);
  const visible = needsAttention(profile) && ['open', 'opening'].includes(state);
  return { visible, disabled: Boolean(profile.close_pending) || state === 'closing', label: 'Show profile' };
}

export function profileUpdatePayload({ label, enabled, imageWorkerId, videoWorkerId } = {}) {
  return {
    label: String(label ?? '').trim(),
    enabled: Boolean(enabled),
    // Empty strings intentionally clear a manual binding; native treats absent arguments as unchanged.
    imageWorkerId: String(imageWorkerId ?? '').trim(),
    videoWorkerId: String(videoWorkerId ?? '').trim(),
  };
}

// The label is the only field Relay autosaves, so it never carries enablement or worker bindings.
export function labelAutosavePayload(label) {
  return { label: String(label ?? '').trim() };
}

export const LABEL_AUTOSAVE_MS = 600;
const LABEL_RETRY_MS = 400;

const LABEL_STATUS = {
  pending: { tone: 'info', text: 'Waiting to save…' },
  saving: { tone: 'info', text: 'Saving…' },
  saved: { tone: 'ok', text: 'Label saved' },
  failed: { tone: 'bad', text: 'Label not saved' },
  invalid: { tone: 'bad', text: 'A label cannot be empty' },
  waiting: { tone: 'warn', text: 'Waiting for the current command…' },
};

// One shared timer for the whole list: a bounded debounce, one write in flight per account and the newest draft wins.
export function createLabelAutosave({ invoke, ready = () => true, delay = LABEL_AUTOSAVE_MS, setTimer = setTimeout, clearTimer = clearTimeout, now = () => Date.now(), onChange = () => {} } = {}) {
  const rows = new Map();
  let timer = null;
  let timerDue = Infinity;
  let revision = 0;

  const state = (id) => {
    const row = rows.get(id);
    return row ? { status: row.status, draft: row.draft, error: row.error, saved: row.saved } : { status: 'idle', draft: null, error: null, saved: null };
  };
  const rowFor = (id) => {
    let row = rows.get(id);
    if (!row) { row = { draft: null, due: null, sending: false, composing: false, status: 'idle', error: null, saved: null, savedAt: 0, wait: LABEL_RETRY_MS, waiters: [] }; rows.set(id, row); }
    return row;
  };
  const arm = (when) => {
    if (timer !== null) { if (timerDue <= when) return; clearTimer(timer); timer = null; }
    timerDue = when;
    timer = setTimer(() => { timer = null; timerDue = Infinity; flush(); }, Math.max(0, when - now()));
  };
  const flush = () => {
    const time = now();
    let next = Infinity;
    for (const [id, row] of rows) {
      if (row.sending || row.due === null) continue;
      if (row.due <= time) { row.due = null; write(id, row); }
      else if (row.due < next) next = row.due;
    }
    if (next !== Infinity) arm(next);
  };
  const write = async (id, row) => {
    if (row.sending || row.composing) return;
    const text = row.draft;
    if (text === null) return;
    if (!String(text).trim()) { row.status = 'invalid'; onChange(id, state(id)); return; }
    // Another native command owns the manager right now; the edit waits instead of racing it.
    if (!ready()) {
      row.wait = Math.min(row.wait * 2, 2000);
      row.due = now() + row.wait;
      arm(row.due);
      row.status = 'waiting';
      onChange(id, state(id));
      return;
    }
    row.sending = true;
    row.status = 'saving';
    onChange(id, state(id));
    let failure = null;
    try { await invoke('manager_update_profile', { profileId: id, ...labelAutosavePayload(text) }); }
    catch (error) { failure = String(error?.message ?? error); }
    row.sending = false;
    for (const waiter of row.waiters.splice(0)) waiter();
    if (failure === null) { row.saved = String(text).trim(); row.savedAt = ++revision; }
    if (row.composing) { row.status = 'pending'; row.due = null; }
    // A newer edit still gets its own attempt even when the older write failed.
    else if (row.draft !== null && row.draft !== text) {
      row.status = 'pending'; row.error = null; row.due ??= now(); arm(row.due);
    }
    else if (failure !== null) { row.status = 'failed'; row.error = failure; }
    else { row.draft = null; row.status = 'saved'; row.error = null; }
    onChange(id, state(id));
  };

  // The typed value becomes the pending draft and the write is scheduled after the debounce.
  const edit = (id, text) => {
    const row = rowFor(id);
    row.draft = String(text ?? '');
    row.status = 'pending';
    row.error = null;
    row.wait = LABEL_RETRY_MS;
    row.composing = false;
    row.due = now() + delay;
    arm(row.due);
    onChange(id, state(id));
  };
  // Mid-composition text is kept as the draft but never written before the composition ends.
  const hold = (id, text) => {
    const row = rowFor(id);
    row.draft = String(text ?? '');
    row.composing = true;
    row.due = null;
    row.status = 'pending';
    onChange(id, state(id));
  };
  // The explicit account save carries the label itself, so the scheduled write is dropped but the draft is kept.
  const cancel = (id) => {
    const row = rows.get(id);
    if (row) row.due = null;
  };
  const settled = (id, text) => {
    const row = rows.get(id);
    if (!row) return;
    row.saved = String(text ?? '').trim(); row.savedAt = ++revision;
    if (!row.composing && (row.draft === null || row.draft.trim() === row.saved)) {
      row.draft = null; row.due = null; row.status = 'saved'; row.error = null;
    } else if (!row.composing) { retry(id); }
    onChange(id, state(id));
  };
  const retry = (id) => {
    const row = rows.get(id);
    if (!row || row.draft === null || row.composing) return;
    row.status = 'pending';
    row.due = now() + delay;
    arm(row.due);
    onChange(id, state(id));
  };
  const idle = (id) => {
    const row = rows.get(id);
    if (!row || !row.sending) return Promise.resolve();
    return new Promise((resolve) => row.waiters.push(resolve));
  };
  const forget = (id) => { rows.delete(id); };

  // Only a snapshot requested after the save acknowledgement may replace its saved label.
  const observe = (profiles, requestedAt) => {
    for (const profile of profiles || []) {
      const row = rows.get(profile.id);
      if (row && requestedAt >= row.savedAt) row.saved = null;
    }
  };
  const label = (id, observed) => {
    const row = rows.get(id);
    return row?.draft ?? row?.saved ?? observed;
  };
  return { edit, hold, cancel, settled, retry, idle, forget, state, observe, label, revision: () => revision };
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

function pauseSeconds(raw, name) {
  const text = String(raw ?? '').trim();
  if (!text) throw new Error(`${name} is required.`);
  const parsed = Number(text);
  if (!Number.isFinite(parsed) || parsed < 0) throw new Error(`${name} must be zero or more seconds.`);
  return parsed;
}

export function settingsPayload(fields = {}) {
  const memoryBudgetMb = optionalWholeNumber(fields.memoryBudgetMb, 'Memory budget', 1);
  const reserveMb = wholeNumber(fields.reserveMb, 'Memory reserve', 0);
  const maxActive = optionalWholeNumber(fields.maxActive, 'Maximum open accounts', 1);
  const pauseMinSeconds = pauseSeconds(fields.pauseMinSeconds, 'Pause minimum');
  const pauseMaxSeconds = pauseSeconds(fields.pauseMaxSeconds, 'Pause maximum');
  if (pauseMaxSeconds < pauseMinSeconds) throw new Error('Pause maximum must be the same as or greater than the pause minimum.');
  return { memoryBudgetMb, reserveMb, maxActive, pauseMinSeconds, pauseMaxSeconds };
}

const formatAmount = (amount) => number(amount) === null ? 'Not reported' : new Intl.NumberFormat().format(number(amount));

export function memorySummary(memory = {}) {
  const used = number(memory.used_mb);
  const budget = number(memory.budget_mb);
  const available = number(memory.available_mb);
  const physical = number(memory.physical_mb);
  const parts = [used === null ? 'Measured use not reported'
    : used === 0 ? 'Nothing opened yet — no account measured this session'
    : `Measured use ${formatAmount(used)} MB`];
  parts.push(budget === null ? 'budget not set' : `budget ${formatAmount(budget)} MB`);
  if (used !== null && budget !== null && used > 0) parts.push(`${formatAmount(budget - used)} MB headroom`);
  const reserve = number(memory.reserve_mb);
  const perProfile = number(memory.per_profile_mb);
  if (perProfile !== null) parts.push(`${formatAmount(perProfile)} MB per open account`);
  // Plain arithmetic, so the page answers "how much is there and how many fit" without the
  // reader doing subtraction in their head.
  if (available !== null && reserve !== null) {
    const room = available - reserve;
    parts.push(`room ${formatAmount(Math.max(0, room))} MB after the ${formatAmount(reserve)} MB reserve`);
    if (perProfile) parts.push(`about ${Math.floor(Math.max(0, room) / perProfile)} more account(s) fit at ${formatAmount(perProfile)} MB each`);
  }
  if (used === null || used === 0) parts.push('Measured use is not a cap: the first opened account is measured, and that figure governs the next');
  if (physical !== null) parts.push(available === null ? `${formatAmount(physical)} MB physical memory; available not reported` : `${formatAmount(available)} MB available of ${formatAmount(physical)} MB physical`);
  if (number(memory.queued)) parts.push(`${formatAmount(memory.queued)} waiting to open${value(memory.reason) ? `: ${memory.reason}` : ''}`);
  return parts.join(' · ');
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
  let deletingId = null;
  const busyIds = new Set();
  // A refused delete stays on its own row until the next attempt, not just in the page banner.
  const rowErrors = new Map();
  const listeners = new Set();
  const dirtyForms = new WeakSet();
  const markDirty = (event) => { if (event.target.form) dirtyForms.add(event.target.form); };
  list.addEventListener('input', markDirty);
  $('manager-settings').addEventListener('input', markDirty);

  const autosave = createLabelAutosave({
    invoke,
    ready: () => !busy,
    onChange: (id, next) => {
      const refs = rows.get(id);
      if (!refs) return;
      renderLabelState(refs, next);
      // The row title follows the edit at once instead of waiting for the next read.
      const shown = next.draft !== null ? (String(next.draft).trim() || 'Untitled account') : (next.saved || '');
      if (shown) refs.label.textContent = shown;
    },
  });

  async function readManager(name, args) {
    const requestedAt = autosave.revision();
    const next = await invoke(name, args);
    autosave.observe(next?.profiles, requestedAt);
    return next;
  }

  function renderLabelState(refs, next) {
    const view = LABEL_STATUS[next.status];
    refs.labelStatus.textContent = !view ? '' : (next.status === 'failed' && next.error ? `${view.text}: ${next.error}` : view.text);
    refs.labelStatus.className = view ? `state ${view.tone}` : 'state';
    refs.labelStatus.hidden = !view;
  }

  const setError = (message) => {
    $('accounts-error').textContent = message ? String(message) : '';
    $('accounts-error').hidden = !message;
  };
  const setPairError = (message, secret) => {
    const text = message ? String(message).split(secret || '\u0000').join('[hidden]') : '';
    $('manager-pair-error').textContent = text;
    $('manager-pair-error').hidden = !text;
  };
  const fmt = formatAmount;
  const setInput = (input, next) => {
    if (document.activeElement !== input && !dirtyForms.has(input.form)) input.value = next === null || next === undefined ? '' : String(next);
  };

  function setControls() {
    const disabled = busy || !snapshot;
    for (const id of ['add-labels', 'add-submit', 'range-input', 'select-range', 'select-all', 'select-none']) $(id).disabled = disabled;
  }
  function empty(message) {
    $('profiles-empty').textContent = message;
    $('profiles-empty').hidden = !message;
  }

  // A web confirm dialog never appears in this app's webview, so it silently cancelled
  // Delete. The confirmation is Relay's own dialog, like Force close.
  function askDelete(id) {
    const current = snapshot?.profiles?.find((entry) => entry.id === id);
    if (!current || busy || busyIds.has(id)) return;
    deletingId = id;
    $('delete-body').textContent = `Account ${current.number} — ${current.label || 'Untitled account'} will be deleted with its Chrome profile.`;
    $('delete-dialog').showModal();
  }

  async function deleteAccount(id) {
    if (busy) return;
    // A list read already in flight must not land a pre-delete snapshot on top of the delete.
    busy = true;
    setControls();
    const refs = rows.get(id);
    const account = snapshot?.profiles?.find((entry) => entry.id === id);
    if (refs) refs.remove.disabled = true;
    rowErrors.delete(id);
    renderList();
    try {
      const next = await invoke('manager_delete_profile', { profileId: id });
      selection.delete(id);
      render(next);
      const named = account ? `Account ${account.number} — ${account.label || 'Untitled account'}` : 'Account';
      $('accounts-error').hidden = false;
      $('accounts-error').textContent = `${named} deleted.`;
    } catch (error) {
      rowErrors.set(id, String(error?.message ?? error));
      // Re-read so a stale list, or an account that no longer exists, cannot outlive the refusal.
      try { render(await readManager('manager_snapshot', {})); }
      catch { renderList(); }
    } finally {
      if (refs) refs.remove.disabled = false;
      deletingId = null;
      busy = false;
      renderList();
      setControls();
    }
  }

  function startRename(id, labelEl, titleEl) {
    if (busy) return;
    const current = snapshot?.profiles?.find((entry) => entry.id === id);
    if (!current) return;
    const input = document.createElement('input');
    input.type = 'text';
    input.className = 'rename-input';
    input.value = current.label;
    input.setAttribute('aria-label', `Rename account ${current.number}`);
    let settled = false;
    const finish = async (save) => {
      // Escape removes the input, which fires blur; that must never turn into a save.
      if (settled) return;
      settled = true;
      const value = input.value.trim();
      if (input.isConnected) input.replaceWith(labelEl);
      if (!save || !value || value === current.label) return;
      await runCommand('manager_update_profile', { profileId: id, label: value }, [id]);
    };
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') { event.preventDefault(); finish(true); }
      if (event.key === 'Escape') { event.preventDefault(); finish(false); }
    });
    input.addEventListener('blur', () => finish(true));
    labelEl.replaceWith(input);
    input.focus();
    input.select();
    void titleEl;
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
    const label = document.createElement('button');
    label.type = 'button';
    label.className = 'account-label';
    label.title = 'Rename this account';
    label.addEventListener('click', () => startRename(id, label, title));
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
    const creditsRow = document.createElement('span');
    creditsRow.className = 'account-credits';
    status.append(connectionState, connectionDetail, activityState, creditsRow);

    const action = document.createElement('button');
    action.type = 'button';
    action.addEventListener('click', () => {
      const current = snapshot?.profiles?.find((entry) => entry.id === id);
      if (!current) return;
      const next = rowAction(current);
      if (next.disabled) return;
      runCommand(next.kind === 'open' ? 'manager_open_profiles' : 'manager_close_profiles', { profileIds: [id] }, [id]);
    });
    const show = document.createElement('button');
    show.type = 'button';
    show.textContent = 'Show profile';
    show.addEventListener('click', () => runCommand('manager_show_profiles', { profileIds: [id] }, [id]));
    const remove = document.createElement('button');
    remove.type = 'button';
    remove.className = 'ghost danger';
    remove.textContent = 'Delete';
    remove.addEventListener('click', () => askDelete(id));
    const actionCell = document.createElement('div');
    actionCell.className = 'account-actions';
    row.append(pick, identity, status, actionCell);

    const message = document.createElement('p');
    message.className = 'state bad';
    message.setAttribute('role', 'alert');
    message.hidden = true;

    // A visible text button owns the disclosure; the native triangle is banned (owner, 2026-11-12).
    const details = document.createElement('div');
    details.className = 'account-details';
    details.id = `account-details-${id}`;
    details.hidden = true;
    const detailsButton = document.createElement('button');
    detailsButton.type = 'button';
    detailsButton.textContent = 'Show details';
    detailsButton.setAttribute('aria-expanded', 'false');
    detailsButton.setAttribute('aria-controls', details.id);
    detailsButton.addEventListener('click', () => {
      details.hidden = !details.hidden;
      detailsButton.textContent = details.hidden ? 'Show details' : 'Hide details';
      detailsButton.setAttribute('aria-expanded', String(!details.hidden));
    });
    actionCell.append(action, show, detailsButton, remove);
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
      ['credits', 'Credits'], ['memory', 'Measured memory'], ['seeding', 'Extension seeding'],
    ]) addField(name, term);

    const form = document.createElement('form');
    form.className = 'profile-edit';
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const nextLabel = labelInput.value.trim();
      if (!nextLabel) { setError('Account label cannot be empty.'); return; }
      setError(null);
      updateProfile(id, profileUpdatePayload({
        label: labelInput.value,
        enabled: enabledInput.checked,
        imageWorkerId: imageInput.value,
        videoWorkerId: videoInput.value,
      }), form);
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
    // Only the label autosaves, and never mid-composition.
    labelInput.addEventListener('input', (event) => {
      if (event.isComposing) { autosave.hold(id, labelInput.value); return; }
      autosave.edit(id, labelInput.value);
    });
    labelInput.addEventListener('compositionend', () => autosave.edit(id, labelInput.value));
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
    const labelStatus = document.createElement('span');
    labelStatus.className = 'state';
    labelStatus.setAttribute('role', 'status');
    labelStatus.hidden = true;
    const note = document.createElement('p');
    note.className = 'account-meta';
    note.textContent = 'Local enablement only. Gateway account enablement stays off until Gateway authorizes this worker pairing.';
    footer.append(save, labelStatus, note);
    form.append(grid, footer);
    details.append(dl, form);

    li.append(row, message, details);
    return { li, checkbox, ordinal, label, metaText, processState, connectionState, connectionDetail, activityState, creditsRow, action, show, remove, message, fields, labelInput, labelStatus, imageInput, videoInput, enabledInput };
  }

  function updateRow(refs, profile) {
    refs.checkbox.checked = selection.has(profile.id);
    refs.ordinal.textContent = `#${profile.number}`;
    const draft = autosave.state(profile.id).draft;
    const label = autosave.label(profile.id, value(profile.label) || '');
    refs.label.textContent = String(label).trim() || 'Untitled account';
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
    refs.creditsRow.textContent = creditsLine(profile.gateway);
    const action = rowAction(profile);
    refs.action.textContent = action.label;
    refs.action.setAttribute('aria-label', `${action.label} account #${profile.number} ${label}`);
    refs.action.disabled = action.disabled || busyIds.has(profile.id) || busy;
    const show = showAction(profile);
    refs.show.setAttribute('aria-label', `Show profile for account #${profile.number} ${label}`);
    refs.show.hidden = !show.visible;
    refs.show.disabled = show.disabled || busyIds.has(profile.id) || busy;
    const refusal = rowErrors.get(profile.id);
    refs.message.textContent = refusal ? `Delete failed: ${refusal}` : '';
    refs.message.hidden = !refusal;
    refs.remove.disabled = busy;
    refs.li.hidden = attentionOnly && !needsAttention(profile);

    const gateway = profile.gateway || {};
    const credits = creditsView(gateway);
    // A pending draft and a focused caret both outrank the snapshot value.
    if (draft === null && document.activeElement !== refs.labelInput) refs.labelInput.value = label;
    setInput(refs.imageInput, value(gateway.image_worker_id) || '');
    setInput(refs.videoInput, value(gateway.video_worker_id) || '');
    if (document.activeElement !== refs.enabledInput && !dirtyForms.has(refs.enabledInput.form)) refs.enabledInput.checked = Boolean(profile.enabled);
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
    refs.fields.seeding.textContent = value(profile.seeding) || 'Not checked yet — open this account from Relay to seed its extensions.';
  }

  function renderList() {
    if (!snapshot) return;
    const profiles = sortProfiles(snapshot.profiles);
    const current = new Set(profiles.map((profile) => profile.id));
    for (const [id, refs] of rows) {
      if (current.has(id)) continue;
      refs.li.remove(); rows.delete(id); selection.delete(id); autosave.forget(id); rowErrors.delete(id);
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
    if (attention) parts.push(`${attention} ${attention === 1 ? 'needs' : 'need'} attention`);
    $('accounts-summary').textContent = parts.join(' · ');
  }

  function renderMemory() {
    if (!snapshot) return;
    const settings = snapshot.settings || {};
    const summary = memorySummary(snapshot.memory || {});
    $('memory-summary').textContent = summary;
    $('memory-summary-page').textContent = summary;
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
    renderSummary();
    renderMemory();
    renderPairing();
    renderList();
    setControls();
    // Other panes (Browsers) read the same snapshot instead of asking the native manager again.
    for (const notify of listeners) notify(next);
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
    try { render(await readManager('manager_snapshot', {})); }
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
    try { render(await readManager(name, args)); }
    catch (error) { await reload(); setError(error); }
    finally {
      busy = false;
      for (const id of ids) busyIds.delete(id);
      renderList();
      setControls();
    }
  }

  async function updateProfile(id, fields, form) {
    if (busy || busyIds.has(id)) return;
    busyIds.add(id);
    // An in-flight label write settles first so it cannot land after this save.
    await autosave.idle(id);
    if (busy) { busyIds.delete(id); renderList(); return; }
    // The explicit save carries the label itself, so it replaces the scheduled label write.
    autosave.cancel(id);
    busy = true; setError(null); renderList();
    try {
      const next = await readManager('manager_update_profile', { profileId: id, ...fields });
      dirtyForms.delete(form);
      autosave.settled(id, fields.label);
      render(next);
    }
    catch (error) { autosave.retry(id); await reload(); setError(error); }
    finally { busy = false; busyIds.delete(id); renderList(); setControls(); }
  }

  $('add-form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const labels = parseLabels($('add-labels').value);
    if (!labels.length) { setError('Add at least one account label.'); return; }
    busy = true; setControls(); setError(null);
    try {
      render(await readManager('manager_add_profiles', { labels }));
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
  // This form lives on Memory & pacing; its errors must be visible there, not on Accounts.
  const setMemoryError = (message) => {
    $('memory-error').textContent = message ? String(message) : '';
    $('memory-error').hidden = !message;
  };
  $('manager-settings').addEventListener('input', () => setMemoryError(null));
  $('manager-settings').addEventListener('submit', async (event) => {
    event.preventDefault();
    let payload;
    try { payload = settingsPayload({ memoryBudgetMb: $('memory-budget').value, reserveMb: $('memory-reserve').value, maxActive: $('memory-max').value, pauseMinSeconds: $('pause-min').value, pauseMaxSeconds: $('pause-max').value }); }
    catch (error) { setMemoryError(error); return; }
    busy = true; setControls(); setMemoryError(null);
    try {
      const next = await readManager('manager_save_settings', payload);
      dirtyForms.delete($('manager-settings'));
      render(next);
      $('settings-state').textContent = 'Saved. Values shown here came back from the native manager.';
    } catch (error) { setMemoryError(error); await reload(); }
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
      render(await readManager('manager_pair_gateway', { token }));
      $('manager-pairing-note').textContent = 'Token saved in this Mac’s Keychain. Gateway account enablement stays off until Gateway authorizes these workers. The token is never shown again.';
    } catch (error) { renderPairing(); setPairError(error, token); $('manager-pairing-note').textContent = 'Token was not saved. Nothing was stored in browser storage.'; }
    finally { input.value = ''; busy = false; $('manager-pair').disabled = false; setControls(); }
  });
  $('manager-open').addEventListener('click', () => invoke('open_gateway').catch((error) => setPairError(error)));
  $('manager-token').addEventListener('input', () => setPairError(null));
  $('delete-cancel').addEventListener('click', () => $('delete-dialog').close());
  $('delete-confirm').addEventListener('click', () => {
    const id = deletingId;
    $('delete-dialog').close();
    if (id) deleteAccount(id);
  });

  return {
    refresh,
    subscribe(notify) { listeners.add(notify); if (snapshot) notify(snapshot); return () => listeners.delete(notify); },
    current: () => snapshot,
  };
}
