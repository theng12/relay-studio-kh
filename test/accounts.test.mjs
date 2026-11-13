import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import {
  activityTier, closableIds, connectionTier, createLabelAutosave, creditsLine, creditsView, gatewaySpend, LABEL_AUTOSAVE_MS,
  labelAutosavePayload, memorySummary, needsAttention, openableIds,
  closeAction, connectionStateLabel, pairingStatus, parseLabels, parseRange, processTier, profileUpdatePayload, rowAction, showAction,
  settingsPayload, sortProfiles, timeLabel, toMillis,
} from '../desktop/accounts.mjs';
import { browserProfiles, browserSummary, closeElapsedSeconds, elapsedLabel, forceBlockReason, residentIds } from '../desktop/browsers.mjs';

const profile = (number, overrides = {}) => ({ id: `id-${number}`, number, label: `Account ${number}`, enabled: true, desired_open: false, process_state: 'closed', ...overrides });
const profiles = [profile(3), profile(1), profile(5), profile(2)];

test('range input resolves immutable ordinals to stable ids in number order', () => {
  assert.deepEqual(parseRange('2-4', profiles), ['id-2', 'id-3']);
  assert.deepEqual(parseRange('1, 3, 5', profiles), ['id-1', 'id-3', 'id-5']);
  assert.deepEqual(parseRange(' 2 ', profiles), ['id-2']);
  assert.deepEqual(profiles.map((entry) => entry.number), [3, 1, 5, 2]);
});

test('range input rejects reversed, malformed, absent and unsafe numbers without guessing', () => {
  assert.throws(() => parseRange('5-2', profiles), /lower number first/);
  assert.throws(() => parseRange('2 x', profiles), /whole account numbers/);
  assert.throws(() => parseRange('', profiles), /Enter an account number/);
  assert.throws(() => parseRange('9-12', profiles), /No saved account matches/);
  assert.throws(() => parseRange('1-9007199254740993', profiles), /safe positive/);
  assert.throws(() => parseRange('9007199254740992', profiles), /safe positive/);
  assert.throws(() => parseRange('0', profiles), /safe positive/);
});

test('range input resolves against existing profiles without enumerating the interval', () => {
  const sparse = [profile(1), profile(5), profile(9007199254740000, { id: 'far' })];
  assert.deepEqual(parseRange('1-9007199254740000', sparse), ['id-1', 'id-5', 'far']);
});

test('profile order is by immutable number', () => {
  assert.deepEqual(sortProfiles(profiles).map((entry) => entry.number), [1, 2, 3, 5]);
  assert.deepEqual(profiles.map((entry) => entry.number), [3, 1, 5, 2]);
});

test('attention filtering uses native attention only', () => {
  const entries = [profile(1, { attention: 'worker offline' }), profile(2), profile(3, { attention: null })];
  assert.equal(needsAttention(entries[0]), true);
  assert.equal(needsAttention(entries[1]), false);
  assert.equal(needsAttention(entries[2]), false);
});

test('bulk open and close exclude uncertain or already settled accounts', () => {
  const entries = [
    profile(1),
    profile(2, { desired_open: true }),
    profile(3, { process_state: 'queued' }),
    profile(4, { process_state: 'open' }),
    profile(5, { process_state: 'closing' }),
    profile(6, { close_pending: true, desired_open: true }),
    profile(7, { process_state: 'opening' }),
  ];
  const ids = entries.map((entry) => entry.id);
  assert.deepEqual(openableIds(entries, ids), ['id-1']);
  assert.deepEqual(closableIds(entries, ids), ['id-2', 'id-3', 'id-4', 'id-7']);
});

test('an open account focuses, a closed one opens, and a pending close never replays', () => {
  assert.deepEqual(rowAction(profile(1)), { kind: 'open', label: 'Open', disabled: false });
  // Open browsers focus instead of offering a dead Open or a Close-first label.
  assert.deepEqual(rowAction(profile(1, { process_state: 'open' })), { kind: 'focus', label: 'Focus', disabled: false });
  assert.deepEqual(rowAction(profile(1, { process_state: 'opening' })), { kind: 'focus', label: 'Focus', disabled: false });
  assert.deepEqual(rowAction(profile(1, { desired_open: true, process_state: 'queued' })), { kind: 'open', label: 'Open', disabled: false });
  assert.equal(rowAction(profile(1, { close_pending: true })).disabled, true);
  assert.equal(rowAction(profile(1, { process_state: 'closing' })).disabled, true);
});

