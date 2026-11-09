import test from 'node:test';
import assert from 'node:assert/strict';
import { patchFixture } from './patch-fixture.mjs';

const { sourceText } = await patchFixture('video-flow-credits.patch', 'src/lib/flow-exec.js');
const { flowCommand } = await import(`data:text/javascript;base64,${Buffer.from(sourceText).toString('base64')}`);

function rpc(t, result, { signedIn = true, status = 200 } = {}) {
  const previous = { window: globalThis.window, location: globalThis.location };
  globalThis.window = { WIZ_global_data: signedIn ? { SNlM0e: 'fixture-at' } : {} };
  globalThis.location = { pathname: '/project/fixture-project' };
  t.after(() => {
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) delete globalThis[key];
      else globalThis[key] = value;
    }
  });
  const calls = [];
  t.mock.method(globalThis, 'fetch', async (url, init) => {
    calls.push({ url, init });
    const frame = [['wrb.fr', 'nzlxg', JSON.stringify(result)]];
    return new Response(")]}'\n" + JSON.stringify(frame), { status });
  });
  return calls;
}

test('credit read uses captured empty args and reports observed balance with provenance', async (t) => {
  const calls = rpc(t, [11, 3, 8, 1, null, 11, [1000, 0]]);
  const before = Date.now();
  const response = await flowCommand({ op: 'getCredits' });
  assert.equal(response.ok, true);
  assert.equal(response.result.value, 11);
  assert.equal(response.result.source, 'flow.nzlxg');
  assert.ok(Date.parse(response.result.observedAt) >= before);
  assert.ok(Date.parse(response.result.observedAt) <= Date.now());
  assert.equal(calls.length, 1);
  assert.match(calls[0].url, /^\/_\/AiSandboxAngularFrontend\/data\/batchexecute\?/);
  assert.equal(new URL(calls[0].url, 'https://flow.invalid').searchParams.get('rpcids'), 'nzlxg');
  const body = new URLSearchParams(calls[0].init.body);
  assert.deepEqual(JSON.parse(body.get('f.req')), [[['nzlxg', '[]', null, 'generic']]]);
  assert.equal(calls[0].init.credentials, 'include');
});

test('a stated zero balance remains zero', async (t) => {
  rpc(t, [0]);
  const response = await flowCommand({ op: 'getCredits' });
  assert.equal(response.ok, true);
  assert.equal(response.result.value, 0);
});

for (const [name, value] of [['missing', []], ['null', [null]], ['numeric string', ['11']], ['negative', [-1]]]) {
  test(`${name} balance fails explicitly instead of becoming zero`, async (t) => {
    rpc(t, value);
    const response = await flowCommand({ op: 'getCredits' });
    assert.equal(response.ok, false);
    assert.match(response.error, /no valid credit balance/);
    assert.equal(response.result, undefined);
  });
}

test('signed-out credit read fails locally without a network request', async (t) => {
  const calls = rpc(t, [11], { signedIn: false });
  const response = await flowCommand({ op: 'getCredits' });
  assert.equal(response.ok, false);
  assert.match(response.error, /Not signed in/);
  assert.equal(calls.length, 0);
});

test('credit RPC rejection is explicit and never retried', async (t) => {
  const calls = rpc(t, null, { status: 403 });
  const response = await flowCommand({ op: 'getCredits' });
  assert.equal(response.ok, false);
  assert.match(response.error, /403/);
  assert.equal(calls.length, 1);
});
