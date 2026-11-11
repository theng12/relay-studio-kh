import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import {
  activityTier, closableIds, connectionTier, createLabelAutosave, creditsLine, creditsView, gatewaySpend, LABEL_AUTOSAVE_MS,
  labelAutosavePayload, memorySummary, needsAttention, openableIds,
  pairingStatus, parseLabels, parseRange, processTier, profileUpdatePayload, rowAction, showAction,
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

test('open and close actions follow desired intent and never replay a pending close', () => {
  assert.deepEqual(rowAction(profile(1)), { kind: 'open', label: 'Open', disabled: false });
  assert.deepEqual(rowAction(profile(1, { desired_open: true })), { kind: 'close', label: 'Close', disabled: false });
  assert.equal(rowAction(profile(1, { close_pending: true })).disabled, true);
  assert.equal(rowAction(profile(1, { process_state: 'closing' })).disabled, true);
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
    label: 'Studio', enabled: true, imageWorkerId: '', videoWorkerId: 'video-1',
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
  for (const page of ['accounts', 'browsers', 'memory', 'gateway', 'extensions']) {
    assert.match(html, new RegExp(`data-page="${page}"`), `missing sidebar row ${page}`);
    assert.match(html, new RegExp(`id="page-${page}"`), `missing pane ${page}`);
  }
  assert.match(html, /id="page-whats-new"/, 'missing What’s New pane');
  assert.match(html, /id="version" class="version-pill"/, 'version and What’s New must share one pill');
  assert.doesNotMatch(html, /nav-whats-new/, 'What’s New must not be a separate sidebar row');
  for (const id of ['browsers-summary', 'browser-attention-only', 'browser-close', 'browser-force', 'browser-cards', 'browser-empty', 'notes', 'notes-title', 'force-blocked', 'force-cancel', 'force-confirm']) {
    assert.match(html, new RegExp(`id="${id}"`), `missing browser control ${id}`);
  }
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