test('Close is its own visible control: open or queued accounts, never a closed one', () => {
  assert.deepEqual(closeAction(profile(1, { process_state: 'open' })), { kind: 'close', label: 'Close', visible: true, disabled: false });
  assert.deepEqual(closeAction(profile(1, { process_state: 'opening' })), { kind: 'close', label: 'Close', visible: true, disabled: false });
  assert.deepEqual(closeAction(profile(1, { desired_open: true, process_state: 'queued' })), { kind: 'close', label: 'Close', visible: true, disabled: false });
  assert.equal(closeAction(profile(1)).visible, false, 'a closed account has nothing to close');
  assert.equal(closeAction(profile(1, { close_pending: true })).visible, false, 'the primary already says Waiting to close');
  assert.equal(closeAction(profile(1, { process_state: 'closing' })).visible, false);
});

test('attention keeps a direct show action even while the profile is already open', () => {
  assert.equal(showAction(profile(1, { attention: true, process_state: 'open' })).visible, true);
  assert.equal(showAction(profile(1, { attention: true, desired_open: true })).visible, false);
  assert.equal(showAction(profile(1, { attention: true, desired_open: true, process_state: 'queued' })).visible, false);
  assert.equal(showAction(profile(1, { attention: true })).visible, false);
  assert.equal(showAction(profile(1, { process_state: 'open' })).visible, false);
  assert.equal(showAction(profile(1, { attention: true, process_state: 'open', close_pending: true })).disabled, true);
});

test('worker bindings clear with an empty string while absent arguments stay unchanged', () => {
  assert.deepEqual(profileUpdatePayload({ label: '  Studio ', enabled: true, imageWorkerId: '', videoWorkerId: ' video-1 ' }), {
    label: 'Studio', enabled: true, imageWorkerId: '', videoWorkerId: 'video-1', audioWorkerId: '',
  });
});

test('connection, sign-in, readiness and activity stay independent tiers', () => {
  assert.equal(connectionTier({}).label, 'Connection unknown');
  assert.equal(connectionTier({ connection: 'connected' }).tone, 'ok');
  assert.match(connectionTier({ connection: 'connected', sign_in: 'unknown', readiness: 'unknown' }).detail, /Sign-in unknown · Readiness unknown/);
  assert.equal(connectionTier({ connection: 'offline' }).tone, 'warn');
  assert.equal(connectionTier({ connection: 'paused' }).label, 'Paused');
  assert.equal(connectionTier({ connection: 'error' }).tone, 'bad');
  assert.equal(connectionTier({ connection: 'connected', error: 'transport failed' }).tone, 'bad');
  assert.equal(activityTier({ activity: 'generating' }).label, 'Generating');
  assert.equal(activityTier({ activity: 'recovering' }).label, 'Recovering');
  assert.equal(activityTier({}).label, 'Activity unknown');
  assert.equal(processTier({ close_pending: true }).label, 'Waiting to close');
  assert.equal(processTier({ process_state: 'queued' }).label, 'Waiting for memory');
});

test('Gateway epoch seconds and ISO times convert without a timezone assumption', () => {
  assert.equal(toMillis(1794350000), 1794350000000);
  assert.equal(toMillis('1794350000.0'), 1794350000000);
  assert.equal(toMillis(1794350000000), 1794350000000);
  assert.equal(toMillis('2026-11-11T12:00:00Z'), Date.parse('2026-11-11T12:00:00Z'));
  assert.equal(toMillis('not-a-time'), null);
  assert.equal(toMillis(null), null);
  assert.ok(timeLabel(1794350000));
});

test('credits follow the Gateway balance/credit_buckets shape without summing workers', () => {
  const now = Date.parse('2026-11-11T12:00:00Z');
  assert.equal(creditsView({ credits: null }, now).known, false);
  const fresh = creditsView({ credits: {
    balance: { value: 0, unit: 'credits', source: 'flow.balance', observed_at: '2026-11-11T11:59:00Z' },
    fresh: true,
    credit_buckets: [{ id: 'image', value: 4, unit: 'credits', reset_at: '2026-11-11T13:00:00Z' }, { id: 'video', value: 4, unit: 'credits' }],
  } }, now);
  assert.equal(fresh.known, true);
  assert.equal(fresh.stale, false);
  assert.equal(fresh.balance.value, '0');
  assert.equal(fresh.balance.unit, 'credits');
  assert.equal(fresh.source, 'flow.balance');
  assert.deepEqual(fresh.buckets.map((bucket) => bucket.label), ['Image', 'Video']);
  assert.match(fresh.buckets[0].value, /reset /);
  assert.equal('total' in fresh, false);
  assert.equal(creditsLine({ credits: null }, now), 'Credits not reported');
  assert.equal(creditsLine({ credits: { balance: null } }, now), 'Balance not reported');
  assert.equal(creditsLine({ credits: { balance: { value: 2, unit: 'credits', observed_at: '2026-11-11T11:59:00Z' }, fresh: true } }, now), '2 credits · fresh');
  assert.equal(creditsLine({ credits: { balance: { value: 2, unit: 'credits', observed_at: '2026-11-11T11:59:00Z' }, fresh: false } }, now), '2 credits · stale');
  assert.equal(creditsLine({ credits: { balance: { value: 2, unit: 'credits', observed_at: '2026-11-11T11:59:00Z' } } }, now), '2 credits · freshness unknown');
  const stale = creditsView({ credits: { balance: null, fresh: false, observed_at: '2026-11-11T11:00:00Z', reset_at: null } }, now);
  assert.equal(stale.stale, true);
  assert.equal(stale.balance, null);
  assert.equal(creditsView({ credits: { observed_at: 'not-a-time' } }, now).stale, null);
  const epoch = creditsView({ credits: { balance: { value: 5, unit: 'credits', observedAt: 1794350000 }, fresh: true } }, 1794350060000);
  assert.equal(epoch.stale, false);
  assert.equal(creditsView({ credits: null }, now).text, 'Not reported');
  assert.match(gatewaySpend({ gateway_enabled: false }), /disabled by Gateway/);
  assert.match(gatewaySpend({}), /Not authorized/);
});

