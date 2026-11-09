// Opt-in real-Gateway fixture proof. Never targets a production URL or runs a provider.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, stat, writeFile } from 'node:fs/promises';
import { videoModule } from './source.mjs';
import { readWorkers } from '../web/api.mjs';

const base = 'http://127.0.0.1:7916';
const client = await videoModule('gateway-client.js');
const request = (path, options) => fetch(base + path, options);
async function control(path, body) {
  const response = await request('/__relay_proof' + path, { method: body === undefined ? 'GET' : 'POST',
    headers: { 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  assert.equal(response.status, 200, `Fixture control failed (${response.status}).`);
  return response.json();
}
async function minted(workerId) {
  const response = await request('/admin/api/extension-workers', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ worker_id: workerId, kind: 'video' }) });
  assert.equal(response.status, 200);
  return { gatewayUrl: base, gatewayWorkerId: workerId, gatewayToken: (await response.json()).token };
}
const info = await control('/info');
assert.equal(info.fixture, true, 'Refuse any server that is not the dedicated fixture.');
assert.ok(info.gateway_commit.startsWith('5b0ed72'), 'Refuse the known-bad baseline or an unreviewed candidate.');
assert.ok(info.root.startsWith('/build/Developer/_orchestra/relay-lead/worker-contract-20261109/gateway-http-proof-'));
const checks = [];
const settings = await minted('relay-proof-video');
const secondary = await minted('relay-proof-other');
const scoped = (settings, body) => ({ method: 'POST', headers: { Authorization: `Bearer ${settings.gatewayToken}`, 'Content-Type': 'application/json' }, body });
const mp4 = Buffer.concat([Buffer.from([0, 0, 0, 16]), Buffer.from('ftypisom\0\0\0\0'), Buffer.from([0, 0, 0, 8]), Buffer.from('moov')]);
const expectedHash = createHash('sha256').update(mp4).digest('hex');
try {
  await client.testGateway(settings);
  assert.equal(await client.claimGatewayJob(settings), null);
  checks.push('original extension client authenticated health and idle204');
  let status = await readWorkers(request);
  assert.equal(status.workers.find(w => w.workerId === settings.gatewayWorkerId).telemetry.credits, null);
  const raw = `{"workerId":"${settings.gatewayWorkerId}","kind":"video","telemetry":{"credits":{"value":NaN,"source":"fixture.relay","observedAt":"2026-11-09T10:00:00Z"}}}`;
  assert.equal((await request('/api/workers/claim', scoped(settings, raw))).status, 204);
  status = await readWorkers(request);
  assert.equal(status.workers.find(w => w.workerId === settings.gatewayWorkerId).telemetry.credits, null);
  const zero = { workerId: settings.gatewayWorkerId, kind: 'video', telemetry: { credits: { value: 0, source: 'fixture.relay', observedAt: '2026-11-09T10:00:00Z' } } };
  assert.equal((await request('/api/workers/claim', scoped(settings, JSON.stringify(zero)))).status, 204);
  status = await readWorkers(request);
  assert.equal(status.workers.find(w => w.workerId === settings.gatewayWorkerId).telemetry.credits.value, 0);
  checks.push('corrected nonfinite telemetry stays JSON-safe/unknown; stated zero survives actual Relay readback');

  const success = await control('/offer', {});
  const first = await client.claimGatewayJob(settings);
  assert.equal(first.modelKey, info.model);
  assert.equal(first.durationSec, info.duration);
  const again = await client.claimGatewayJob(settings);
  assert.equal(again.jobId, first.jobId);
  await client.reportGatewayProgress(settings, first.jobId, 'fixture', 'Known bytes; no generation');
  await client.reportGatewayResult(settings, first.jobId, mp4, 'video/mp4');
  const done = await control('/settled/' + success.row_id);
  assert.equal(done.state, 'succeeded');
  assert.ok(done.artifacts.some(a => a.sha256 === expectedHash && a.bytes === mp4.length));
  checks.push('real Worker/adapter claim-dedup-progress-result writes exact artifact bytes');

  const image = await control('/offer?mode=image', {});
  const reference = await client.claimGatewayJob(settings);
  assert.equal(reference.videoMode, 'image');
  assert.equal(reference.referenceImages.length, 1);
  assert.equal(reference.referenceImages[0].mediaType, 'image/png');
  assert.equal(reference.referenceImages[0].dataBase64, 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+kfukAAAAASUVORK5CYII=');
  await client.reportGatewayResult(settings, reference.jobId, mp4, 'video/mp4');
  assert.equal((await control('/settled/' + image.row_id)).state, 'succeeded');
  checks.push('real image-key adapter stages exactly one reference and original client receives exact bytes');

  const held = await control('/offer', {});
  const lost = await client.claimGatewayJob(settings);
  const unknown = await control('/settled/' + held.row_id);
  assert.equal(unknown.unknown, true);
  assert.equal(await client.claimGatewayJob(secondary), null);
  await client.reportGatewayFailure(settings, lost.jobId, 'Fixture acknowledgement still unknown', true);
  assert.equal((await control('/settled/' + held.row_id)).unknown, true);
  await client.reportGatewayFailure(settings, lost.jobId, 'Fixture definitively refused', false);
  const failed = await control('/settled/' + held.row_id);
  assert.equal(failed.state, 'failed');
  assert.equal(failed.error_code, 'PROVIDER_FAILED');
  assert.equal(failed.unknown, false);
  checks.push('silent lease is durable/no reassignment; ambiguous late fail retains hold; definite late fail settles');

  const late = await control('/offer', {});
  const pending = await client.claimGatewayJob(settings);
  assert.equal((await control('/settled/' + late.row_id)).unknown, true);
  await assert.rejects(client.reportGatewayResult(secondary, pending.jobId, mp4, 'video/mp4'), /404/);
  await client.reportGatewayResult(settings, pending.jobId, mp4, 'video/mp4');
  assert.equal((await control('/settled/' + late.row_id)).state, 'succeeded');
  checks.push('late exact-owner multipart result adopts durable row; another worker refused');
  const tokenPath = info.root + '/extension-workers.json';
  const stored = JSON.parse(await readFile(tokenPath, 'utf8')).workers;
  assert.equal((await stat(tokenPath)).mode & 0o777, 0o600);
  for (const [digest, row] of Object.entries(stored)) {
    assert.match(digest, /^[a-f0-9]{64}$/);
    assert.deepEqual(Object.keys(row).sort(), ['created_at', 'kind', 'revoked_at', 'worker_id']);
  }
  checks.push('real token store contains only SHA256 keys and bounded metadata with0600 permissions');
  const after = await control('/info');
  assert.deepEqual(after.source_hashes, info.source_hashes, 'Gateway source changed during proof.');
} finally {
  for (const s of [settings, secondary]) {
    const response = await request(`/admin/api/extension-workers/${encodeURIComponent(s.gatewayWorkerId)}`, { method: 'DELETE' });
    assert.equal(response.status, 200);
    assert.equal((await request('/api/workers/health', { headers: { Authorization: `Bearer ${s.gatewayToken}` } })).status, 401);
    s.gatewayToken = '';
  }
}
checks.push('fixture worker tokens revoked immediately; no raw token retained in receipt');
const receipt = { checked_at: new Date().toISOString(), scope: 'Real Gateway source and original extension HTTP client, isolated loopback fixture only; no provider generation or production runtime.', gateway: info, checks, artifact: { sha256: expectedHash, bytes: mp4.length }, no_paid_calls: true };
const output = process.env.RELAY_PROOF_RECEIPT;
if (output) await writeFile(output, JSON.stringify(receipt, null, 2) + '\n');
for (const check of checks) console.log(`PASS ${check}`);
