// Opt-in real-Gateway fixture proof. Never targets a production URL or runs a provider.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, realpath, stat, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { basename, dirname, resolve } from 'node:path';
import { videoModule, videoRoot } from './source.mjs';
import { readWorkers } from '../web/api.mjs';

const lock = JSON.parse(await readFile(new URL('../scripts/gateway-proof-lock.json', import.meta.url), 'utf8'));
assert.equal(lock.schema, 1);
assert.equal(lock.status, 'reviewed-merged', 'Gateway proof awaits review of the final merged commit.');
assert.match(lock.gateway.commit, /^[0-9a-f]{40}$/);
assert.deepEqual(Object.keys(lock.gateway.source_hashes).sort(), ['gateway/adapters/chrome_extension.py', 'gateway/extension_workers.py']);
for (const hash of Object.values(lock.gateway.source_hashes)) assert.match(hash, /^[0-9a-f]{64}$/);
assert.ok(process.env.RELAY_VIDEO_SOURCE, 'Set RELAY_VIDEO_SOURCE to the exact reviewed video worktree.');
const catalog = JSON.parse(await readFile(new URL('../desktop/extensions.json', import.meta.url), 'utf8'));
assert.equal(catalog.extensions.find(entry => entry.id === 'video')?.source_commit, lock.video.commit, 'Video proof lock differs from the desktop candidate.');
async function verifiedVideoSource() {
  const git = (...args) => execFileSync('git', ['--no-optional-locks', '-C', videoRoot, ...args], { encoding: 'utf8' }).trim();
  assert.equal(git('status', '--porcelain', '--untracked-files=normal'), '', 'Video proof source must be clean.');
  const source = { commit: git('rev-parse', 'HEAD'), client_sha256: createHash('sha256').update(await readFile(resolve(videoRoot, 'src/lib/gateway-client.js'))).digest('hex') };
  assert.deepEqual(source, lock.video, 'Video client differs from the reviewed proof lock.');
  return source;
}
const video = await verifiedVideoSource();
const base = 'http://127.0.0.1:7916';
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
assert.equal(info.gateway_commit, lock.gateway.commit, 'Gateway proof commit differs from the reviewed merged source.');
assert.deepEqual(info.source_hashes, lock.gateway.source_hashes, 'Gateway proof bytes differ from the reviewed merged source.');
const proofRoot = resolve(process.env.RELAY_PROOF_ROOT || resolve(import.meta.dirname, '../../_evidence/worker-contract-20261109'));
assert.equal(dirname(info.root), await realpath(proofRoot));
assert.ok(basename(info.root).startsWith('gateway-http-proof-'));
const client = await videoModule('gateway-client.js');
const checks = [];
const tokens = [];
const scoped = (settings, body) => ({ method: 'POST', headers: { Authorization: `Bearer ${settings.gatewayToken}`, 'Content-Type': 'application/json' }, body });
const mp4 = Buffer.concat([Buffer.from([0, 0, 0, 16]), Buffer.from('ftypisom\0\0\0\0'), Buffer.from([0, 0, 0, 8]), Buffer.from('moov')]);
const expectedHash = createHash('sha256').update(mp4).digest('hex');
try {
  const settings = await minted('relay-proof-video');
  tokens.push(settings);
  const secondary = await minted('relay-proof-other');
  tokens.push(secondary);
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
} finally {
  const revoked = await Promise.allSettled(tokens.map(async s => {
    try {
      const response = await request(`/admin/api/extension-workers/${encodeURIComponent(s.gatewayWorkerId)}`, { method: 'DELETE' });
      assert.equal(response.status, 200);
      assert.equal((await request('/api/workers/health', { headers: { Authorization: `Bearer ${s.gatewayToken}` } })).status, 401);
    } finally {
      s.gatewayToken = '';
    }
  }));
  assert.ok(revoked.every(result => result.status === 'fulfilled'), 'Fixture token revocation was not fully verified.');
}
checks.push('fixture worker tokens revoked immediately; no raw token retained in receipt');
const after = await control('/info');
assert.deepEqual(after, info, 'Gateway proof identity or source changed during proof.');
assert.deepEqual(after.source_hashes, lock.gateway.source_hashes);
assert.equal(after.gateway_commit, lock.gateway.commit);
assert.deepEqual(await verifiedVideoSource(), video, 'Video source changed during proof.');
checks.push('exact reviewed Gateway and video source identities retained through receipt');
const receipt = { checked_at: new Date().toISOString(), scope: 'Reviewed merged Gateway source and pinned video HTTP client, isolated loopback fixture only; no provider generation or production runtime.', gateway: info, video, checks, artifact: { sha256: expectedHash, bytes: mp4.length }, no_paid_calls: true };
const output = process.env.RELAY_PROOF_RECEIPT;
if (output) await writeFile(output, JSON.stringify(receipt, null, 2) + '\n');
for (const check of checks) console.log(`PASS ${check}`);