test('freshness never falls back to a local age guess or accepts a future observation', () => {
  const now = Date.parse('2026-11-11T12:00:00Z');
  assert.equal(creditsView({ credits: { balance: { value: 1, unit: 'credits', observedAt: '2026-11-11T11:59:00Z' } } }, now).stale, null);
  assert.equal(creditsView({ credits: { balance: { value: 1, unit: 'credits', observedAt: '2026-11-11T13:00:00Z' }, fresh: true } }, now).stale, null);
  assert.equal(creditsView({ credits: { balance: { value: 1, unit: 'credits', observedAt: 'bad-time' }, fresh: true } }, now).stale, null);
  assert.equal(creditsView({ credits: { balance: { value: 1, unit: 'credits', observedAt: '2026-11-11T13:00:00Z' }, fresh: false } }, now).stale, true);
});

test('Gateway pairing is not claimed until an authoritative snapshot says so', () => {
  assert.deepEqual(pairingStatus({}), { paired: false, label: 'Not paired' });
  assert.deepEqual(pairingStatus({ manager_paired: false }), { paired: false, label: 'Not paired' });
  assert.deepEqual(pairingStatus({ manager_paired: true }), { paired: true, label: 'Paired' });
  assert.deepEqual(pairingStatus({ pairing: { paired: true } }), { paired: true, label: 'Paired' });
});

test('settings payload keeps zero pacing, maps blanks to null and rejects invalid values', () => {
  assert.deepEqual(settingsPayload({ memoryBudgetMb: '', reserveMb: '512', maxActive: '', pauseMinSeconds: '0', pauseMaxSeconds: '0' }), {
    memoryBudgetMb: null, reserveMb: 512, maxActive: null, pauseMinSeconds: 0, pauseMaxSeconds: 0,
  });
  assert.deepEqual(settingsPayload({ memoryBudgetMb: '8192', reserveMb: '0', maxActive: '3', pauseMinSeconds: '5', pauseMaxSeconds: '15' }), {
    memoryBudgetMb: 8192, reserveMb: 0, maxActive: 3, pauseMinSeconds: 5, pauseMaxSeconds: 15,
  });
  assert.throws(() => settingsPayload({ reserveMb: '0', pauseMinSeconds: '15', pauseMaxSeconds: '5' }), /same as or greater than/);
  assert.throws(() => settingsPayload({ reserveMb: '-1', pauseMinSeconds: '0', pauseMaxSeconds: '5' }), /0 or more/);
  assert.throws(() => settingsPayload({ reserveMb: '0', pauseMinSeconds: '-0.5', pauseMaxSeconds: '5' }), /zero or more/);
  assert.throws(() => settingsPayload({ reserveMb: '0', pauseMinSeconds: '', pauseMaxSeconds: '5' }), /required/);
});

test('saving memory preferences preserves finite fractional pacing overrides', () => {
  const fields = { memoryBudgetMb: '4096', reserveMb: '512', maxActive: '', pauseMinSeconds: '1.5', pauseMaxSeconds: '2.75' };
  const payload = settingsPayload(fields);
  assert.equal(payload.memoryBudgetMb, 4096);
  assert.equal(payload.pauseMinSeconds, 1.5);
  assert.equal(payload.pauseMaxSeconds, 2.75);
  for (const invalid of ['Infinity', 'NaN', '-0.1']) {
    assert.throws(() => settingsPayload({ ...fields, pauseMinSeconds: invalid }), /zero or more/);
    assert.throws(() => settingsPayload({ ...fields, pauseMaxSeconds: invalid }), /zero or more/);
  }
});

