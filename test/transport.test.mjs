import test from 'node:test';
import assert from 'node:assert/strict';
import { videoModule } from './source.mjs';

const client = await videoModule('gateway-client.js');
const settings = { gatewayUrl: 'http://relay.invalid///', gatewayToken: 'fixture-only', gatewayWorkerId: 'worker_fixture' };
const job = { jobId: 'job/one', kind: 'video', prompt: 'Fixture', videoMode: 'text', modelKey: 'fixture_model', flowProjectId: 'fixture_project' };

function intercept(t, reply = { ok: true }, status = 200) {
  const calls = [];
  t.mock.method(globalThis, 'fetch', async (url, init) => {
    calls.push({ url, ...init });
    return new Response(status === 204 ? null : JSON.stringify(reply), { status });
  });
  return calls;
}

// Keep interception serial: no real network or shared global fetch races.
test('health uses outbound bearer auth and normalized base URL', async (t) => {
  const calls = intercept(t);
  assert.equal((await client.testGateway(settings)).ok, true);
  assert.equal(calls[0].url, 'http://relay.invalid/api/workers/health');
  assert.equal(calls[0].method, 'GET');
  assert.equal(calls[0].headers.Authorization, 'Bearer fixture-only');
  assert.ok(calls[0].signal instanceof AbortSignal);
});

test('claim sends only worker identity and video kind', async (t) => {
  const calls = intercept(t, { job });
  assert.deepEqual(await client.claimGatewayJob(settings), job);
  assert.equal(calls[0].url, 'http://relay.invalid/api/workers/claim');
  assert.deepEqual(JSON.parse(calls[0].body), { workerId: 'worker_fixture', kind: 'video' });
});

test('empty claim is idle', async (t) => {
  intercept(t, null, 204);
  assert.equal(await client.claimGatewayJob(settings), null);
});

test('missing worker identity refuses before transport', async (t) => {
  const calls = intercept(t);
  await assert.rejects(client.claimGatewayJob({ ...settings, gatewayWorkerId: '' }), /worker id/);
  assert.equal(calls.length, 0);
});

test('progress escapes job identity and bounds message', async (t) => {
  const calls = intercept(t);
  await client.reportGatewayProgress(settings, job.jobId, 'polling', 'x'.repeat(400));
  assert.equal(calls[0].url, 'http://relay.invalid/api/workers/jobs/job%2Fone/progress');
  assert.deepEqual(JSON.parse(calls[0].body), { phase: 'polling', message: 'x'.repeat(300) });
});

test('result uses multipart bytes with mediaType and browser boundary', async (t) => {
  const calls = intercept(t);
  const bytes = Uint8Array.from([0, 0, 0, 24, 102, 116, 121, 112]);
  await client.reportGatewayResult(settings, job.jobId, bytes.buffer);
  assert.equal(calls[0].url, 'http://relay.invalid/api/workers/jobs/job%2Fone/result');
  assert.equal(calls[0].headers['Content-Type'], undefined);
  assert.ok(calls[0].body instanceof FormData);
  assert.equal(calls[0].body.get('mediaType'), 'video/mp4');
  const file = calls[0].body.get('file');
  assert.equal(file.type, 'video/mp4');
  assert.deepEqual(new Uint8Array(await file.arrayBuffer()), bytes);
});

test('failure carries explicit paid ambiguity without retries', async (t) => {
  const calls = intercept(t);
  await client.reportGatewayFailure(settings, job.jobId, new Error('Outcome unknown'), true);
  assert.equal(calls[0].url, 'http://relay.invalid/api/workers/jobs/job%2Fone/fail');
  assert.deepEqual(JSON.parse(calls[0].body), { error: 'Outcome unknown', ambiguous: true });
  assert.equal(calls.length, 1);
});

test('failure text is bounded and ambiguity requires boolean true', async (t) => {
  const calls = intercept(t);
  await client.reportGatewayFailure(settings, job.jobId, 'x'.repeat(600), 'true');
  assert.deepEqual(JSON.parse(calls[0].body), { error: 'x'.repeat(500), ambiguous: false });
});

test('an HTTP refusal fails loudly without transport replay', async (t) => {
  const calls = intercept(t, { detail: 'Fixture auth refused' }, 401);
  await assert.rejects(client.claimGatewayJob(settings), /401.*Fixture auth refused/);
  assert.equal(calls.length, 1);
});

test('missing job ids do not send reports', async (t) => {
  const calls = intercept(t);
  await client.reportGatewayProgress(settings, '', 'queued');
  await client.reportGatewayFailure(settings, '', 'error');
  await client.reportGatewayResult(settings, '', new ArrayBuffer(0));
  assert.equal(calls.length, 0);
});
