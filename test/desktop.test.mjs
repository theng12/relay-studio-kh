import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRecovery } from '../desktop/recovery.mjs';

const state = (reachable = true, origin = 'http://fixture.invalid') => ({ configured: true, reachable, origin });
test('a working address is reported and never opens a sign-in window by itself', async () => {
  let value = { configured: false };
  const messages = [];
  const tick = createRecovery({ probe: async () => value, report: text => messages.push(text) });
  await tick();
  value = state(false); await tick();
  value = state(); await tick(); await tick();
  value = state(true, 'http://different.invalid'); await tick();
  // Saving or health-checking an address must not put Gateway's sign-in on screen.
  assert.ok(messages.some(text => text.includes('Gateway responded')), messages.join(' | '));
  assert.ok(!messages.some(text => /sign-in and worker interface/.test(text)), 'no window is opened by the check');
  assert.ok(messages.some(text => text.includes('no work is resubmitted')));
});

test('an unreachable Gateway is reported honestly and never claims a connection', async () => {
  let message;
  const tick = createRecovery({ probe: async () => { throw new Error('unavailable'); }, report: text => { message = text; } });
  await tick(); assert.match(message, /Could not check/);
  await tick(); assert.match(message, /Could not check/);
});

test('overlapping recovery ticks do not create competing probes', async () => {
  let release; let probes = 0;
  const barrier = new Promise(resolve => { release = resolve; });
  const tick = createRecovery({ probe: async () => { probes++; await barrier; return state(); }, report() {} });
  const first = tick(); await tick(); assert.equal(probes, 1);
  release(); await first;
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
  assert.equal(config.build.beforeBuildCommand, 'npm run desktop:release-assets');
  assert.deepEqual(config.bundle.resources, { '../desktop-resources/': 'relay/' });
  const browserDefinitions = JSON.parse(await load('../desktop/browsers.json'));
  assert.deepEqual(browserDefinitions.map(browser => browser.id), ['chrome', 'edge', 'brave']);
  assert.equal(config.bundle.macOS.signingIdentity, 'KH Studio Local Signing');
  assert.ok(!capability.permissions.some(value => /http|shell|fs|process|opener/.test(value)));
});