test('memory readout keeps deficits negative and never turns missing measurements into zero', () => {
  const summary = memorySummary({ used_mb: 5000, budget_mb: 4096, available_mb: -100, physical_mb: 8192, per_profile_mb: 512, queued: 1, reason: 'Below reserve' });
  assert.match(summary, /-904 MB headroom/);
  assert.match(summary, /-100 MB available of/);
  assert.match(summary, /1 waiting to open: Below reserve/);
  const zero = memorySummary({ used_mb: 0, budget_mb: 4096, available_mb: 8192, reserve_mb: 2048, per_profile_mb: 1024 });
  assert.match(zero, /Nothing opened yet/);
  assert.match(zero, /about 4 more fit/, 'the fit estimate respects the budget');
  assert.doesNotMatch(zero, /account\(s\)/);
  const unknown = memorySummary({});
  assert.doesNotMatch(unknown, /headroom/);
  assert.match(unknown, /Measured use not reported/);
  assert.match(unknown, /budget not set/);
});

test('label input is one account per non-empty line', () => {
  assert.deepEqual(parseLabels(' Studio\n\n  Draft \r\n'), ['Studio', 'Draft']);
  assert.deepEqual(parseLabels('   '), []);
});

// A controllable clock and timer so the debounce is exercised without real waiting.
const fakeClock = () => {
  let time = 0;
  let handle = 0;
  const timers = new Map();
  const settle = () => new Promise((resolve) => setImmediate(resolve));
  return {
    now: () => time,
    setTimer: (fn, ms) => { const id = ++handle; timers.set(id, { at: time + ms, fn }); return id; },
    clearTimer: (id) => timers.delete(id),
    async advance(ms) { time += ms; for (const [id, timer] of [...timers]) if (timer.at <= time) { timers.delete(id); timer.fn(); } await settle(); },
    settle,
  };
};

test('the label autosave writes only the profile id and label', async () => {
  const clock = fakeClock();
  const calls = [];
  const autosave = createLabelAutosave({
    invoke: async (name, args) => { calls.push({ name, args }); },
    setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  autosave.edit('id-1', '  Studio  ');
  await clock.advance(LABEL_AUTOSAVE_MS);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].name, 'manager_update_profile');
  assert.deepEqual(Object.keys(calls[0].args).sort(), ['label', 'profileId']);
  assert.deepEqual(calls[0].args, { profileId: 'id-1', label: 'Studio' });
  assert.deepEqual(labelAutosavePayload(' Studio '), { label: 'Studio' });
});

test('rapid label edits debounce into one write of the newest label', async () => {
  const clock = fakeClock();
  const calls = [];
  const autosave = createLabelAutosave({
    invoke: async (name, args) => { calls.push(args); },
    setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  autosave.edit('id-1', 'Stu');
  await clock.advance(300);
  autosave.edit('id-1', 'Studio');
  await clock.advance(300);
  assert.equal(calls.length, 0, 'the debounce is still open');
  await clock.advance(300);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].label, 'Studio');
});

