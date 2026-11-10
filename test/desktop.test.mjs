import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRecovery } from '../desktop/recovery.mjs';

const state = (reachable = true, origin = 'http://fixture.invalid') => ({ configured: true, reachable, origin });
test('recovery only opens an existing page on first connection or recovery', async () => {
  let value = { configured: false }; let opens = 0;
  const messages = [];
  const tick = createRecovery({ probe: async () => value, openGateway: async () => { opens++; }, report: text => messages.push(text) });
  await tick(); assert.equal(opens, 0);
  value = state(false); await tick(); assert.equal(opens, 0);
  value = state(); await tick(); await tick(); assert.equal(opens, 1);
  value = state(false); await tick(); value = state(); await tick(); assert.equal(opens, 2);
  value = state(true, 'http://different.invalid'); await tick(); assert.equal(opens, 3);
  assert.ok(messages.some(text => text.includes('no work is resubmitted')));
});

test('uncertain page opening retries navigation only, never claims a successful connection', async () => {
  let attempts = 0; let message;
  const tick = createRecovery({ probe: async () => state(), openGateway: async () => { if (++attempts === 1) throw new Error('unavailable'); }, report: text => { message = text; } });
  await tick(); assert.match(message, /Could not check/);
  await tick(); assert.equal(attempts, 2); assert.match(message, /sign-in and worker interface/);
});

test('overlapping recovery ticks do not create competing probes or windows', async () => {
  let release; let probes = 0; let opens = 0;
  const barrier = new Promise(resolve => { release = resolve; });
  const tick = createRecovery({ probe: async () => { probes++; await barrier; return state(); }, openGateway: async () => { opens++; }, report() {} });
  const first = tick(); await tick(); assert.equal(probes, 1);
  release(); await first; assert.equal(opens, 1);
});

test('local-only native capability and release metadata stay explicit', async () => {
  const load = async path => readFile(new URL(path, import.meta.url), 'utf8');
  const config = JSON.parse(await load('../src-tauri/tauri.conf.json'));
  const capability = JSON.parse(await load('../src-tauri/capabilities/local-shell.json'));
  const version = (await load('../VERSION')).trim();
  assert.equal(config.version, version);
  const versionInclude = (await load('../src-tauri/build.rs')).match(/include_str!\("([^"]+)"\)/)[1];
  assert.equal((await load(`../src-tauri/${versionInclude}`)).trim(), version);
  assert.equal(config.app.windows.length, 0);
  assert.deepEqual(config.app.security.capabilities, ['local-shell']);
  assert.equal(capability.local, true);
  assert.deepEqual(capability.windows, ['main']);
  assert.equal(capability.remote, undefined);
  assert.equal(config.bundle.active, false);
  assert.equal(config.bundle.macOS.signingIdentity, 'KH Studio Local Signing');
  assert.ok(!capability.permissions.some(value => /http|shell|fs|process|opener/.test(value)));
});
