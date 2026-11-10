import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import {
  activityTier, closableIds, connectionTier, creditsLine, creditsView, gatewaySpend, memorySummary, needsAttention, openableIds,
  pairingStatus, parseLabels, parseRange, processTier, profileUpdatePayload, rowAction, showAction,
  settingsPayload, sortProfiles, timeLabel, toMillis,
} from '../desktop/accounts.mjs';

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

test('desktop surface keeps native commands, password pairing and no browser token storage', async () => {
  const load = async (path) => readFile(new URL(path, import.meta.url), 'utf8');
  const html = await load('../desktop/index.html');
  for (const id of ['profiles', 'add-form', 'range-input', 'attention-only', 'open-selected', 'close-selected', 'memory-budget', 'memory-reserve', 'memory-max', 'pause-min', 'pause-max', 'manager-token', 'manager-pair', 'manager-pairing-state']) {
    assert.match(html, new RegExp(`id="${id}"`), `missing control ${id}`);
  }
  assert.match(html, /id="manager-token" type="password" autocomplete="off"/);
  const tokenTag = html.match(/<input id="manager-token"[^>]*>/)[0];
  assert.doesNotMatch(tokenTag, /PASSWORD_|\{\{|\}\}/);
  assert.ok(html.indexOf('id="memory-summary"') < html.indexOf('id="memory-disclosure"'), 'memory readout must sit outside the disclosure');
  assert.equal(html.includes('localStorage'), false);
  const accounts = await load('../desktop/accounts.mjs');
  const app = await load('../desktop/app.mjs');
  for (const name of ['manager_snapshot', 'manager_add_profiles', 'manager_update_profile', 'manager_open_profiles', 'manager_close_profiles', 'manager_save_settings', 'manager_pair_gateway']) {
    assert.ok(accounts.includes(`'${name}'`), `missing native command ${name}`);
  }
  assert.equal(/localStorage|sessionStorage/.test(accounts + app), false);
});