test('a slow reply never overwrites a newer label edit', async () => {
  const clock = fakeClock();
  const calls = [];
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const autosave = createLabelAutosave({
    invoke: async (name, args) => { calls.push(args); await gate; },
    setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  autosave.edit('id-1', 'First');
  await clock.advance(LABEL_AUTOSAVE_MS);
  assert.equal(calls.length, 1);
  autosave.edit('id-1', 'Second');
  await clock.advance(LABEL_AUTOSAVE_MS);
  assert.equal(calls.length, 1, 'no second write while the first is in flight');
  release();
  await clock.settle();
  await clock.advance(0);
  await clock.settle();
  assert.equal(calls.length, 2);
  assert.equal(calls[1].label, 'Second');
  assert.equal(autosave.state('id-1').draft, null);
  assert.equal(autosave.state('id-1').status, 'saved');
  assert.equal(autosave.state('id-1').saved, 'Second');
});

test('a failed label save keeps the draft, reports the failure and does not retry on its own', async () => {
  const clock = fakeClock();
  const calls = [];
  const autosave = createLabelAutosave({
    invoke: async (name, args) => { calls.push(args); throw new Error('Gateway refused the label'); },
    setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  autosave.edit('id-1', 'Studio');
  await clock.advance(LABEL_AUTOSAVE_MS);
  await clock.settle();
  const state = autosave.state('id-1');
  assert.equal(state.status, 'failed');
  assert.equal(state.draft, 'Studio');
  assert.match(state.error, /Gateway refused the label/);
  await clock.advance(LABEL_AUTOSAVE_MS * 4);
  assert.equal(calls.length, 1, 'a failed write is not retried behind the owner’s back');
});

test('an empty label is never written and an explicit save replaces the queued write', async () => {
  const clock = fakeClock();
  const calls = [];
  const autosave = createLabelAutosave({
    invoke: async (name, args) => { calls.push(args); },
    setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  autosave.edit('id-1', '   ');
  await clock.advance(LABEL_AUTOSAVE_MS);
  assert.equal(calls.length, 0);
  assert.equal(autosave.state('id-1').status, 'invalid');
  autosave.edit('id-1', 'Studio');
  autosave.cancel('id-1');
  autosave.settled('id-1', 'Studio');
  await clock.advance(LABEL_AUTOSAVE_MS * 2);
  assert.equal(calls.length, 0, 'the explicit save carries the label itself');
  assert.equal(autosave.state('id-1').status, 'saved');
});

test('a queued label edit finishes after the command that owns the manager', async () => {
  const clock = fakeClock();
  const calls = [];
  let busy = true;
  const autosave = createLabelAutosave({
    invoke: async (name, args) => { calls.push(args); },
    ready: () => !busy,
    setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  autosave.edit('id-1', 'Studio');
  await clock.advance(LABEL_AUTOSAVE_MS);
  assert.equal(calls.length, 0);
  assert.equal(autosave.state('id-1').status, 'waiting');
  busy = false;
  await clock.advance(1000);
  await clock.settle();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].label, 'Studio');
});

test('composition suspends both a queued save and continuation after an in-flight save', async () => {
  for (const inFlight of [false, true]) {
    const clock = fakeClock();
    const calls = [];
    let release;
    const gate = new Promise((resolve) => { release = resolve; });
    const autosave = createLabelAutosave({
      invoke: async (name, args) => { calls.push(args); if (calls.length === 1 && inFlight) await gate; },
      setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
    });
    autosave.edit('id-1', 'Prefix');
    if (inFlight) await clock.advance(LABEL_AUTOSAVE_MS);
    autosave.hold('id-1', 'ខ្ម');
    await clock.advance(LABEL_AUTOSAVE_MS * 2);
    release();
    await clock.settle();
    await clock.advance(LABEL_AUTOSAVE_MS);
    assert.equal(calls.length, inFlight ? 1 : 0, 'no composition text is written');
    autosave.edit('id-1', 'ខ្មែរ');
    await clock.advance(LABEL_AUTOSAVE_MS);
    assert.equal(calls.at(-1).label, 'ខ្មែរ');
  }
});

test('a newer edit is still saved after the older in-flight request fails', async () => {
  const clock = fakeClock();
  const calls = [];
  let reject;
  const gate = new Promise((resolve, fail) => { reject = fail; });
  const autosave = createLabelAutosave({
    invoke: async (name, args) => { calls.push(args); if (calls.length === 1) await gate; },
    setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  autosave.edit('id-1', 'Older');
  await clock.advance(LABEL_AUTOSAVE_MS);
  autosave.edit('id-1', 'Newer');
  await clock.advance(LABEL_AUTOSAVE_MS);
  reject(new Error('Older write failed'));
  await clock.settle();
  await clock.advance(0);
  assert.deepEqual(calls.map((args) => args.label), ['Older', 'Newer']);
  assert.equal(autosave.state('id-1').status, 'saved');
});

test('pre-save snapshots cannot replace acknowledged labels but a newer read can', async () => {
  const clock = fakeClock();
  const autosave = createLabelAutosave({
    invoke: async () => {}, setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  const oldRead = autosave.revision();
  autosave.edit('id-1', 'New label');
  await clock.advance(LABEL_AUTOSAVE_MS);
  autosave.observe([{ id: 'id-1', label: 'Old label' }], oldRead);
  assert.equal(autosave.label('id-1', 'Old label'), 'New label');
  autosave.observe([{ id: 'id-1', label: 'Fresh readback' }], autosave.revision());
  assert.equal(autosave.label('id-1', 'Fresh readback'), 'Fresh readback');
});

test('explicit save completion does not strand an edit made while it was waiting', async () => {
  const clock = fakeClock();
  const calls = [];
  const autosave = createLabelAutosave({
    invoke: async (name, args) => { calls.push(args); },
    setTimer: clock.setTimer, clearTimer: clock.clearTimer, now: clock.now,
  });
  autosave.edit('id-1', 'Submitted');
  autosave.edit('id-1', 'Typed while waiting');
  autosave.cancel('id-1');
  autosave.settled('id-1', 'Submitted');
  await clock.advance(LABEL_AUTOSAVE_MS);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].label, 'Typed while waiting');
});

test('desktop surface keeps native commands, password pairing and no browser token storage', async () => {
  const load = async (path) => readFile(new URL(path, import.meta.url), 'utf8');
  const html = await load('../desktop/index.html');
  for (const id of ['profiles', 'add-form', 'range-input', 'attention-only', 'open-selected', 'close-selected', 'memory-budget', 'memory-reserve', 'memory-max', 'pause-min', 'pause-max', 'manager-token', 'manager-pair', 'manager-pairing-state']) {
    assert.match(html, new RegExp(`id="${id}"`), `missing control ${id}`);
  }
  assert.match(html, /id="manager-token" type="password" autocomplete="off"/);
  const tokenTag = html.match(/<input id="manager-token"[^>]*>/)[0];
  assert.doesNotMatch(tokenTag, /PASSWORD_|\{\{|\}\}/);
  assert.equal(html.includes('localStorage'), false);
  for (const page of ['overview', 'accounts', 'browsers', 'memory', 'gateway', 'extensions', 'settings']) {
    assert.match(html, new RegExp(`data-page="${page}"`), `missing sidebar row ${page}`);
    assert.match(html, new RegExp(`id="page-${page}"`), `missing pane ${page}`);
  }
  assert.match(html, /id="page-whats-new"/, 'missing What’s New pane');
  assert.match(html, /id="whats-new-link"/, 'footer must open What’s New');
  assert.match(html, /<span id="version" class="pill-version">/, 'version is static text in the footer pill');
  assert.doesNotMatch(html, /nav-whats-new|data-page="whats-new"/, 'What’s New must not be a navigation row');
  assert.equal((html.match(/class="nav-row"/g) || []).length, 7, 'one row per destination');
  assert.equal((html.match(/class="nav-row"[\s\S]{0,160}?<svg class="icon"/g) || []).length, 7, 'every nav row carries an icon');
  assert.equal((html.match(/id="overview-(accounts|open|memory|gateway|attention)"/g) || []).length, 5, 'overview dashboard must be complete');
  for (const value of ['system', 'light', 'dark']) {
    assert.match(html, new RegExp(`name="appearance" value="${value}"`), `missing appearance ${value}`);
  }
  // Three options only; a stored `default` from an older build resolves to System instead.
  assert.doesNotMatch(html, /name="appearance" value="default"/, 'the fourth appearance option is gone');
  assert.equal((html.match(/name="appearance" value=/g) || []).length, 3, 'exactly three appearance choices');
  assert.doesNotMatch(html, /Follow System/, 'the choice is labelled System');
  for (const id of ['browsers-summary', 'browser-attention-only', 'browser-close', 'browser-force', 'browser-cards', 'browser-empty', 'notes', 'notes-title', 'force-blocked', 'force-cancel', 'force-confirm', 'start-at-login', 'data-folder', 'show-data-folder']) {
    assert.match(html, new RegExp(`id="${id}"`), `missing control ${id}`);
  }
  const conf = await load('../src-tauri/tauri.conf.json');
  assert.match(conf, /"mainBinaryName": "Relay Studio KH"/, 'the executable must not expose the repo slug');
  assert.match(conf, /"productName": "Relay Studio KH"/, 'the bundle keeps its display name');
  const native = await load('../src-tauri/src/lib.rs');
  assert.match(native, /inner_size\(frame\.as_ref\(\)\.map_or\(1100\.0/, 'default window is 1100 wide');
  assert.match(native, /min_inner_size\(860\.0, 560\.0\)/, 'minimum window size');
  assert.doesNotMatch(native, /AppleInterfaceStyle|System Events|osascript/, 'appearance never changes macOS');
  assert.match(html, /class="window relay-connector"/, 'shared tokens must scope to the window shell');
  const accounts = await load('../desktop/accounts.mjs');
  const browsers = await load('../desktop/browsers.mjs');
  const app = await load('../desktop/app.mjs');
  for (const name of ['manager_snapshot', 'manager_add_profiles', 'manager_update_profile', 'manager_open_profiles', 'manager_close_profiles', 'manager_save_settings', 'manager_pair_gateway']) {
    assert.ok(accounts.includes(`'${name}'`), `missing native command ${name}`);
  }
  for (const name of ['manager_show_profiles', 'manager_open_profiles', 'manager_close_profiles', 'manager_force_close_profiles']) {
    assert.ok(browsers.includes(`'${name}'`), `missing browser command ${name}`);
  }
  assert.equal(/localStorage|sessionStorage/.test(accounts + browsers + app), false);
  assert.doesNotMatch(app, /AppleInterfaceStyle|System Events|osascript/, 'appearance never changes macOS');
});

test('browsers pane lists only resident or queued profiles and counts them honestly', () => {
  const rows = [
    profile(1, { process_state: 'open', process_id: 41, gateway: { activity: 'idle' } }),
    profile(2, { process_state: 'closed' }),
    profile(3, { process_state: 'queued', desired_open: true, gateway: { activity: 'waiting' } }),
    profile(4, { process_state: 'closed', close_pending: true }),
  ];
  assert.deepEqual(browserProfiles(rows).map((entry) => entry.number), [1, 3, 4]);
  assert.equal(browserSummary(rows), '1 browser open · 1 waiting for memory · 1 waiting to close');
  assert.equal(browserSummary([profile(2)]), 'No browsers open.');
});

test('close age counts from the recorded request and never invents a start', () => {
  assert.equal(closeElapsedSeconds({ close_pending: true }, 5000), null);
  assert.equal(closeElapsedSeconds({ close_pending: false, close_requested_at: 100 }, 5000), null);
  assert.equal(closeElapsedSeconds({ close_pending: true, close_requested_at: 100 }, 105000), 5);
  assert.equal(elapsedLabel(5), '5 s');
  assert.equal(elapsedLabel(150), '2 min');
  assert.equal(elapsedLabel(7500), '2 h 5 min');
});

test('force close blocks active or uncertain Gateway work and non-resident rows', () => {
  assert.match(forceBlockReason({ process_id: 1, gateway: { activity: 'generating' } }), /active or uncertain/);
  assert.match(forceBlockReason({ process_id: 1, gateway: { activity: 'recovering' } }), /active or uncertain/);
  assert.match(forceBlockReason({ process_state: 'closed' }), /nothing to force close/);
  assert.equal(forceBlockReason({ process_id: 1, gateway: { activity: 'idle' } }), null);
  const rows = [
    profile(1, { process_state: 'open', process_id: 41, gateway: { activity: 'generating' } }),
    profile(2, { process_state: 'open', process_id: 42, gateway: { activity: 'idle' } }),
    profile(3, { process_state: 'queued', gateway: { activity: 'idle' } }),
  ];
  assert.deepEqual(residentIds(rows, ['id-1', 'id-2', 'id-3']), ['id-1', 'id-2']);
});

test('accounts page offers rename, recoverable delete and a persisted compact/card view', async () => {
  const load = async (path) => readFile(new URL(path, import.meta.url), 'utf8');
  const html = await load('../desktop/index.html');
  const accounts = await load('../desktop/accounts.mjs');
  const app = await load('../desktop/app.mjs');
  const native = await load('../src-tauri/src/lib.rs');
  // Compact is the default and the choice is a real control, not a fixed layout.
  assert.match(html, /name="accounts-view" value="card" checked/, 'Card is the default view');
  assert.match(html, /name="accounts-view" value="card"/, 'Card is available');
  assert.match(html, /<span>Card<\/span>/, 'the second view is named Card');
  assert.match(app, /accountsView/, 'the chosen view is saved with settings');
  assert.match(native, /accounts_view/, 'the native settings keep the chosen view');
  assert.match(native, /matches!\(value, "compact" \| "card" \| "table"\)/, 'Compact, Card and Table are accepted');
  assert.match(app, /value === 'compact' \? 'compact' : value === 'table' \? 'table' : 'card'/, 'the saved view is applied; every unknown value, including grid, opens Card');
  assert.match(html, /name="accounts-view" value="table"/, 'Table is available and remembered');
  assert.match(accounts, /row-menu/, 'Details and Delete live in the row overflow');
  assert.match(accounts, /warning-group/, 'warnings are grouped with counts');
  // Rename and delete exist and go through the native commands rather than local state.
  assert.match(accounts, /manager_update_profile/, 'rename saves through the native update command');
  assert.match(accounts, /manager_delete_profile/, 'delete goes through the native delete command');
  assert.match(accounts, /remove.textContent = 'Delete'/, 'a delete button is always drawn on the row');
  assert.match(accounts, /startRename/, 'the label can be edited in place');
  // The macOS webview never shows window.confirm; relying on it silently cancelled every delete.
  assert.doesNotMatch(accounts, /window\.confirm/, 'delete never waits on a web dialog the macOS webview drops');
  assert.match(html, /id="delete-dialog"/, 'delete asks in Relay\'s own dialog');
  assert.match(accounts, /delete-dialog/, 'the dialog is wired to the row button');
  assert.match(accounts, /deleted\.`/, 'a successful delete says the account is gone');
  assert.doesNotMatch(accounts, /Moved to/, 'the page never talks about a trash folder');
  assert.match(accounts, /rowErrors/, 'a refused delete stays visible on its own account row');
  // The owner rejected the native disclosure triangle outright (2026-11-12).
  assert.doesNotMatch(accounts, /createElement\('details'\)/, 'no native disclosure triangle on accounts');
  assert.match(accounts, /Show details/, 'details open from a visible text button');
  const pane = await load('../web/app.mjs');
  assert.doesNotMatch(pane, /element\('details'\)|element\('summary'\)/, 'no native disclosure triangle in the Gateway pane');
  assert.match(pane, /Show details/, 'the Gateway pane opens details from a visible text button');
  const design = await load('../DESIGN.md');
  assert.match(design, /disclosure triangle/, 'the ban is written into the design rules');
  const manager = await load('../src-tauri/src/browser_manager.rs');
  assert.match(manager, /fn manager_delete_profile/, 'delete exists natively');
  assert.match(manager, /remove_dir_all/, 'delete removes the Chrome profile, as the person asked');
  assert.doesNotMatch(manager, /Moved to/, 'the page never talks about a trash folder');
});

test('row actions are never hidden behind hover', async () => {
  const load = async (path) => readFile(new URL(path, import.meta.url), 'utf8');
  const css = await load('../desktop/desktop.css');
  const html = await load('../desktop/index.html');
  const app = await load('../desktop/app.mjs');
  // Hover may add emphasis; it must never be the only way to reach Rename or Delete.
  assert.doesNotMatch(css, /account:hover \.account-actions \{[^}]*opacity: 1/, 'hover is not the reveal');
  assert.match(css, /\.account-actions \{ opacity: 1/, 'actions are drawn by default');
  // Footer: one tidy line, equal insets, the version truncates instead of wrapping.
  assert.match(css, /\.sidebar-footer \{[^}]*padding: 12px/, 'equal footer insets');
  assert.match(css, /\.footer-pill \{[^}]*white-space: nowrap/, 'the pill stays on one line');
  assert.match(css, /\.pill-version \{[^}]*text-overflow: ellipsis/, 'the version truncates rather than wraps');
  assert.match(css, /\.sidebar-footer \{[^}]*flex-direction: column/, 'the footer is one tidy column');
  assert.match(css, /\.footer-line \{[^}]*justify-content: center/, 'pill and update control share one centred line');
  assert.match(css, /\.footer-status \{[^}]*text-overflow: ellipsis/, 'the status line stays on one line');
  assert.match(css, /@media \(max-width: 1080px\)/, 'narrow windows get their own row layout');
  assert.match(css, /\.account-label \{[^}]*text-overflow: ellipsis/, 'the account label truncates instead of overlapping');
  assert.match(html, /id="support-link"/, 'the Support this app row exists');
  assert.match(html, /id="check-updates"/, 'the Check for Updates control exists');
  assert.match(app, /open_support_link/, 'support opens through the app opener');
  assert.match(app, /open_launcher/, 'updates open Studio Launcher KH, never a self-install');
  const native = await load('../src-tauri/src/lib.rs');
  assert.match(native, /nowpayments\.io\/donation\/studiokh/, 'the support URL is allow-listed in Rust');
});

test('every account row template exposes nextAction and a render failure is reported', async () => {
  const accounts = await (await import('node:fs/promises')).readFile(new URL('../desktop/accounts.mjs', import.meta.url), 'utf8');
  // The crash was a span created but never put into refs; this fails if it comes back.
  assert.match(accounts, /return \{ li, checkbox[^}]*nextAction/, 'nextAction is part of the row refs');
  assert.match(accounts, /if \(refs\.nextAction\)/, 'the write is guarded for templates without it');
  assert.match(accounts, /account list could not be drawn/, 'a render failure is surfaced, not left loading');
});

test('the app surface keeps one search, no duplicate nav card, and diagnostics behind a button', async () => {
  const load = async (path) => (await import('node:fs/promises')).readFile(new URL(path, import.meta.url), 'utf8');
  const html = await load('../desktop/index.html');
  const accounts = await load('../desktop/accounts.mjs');
  const browsers = await load('../desktop/browsers.mjs');
  const overview = await load('../desktop/overview.mjs');
  const css = await load('../desktop/desktop.css');
  assert.match(html, /id="account-search"/, 'one search in the accounts toolbar');
  assert.match(accounts, /String\(profile\.number\)\.includes\(needle\)/, 'search matches a number as well as a label');
  assert.doesNotMatch(html, /overview-browsers-link|overview-gateway-link/, 'no duplicate navigation card on Overview');
  assert.doesNotMatch(overview, /overview-browsers-link|overview-gateway-link/, 'no dead handlers for removed links');
  assert.match(browsers, /Show details/, 'Open windows diagnostics sit behind a visible button');
  assert.match(browsers, /meta\.hidden = true/, 'the card face does not show the diagnostic line by default');
});

test('G1 connection state is the row label, with no invented values', () => {
  assert.equal(connectionStateLabel('connected'), 'Connected');
  assert.equal(connectionStateLabel('no_project'), 'No Flow project');
  assert.equal(connectionStateLabel('needs_review'), 'Needs review');
  assert.equal(connectionStateLabel('something-new'), null, 'an unknown state falls back to the tier label');
});

test('rows are keyboard reachable and run their primary action', async () => {
  const accounts = await readFile(new URL('../desktop/accounts.mjs', import.meta.url), 'utf8');
  assert.match(accounts, /row\.tabIndex = 0/, 'each row joins the tab order');
  assert.match(accounts, /event\.key === 'Enter' \|\| event\.key === ' '/, 'Enter and Space run the row primary');
  assert.match(accounts, /event\.target !== row/, 'keys inside the row controls are never stolen');
});
