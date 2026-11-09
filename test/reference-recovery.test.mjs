import test from 'node:test';
import assert from 'node:assert/strict';
import { background, worker, JOB } from './background-fixture.mjs';
import { patchFixture } from './patch-fixture.mjs';

const { sourceText } = await patchFixture('video-reference-recovery.patch', 'src/background.js');

const reference = { mediaType: 'image/png', dataBase64: 'AQID' };
function imageItem() {
  return worker.gatewayJobToItem({ ...JOB, videoMode: 'image', referenceImages: [reference] }, {});
}
function stage(h, item) {
  h.stored.videoQueue = [item];
  h.stored.gatewayJobReferences = { [JOB.jobId]: [reference], unrelated: [{ dataBase64: 'BAUG' }] };
}
const completedFlow = async ({ op }) => {
  if (op === 'listVideoModels') return { entries: [], signedIn: true };
  if (op === 'uploadImage') return { mediaId: 'fixture_reference' };
  if (op === 'submitVideo') return { mediaId: 'fixture_video' };
  if (op === 'checkVideo') return { done: true };
  if (op === 'getMedia') return { url: 'https://flow-content.google/video/fixture' };
  throw new Error(`Unexpected operation: ${op}`);
};

test('failed reference upload preserves bytes and retry uses the same reference', async () => {
  let uploadFailed = false;
  const operations = [];
  const h = await background({ sourceText, flow: async (command) => {
    operations.push(command.op);
    if (command.op === 'uploadImage' && !uploadFailed) {
      uploadFailed = true;
      throw new Error('Fixture upload disconnected');
    }
    if (command.op === 'uploadImage') assert.equal(command.payload.base64, reference.dataBase64);
    return completedFlow(command);
  } });
  const item = imageItem();
  stage(h, item);
  await assert.rejects(h.api.generateAndSave(item, h.settings, { id: 1 }), /upload disconnected/);
  assert.equal(h.stored.gatewayJobReferences[JOB.jobId][0].dataBase64, 'AQID');
  assert.equal(operations.includes('submitVideo'), false);
  await h.api.generateAndSave(item, h.settings, { id: 1 });
  assert.equal(operations.filter((op) => op === 'uploadImage').length, 2);
  assert.equal(operations.filter((op) => op === 'submitVideo').length, 1);
  assert.equal(operations.includes('generateImage'), false);
  assert.equal(h.stored.gatewayJobReferences[JOB.jobId], undefined);
  assert.equal(h.stored.gatewayJobReferences.unrelated[0].dataBase64, 'BAUG');
});

test('failed result upload retains reference and known media identity', async () => {
  const operations = [];
  const h = await background({ sourceText, resultError: new Error('Fixture result disconnected'), flow: async (command) => {
    operations.push(command.op);
    return completedFlow(command);
  } });
  const item = imageItem();
  stage(h, item);
  await assert.rejects(h.api.generateAndSave(item, h.settings, { id: 1 }), /result disconnected/);
  assert.equal(h.stored.gatewayJobReferences[JOB.jobId][0].dataBase64, 'AQID');
  assert.equal(h.stored.videoQueue[0].videoMediaId, 'fixture_video');
  assert.equal(operations.filter((op) => op === 'submitVideo').length, 1);
});

test('missing staged reference refuses instead of spending on a replacement image', async () => {
  const operations = [];
  const h = await background({ sourceText, flow: async (command) => {
    operations.push(command.op);
    return completedFlow(command);
  } });
  const item = imageItem();
  h.stored.videoQueue = [item];
  await assert.rejects(h.api.generateAndSave(item, h.settings, { id: 1 }), /reference image is no longer available/);
  assert.deepEqual(operations, ['listVideoModels']);
  assert.equal(h.reports.some(([kind]) => kind === 'result'), false);
});

test('patched text-only Gateway work still returns a result without reference upload', async () => {
  const operations = [];
  const h = await background({ sourceText, flow: async (command) => {
    operations.push(command.op);
    return completedFlow(command);
  } });
  const item = worker.gatewayJobToItem(JOB, {});
  h.stored.videoQueue = [item];
  await h.api.generateAndSave(item, h.settings, { id: 1 });
  assert.deepEqual(operations, ['listVideoModels', 'submitVideo', 'checkVideo', 'getMedia']);
  assert.equal(h.reports.filter(([kind]) => kind === 'result').length, 1);
});
