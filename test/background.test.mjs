import test from 'node:test';
import assert from 'node:assert/strict';
import { background, worker, JOB } from './background-fixture.mjs';

test('Story Studio and standalone modes never claim Gateway work', async () => {
  const h = await background();
  h.setClaim(JOB);
  for (const mode of ['studio', 'standalone']) {
    h.settings.mode = mode;
    assert.equal(await h.api.gatewayTick(), null);
  }
  assert.equal(h.stored.videoQueue, undefined);
  assert.equal(h.reports.length, 0);
});

test('claim stages references separately and deduplicates the queued id', async () => {
  const h = await background();
  h.settings.mode = 'gateway';
  h.stored.videoQueuePaused = true;
  h.setClaim({ ...JOB, videoMode: 'image', referenceImages: [{ mediaType: 'image/png', dataBase64: 'AQID' }] });
  const item = await h.api.gatewayTick();
  assert.equal(item.gatewayJobId, JOB.jobId);
  assert.equal(item.videoSource, 'existing');
  assert.equal(item.referenceImages, undefined);
  assert.equal(h.stored.gatewayJobReferences[JOB.jobId][0].dataBase64, 'AQID');
  assert.equal(await h.api.gatewayTick(), null);
  assert.equal(h.stored.videoQueue.length, 1);
  assert.equal(h.reports.filter(([kind]) => kind === 'progress').length, 1);
});

test('a known media id polls and returns bytes without paid resubmission', async () => {
  const operations = [];
  const h = await background({ flow: async ({ op }) => {
    operations.push(op);
    if (op === 'checkVideo') return { done: true };
    if (op === 'getMedia') return { url: 'https://flow-content.google/video/fixture' };
    throw new Error(`Unexpected operation: ${op}`);
  } });
  const item = { ...worker.gatewayJobToItem(JOB, {}), videoMediaId: 'fixture_media' };
  h.stored.videoQueue = [item];
  h.stored.gatewayJobReferences = { [JOB.jobId]: [{ dataBase64: 'AQID' }] };
  const name = await h.api.generateAndSave(item, h.settings, { id: 1 });
  assert.equal(name, `${JOB.jobId}.mp4`);
  assert.deepEqual(operations, ['checkVideo', 'getMedia']);
  const result = h.reports.find(([kind]) => kind === 'result');
  assert.equal(result[1], JOB.jobId);
  assert.deepEqual(new Uint8Array(result[2]), Uint8Array.from([1, 2, 3]));
  assert.equal(result[3], 'video/mp4');
  assert.equal(h.stored.gatewayJobReferences[JOB.jobId], undefined);
});

test('a staged reference travels claim through upload, submit, poll and result', async () => {
  const operations = [];
  const h = await background({ flow: async ({ op, payload }) => {
    operations.push(op);
    if (op === 'listVideoModels') return { entries: [], signedIn: true };
    if (op === 'uploadImage') {
      assert.equal(payload.base64, 'AQID');
      assert.equal(payload.mime, 'image/png');
      return { mediaId: 'fixture_reference' };
    }
    if (op === 'submitVideo') {
      assert.equal(payload.mode, 'image');
      assert.equal(payload.mediaId, 'fixture_reference');
      assert.equal(h.stored.videoQueue[0].phase, 'submitting');
      assert.ok(h.stored.videoQueue[0].submissionStartedAt);
      return { mediaId: 'fixture_video' };
    }
    if (op === 'checkVideo') return { done: true };
    if (op === 'getMedia') return { url: 'https://flow-content.google/video/fixture' };
    throw new Error(`Unexpected operation: ${op}`);
  } });
  h.settings.mode = 'gateway';
  h.stored.videoQueuePaused = true;
  h.setClaim({ ...JOB, videoMode: 'image', referenceImages: [{ mediaType: 'image/png', dataBase64: 'AQID' }] });
  await h.api.gatewayTick();
  await new Promise(setImmediate);
  h.stored.videoQueuePaused = false;
  await h.api.processQueue();
  assert.deepEqual(operations, ['listVideoModels', 'uploadImage', 'submitVideo', 'checkVideo', 'getMedia']);
  assert.equal(h.stored.videoQueue[0].status, 'done');
  assert.equal(h.stored.videoQueue[0].videoMediaId, 'fixture_video');
  assert.equal(h.reports.filter(([kind]) => kind === 'result').length, 1);
  assert.equal(h.reports.filter(([kind]) => kind === 'fail').length, 0);
  assert.equal(h.stored.gatewayJobReferences[JOB.jobId], undefined);
});

test('lost paid submission acknowledgement reports ambiguity and pauses', async () => {
  const h = await background({ flow: async ({ op }) => {
    if (op === 'listVideoModels') return { entries: [], signedIn: true };
    if (op === 'submitVideo') throw new Error('Fixture response lost');
    throw new Error(`Unexpected operation: ${op}`);
  } });
  h.stored.videoQueue = [worker.gatewayJobToItem(JOB, {})];
  await h.api.processQueue();
  assert.equal(h.stored.videoQueue[0].status, 'ambiguous');
  assert.equal(h.stored.videoQueuePaused, true);
  const failure = h.reports.find(([kind]) => kind === 'fail');
  assert.equal(failure[1], JOB.jobId);
  assert.equal(failure[3], true);
  const count = h.reports.length;
  await h.api.processQueue();
  assert.equal(h.reports.length, count);
});
