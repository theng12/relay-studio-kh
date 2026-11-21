import test from 'node:test';
import assert from 'node:assert/strict';
import { copyFile, cp, mkdir, mkdtemp, readFile, rename, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';

const source = fileURLToPath(new URL('..', import.meta.url));
const sharedSource = resolve(process.env.KH_SHARED_UI || resolve(source, '../../Shared Tools/kh-shared-ui'));
const unicodeNote = 'Fixture: ខ្មែរ — café';
async function fixture() {
  const workspace = await mkdtemp(join(process.env.RELAY_TEST_ROOT || tmpdir(), 'relay build fixture '));
  // The real layout (owner, 2026-11-10): apps under `My Own Apps/`, shared tools in
  // `Shared Tools/`, so build.mjs finds kh-shared-ui two levels above the checkout.
  const root = join(workspace, 'My Own Apps', 'Relay checkout');
  const shared = join(workspace, 'Shared Tools', 'kh-shared-ui');
  await mkdir(join(root, 'scripts'), { recursive: true });
  await mkdir(shared, { recursive: true });
  await copyFile(join(source, 'scripts/build.mjs'), join(root, 'scripts/build.mjs'));
  await copyFile(join(source, 'VERSION'), join(root, 'VERSION'));
  const changelog = (await readFile(join(source, 'CHANGELOG.md'), 'utf8')).replace(/(##[^\n]+\n)/, `$1- ${unicodeNote}\n`);
  await writeFile(join(root, 'CHANGELOG.md'), changelog);
  await cp(join(source, 'web'), join(root, 'web'), { recursive: true });
  for (const name of ['whats-new.py', 'design-tokens.json']) await copyFile(join(sharedSource, name), join(shared, name));
  const env = { ...process.env };
  delete env.KH_SHARED_UI;
  return { workspace, root, shared, env };
}
function build(f, overrides = {}) {
  return spawnSync(process.execPath, [join(f.root, 'scripts/build.mjs')], {
    cwd: f.workspace, env: { ...f.env, ...overrides }, encoding: 'utf8', timeout: 15000,
  });
}
async function verifyOutput(f) {
  const version = (await readFile(join(f.root, 'VERSION'), 'utf8')).trim();
  const fragment = await readFile(join(f.root, 'dist/relay-pane.html'), 'utf8');
  const bundle = await readFile(join(f.root, 'dist/relay-pane.js'), 'utf8');
  const preview = await readFile(join(f.root, 'dist/index.html'), 'utf8');
  const sandbox = { window: {} };
  runInNewContext(bundle, sandbox);
  assert.equal(typeof sandbox.window.mountRelayConnector, 'function');
  assert.ok(bundle.includes(JSON.stringify(version)));
  assert.ok(bundle.includes(unicodeNote));
  assert.match(fragment, /\.relay-connector/);
  assert.ok(preview.includes(fragment));
  assert.match(preview, /src="\.\/relay-pane\.js"/);
  assert.equal(await readFile(join(f.root, 'dist/bootstrap.mjs'), 'utf8'), await readFile(join(source, 'web/bootstrap.mjs'), 'utf8'));
  const tokens = await readFile(join(f.root, 'dist/tokens.css'), 'utf8');
  assert.match(tokens, /^:root\{--space-xs:\d+px;/);
  assert.match(tokens, /--space-page:\d+px;.*--type-body:\d+px/);
}

test('real shared tools build from sibling checkout with spaces, arbitrary cwd and Unicode', async () => {
  const f = await fixture();
  const result = build(f);
  assert.equal(result.status, 0, result.stderr || String(result.error));
  await verifyOutput(f);
});

test('shared-directory and interpreter executable overrides remain supported', async () => {
  const f = await fixture();
  await rename(f.shared, join(f.workspace, 'shared tools elsewhere'));
  const python = process.env.RELAY_PYTHON || (process.platform === 'win32' ? 'python' : 'python3');
  const result = build(f, { KH_SHARED_UI: './shared tools elsewhere', RELAY_PYTHON: python });
  assert.equal(result.status, 0, result.stderr || String(result.error));
  await verifyOutput(f);
});

test('missing shared tools fail with setup guidance before writing a bundle', async () => {
  const f = await fixture();
  const result = build(f, { KH_SHARED_UI: './missing shared tools' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /set KH_SHARED_UI/);
  assert.equal(existsSync(join(f.root, 'dist')), false);
});

test('an unavailable interpreter override fails instead of silently falling back', async () => {
  const f = await fixture();
  const result = build(f, { RELAY_PYTHON: join(f.workspace, 'missing python executable') });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /missing python executable/);
  assert.equal(existsSync(join(f.root, 'dist')), false);
});

test('release mismatch is still refused before writing a bundle', async () => {
  const f = await fixture();
  await writeFile(join(f.root, 'VERSION'), '999.0.0\n');
  const result = build(f);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /newest CHANGELOG entry must match/);
  assert.equal(existsSync(join(f.root, 'dist')), false);
});
