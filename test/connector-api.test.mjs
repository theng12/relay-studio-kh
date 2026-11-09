import test from 'node:test';
import assert from 'node:assert/strict';
import { readWorkers, mintWorker, revokeWorker, reportedCredit, workerState } from '../web/api.mjs';

function response(t, data, status = 200) {
  const calls = [];
  t.mock.method(globalThis, 'fetch', async (url, init) => {
    calls.push({ url, ...init });
    return new Response(JSON.stringify(data), { status });
  });
  return calls;
}

test('connector reads same-origin Gateway state through the actual management schema', async (t) => {
  const calls = response(t, { enabled: true, workers: [{ worker_id: 'chrome-a', kind: 'video', has_live_token: true, online: true, last_seen_at: 1794218400, credits: null, extension_version: '1.3.1', queue_depth: 0 }] });
  const data = await readWorkers();
  assert.equal(data.workers[0].workerId, 'chrome-a');
  assert.equal(data.workers[0].lastSeen, '2026-11-09T10:00:00.000Z');
  assert.equal(data.workers[0].telemetry.credits, null);
  assert.equal(data.workers[0].telemetry.queue.depth, 0);
  assert.equal(calls[0].url, '/admin/api/extension-workers');
  assert.equal(calls[0].credentials, 'same-origin');
  assert.equal(calls[0].cache, 'no-store');
  assert.equal(calls[0].headers.Authorization, undefined);
});

test('malformed worker response fails while the explicit 404 switch state is off', async (t) => {
  response(t, { ok: true });
  await assert.rejects(readWorkers(), /did not return a worker list/);
  t.mock.restoreAll();
  response(t, { detail: 'Not Found' }, 404);
  assert.deepEqual(await readWorkers(), { enabled: false, workers: [] });
});

test('pairing uses POST root and the actual worker_id/kind contract', async (t) => {
  const calls = response(t, { token: 'fixture-token' });
  assert.equal((await mintWorker({ workerId: 'fixture_worker', kind: 'image' })).token, 'fixture-token');
  assert.equal(calls[0].method, 'POST');
  assert.equal(calls[0].url, '/admin/api/extension-workers');
  assert.deepEqual(JSON.parse(calls[0].body), { worker_id: 'fixture_worker', kind: 'image' });
});

test('owner-only refusal is preserved without retrying or serializing error objects', async (t) => {
  const calls = response(t, { detail: 'Fixture local app required' }, 403);
  await assert.rejects(mintWorker({ workerId: 'x', kind: 'video' }), (error) => error.status === 403 && /local app required/.test(error.message));
  assert.equal(calls.length, 1);
});

test('non-JSON refusal keeps its HTTP status for sign-in recovery', async (t) => {
  t.mock.method(globalThis, 'fetch', async () => new Response('<html>sign in</html>', { status: 401 }));
  await assert.rejects(readWorkers(), (error) => error.status === 401 && !error.message.includes('<html>'));
});

test('revocation uses DELETE and escapes the id within its route', async (t) => {
  const calls = response(t, { ok: true });
  await revokeWorker('worker/a');
  assert.equal(calls[0].url, '/admin/api/extension-workers/worker%2Fa');
  assert.equal(calls[0].method, 'DELETE');
  assert.equal(calls[0].body, undefined);
});

test('Gateway can supply its existing authenticated request helper', async () => {
  const calls = [];
  await readWorkers(async (path, options) => { calls.push({ path, options }); return new Response('{"workers":[]}'); });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].path, '/admin/api/extension-workers');
});

test('unknown balance remains unknown, while a stated zero is retained', () => {
  for (const credits of [undefined, null, {}, { value: null }, { value: '0' }, { value: -1 }, { value: NaN }]) assert.equal(reportedCredit(credits), null);
  assert.equal(reportedCredit({ value: 0 }), 0);
  assert.equal(reportedCredit({ value: 11 }), 11);
});

test('waiting, online, offline, unknown and revoked remain distinct', () => {
  assert.equal(workerState({ online: false }).label, 'Waiting for extension');
  assert.equal(workerState({ online: true }).label, 'Connected');
  assert.equal(workerState({ online: false, lastSeen: '2026-11-09' }).label, 'Offline');
  assert.equal(workerState({}).label, 'Not checked');
  assert.equal(workerState({ revoked: true, online: true }).label, 'Access revoked');
});
