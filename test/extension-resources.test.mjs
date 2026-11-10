import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { prepareExtensionResources } from '../scripts/extension-resources.mjs';

const sha = (buf) => createHash('sha256').update(buf).digest('hex');
const COMMIT = 'a'.repeat(40);

function fixture() {
  const root = fs.mkdtempSync(path.join(process.env.RELAY_TEST_ROOT || os.tmpdir(), 'relay-extension-resources '));
  const payloadRoot = path.join(root, 'payloads');
  fs.mkdirSync(payloadRoot, { recursive: true });
  return { root, payloadRoot, catalogPath: path.join(root, 'extensions.json'), out: () => path.join(root, `out-${Math.random().toString(36).slice(2)}`) };
}

function writeCatalog(f, extensions) {
  fs.writeFileSync(f.catalogPath, JSON.stringify({ schema: 1, extensions }, null, 2));
}

const unavailable = (id, reason = `${id} extension not reviewed yet`) => ({ id, name: `${id} connector`, version: null, source_commit: null, accepted: false, reason });

// Writes files under payloadRoot/{id} and returns an accepted catalog entry with a matching lock.
function acceptedEntry(f, id, files, { version = '1.2.3', tamper, manifest = { manifest_version: 3, version: '1.2.3' } } = {}) {
  const dir = path.join(f.payloadRoot, id);
  fs.mkdirSync(dir, { recursive: true });
  const manifestBytes = Buffer.from(JSON.stringify(manifest));
  fs.writeFileSync(path.join(dir, 'manifest.json'), manifestBytes);
  const lock = [{ path: 'manifest.json', sha256: sha(manifestBytes) }];
  for (const [name, content] of Object.entries(files)) {
    const bytes = Buffer.from(tamper === name ? content + '!' : content);
    fs.writeFileSync(path.join(dir, name), bytes);
    lock.push({ path: name, sha256: sha(Buffer.from(content)) });
  }
  return { id, name: `${id} connector`, version, source_commit: COMMIT, accepted: true, manifest_sha256: sha(manifestBytes), files: lock };
}

test('all-unavailable catalog stays honest; require-complete blocks the release', () => {
  const f = fixture();
  writeCatalog(f, [unavailable('video'), unavailable('image'), unavailable('audio')]);
  const outRoot = f.out();
  const result = prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: outRoot, payloadRoot: f.payloadRoot });
  assert.deepEqual(result, { rows: [
    { id: 'video', name: 'video connector', version: null, source_commit: null, available: false, required: true, reason: 'video extension not reviewed yet', files: [], payload_sha256: null },
    { id: 'image', name: 'image connector', version: null, source_commit: null, available: false, required: true, reason: 'image extension not reviewed yet', files: [], payload_sha256: null },
    { id: 'audio', name: 'audio connector', version: null, source_commit: null, available: false, required: true, reason: 'audio extension not reviewed yet', files: [], payload_sha256: null },
  ], available: 0, total: 3 });
  assert.equal(fs.existsSync(path.join(outRoot, 'extensions', 'video')), false);
  const strictOut = f.out();
  assert.throws(
    () => prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: strictOut, payloadRoot: f.payloadRoot, requireComplete: true }),
    /require-complete.*unavailable/,
  );
});

test('accepted MV3 payload is validated, staged and hashed deterministically', () => {
  const f = fixture();
  const content = 'chrome.runtime.onMessage.addListener(() => {});\n';
  const entry = acceptedEntry(f, 'video', { 'content.js': content });
  writeCatalog(f, [entry, unavailable('image'), unavailable('audio')]);
  const outRoot = f.out();
  const result = prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: outRoot, payloadRoot: f.payloadRoot });
  assert.throws(() => prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: f.out() }), /set RELAY_EXTENSION_PAYLOADS/);
  assert.equal(result.available, 1);
  const row = result.rows[0];
  assert.equal(row.available, true);
  assert.equal(row.reason, null);
  assert.deepEqual(row.files.map((x) => x.path), ['content.js', 'manifest.json']);
  const expected = sha(Buffer.from(row.files.map((x) => `${x.path} ${x.sha256}`).sort().join('\n')));
  assert.equal(row.payload_sha256, expected);
  assert.equal(fs.readFileSync(path.join(outRoot, 'extensions', 'video', 'content.js'), 'utf8'), content);
  const index = JSON.parse(fs.readFileSync(path.join(outRoot, 'extensions', 'index.json'), 'utf8'));
  assert.equal(index.extensions.length, 3);
  assert.equal(index.extensions[1].available, false);
});

test('tampered bytes, wrong version and missing payload fail loudly, never downgrade', () => {
  const f = fixture();
  const entry = acceptedEntry(f, 'video', { 'content.js': 'x' }, { tamper: 'content.js' });
  writeCatalog(f, [entry, unavailable('image'), unavailable('audio')]);
  assert.throws(() => prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: f.out(), payloadRoot: f.payloadRoot }), /hash mismatch for content.js/);

  const f2 = fixture();
  const mismatch = acceptedEntry(f2, 'video', { 'content.js': 'x' }, { version: '9.9.9' });
  writeCatalog(f2, [mismatch, unavailable('image'), unavailable('audio')]);
  assert.throws(() => prepareExtensionResources({ catalogPath: f2.catalogPath, outputRoot: f2.out(), payloadRoot: f2.payloadRoot }), /catalog version/);

  const f3 = fixture();
  writeCatalog(f3, [{ id: 'video', name: 'video connector', version: '1.0.0', source_commit: COMMIT, accepted: true, manifest_sha256: 'b'.repeat(64), files: [{ path: 'manifest.json', sha256: 'c'.repeat(64) }] }, unavailable('image'), unavailable('audio')]);
  assert.throws(() => prepareExtensionResources({ catalogPath: f3.catalogPath, outputRoot: f3.out(), payloadRoot: f3.payloadRoot }), /payload directory missing/);
});

test('traversal, unlisted files and symlinks in the payload are refused', (t) => {
  const f = fixture();
  const traversal = acceptedEntry(f, 'video', { 'content.js': 'x' });
  traversal.files.push({ path: '../escape', sha256: 'd'.repeat(64) });
  writeCatalog(f, [traversal, unavailable('image'), unavailable('audio')]);
  assert.throws(() => prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: f.out(), payloadRoot: f.payloadRoot }), /bad lock entry/);

  const f2 = fixture();
  acceptedEntry(f2, 'video', { 'content.js': 'x' });
  fs.writeFileSync(path.join(f2.payloadRoot, 'video', 'notes.txt'), 'unreviewed');
  writeCatalog(f2, [acceptedEntry(f2, 'video', { 'content.js': 'x' }), unavailable('image'), unavailable('audio')]);
  assert.throws(() => prepareExtensionResources({ catalogPath: f2.catalogPath, outputRoot: f2.out(), payloadRoot: f2.payloadRoot }), /unlisted files notes.txt/);

  const f3 = fixture();
  const entry = acceptedEntry(f3, 'video', { 'content.js': 'x' });
  try {
    fs.symlinkSync(path.join(f3.payloadRoot, 'video', 'content.js'), path.join(f3.payloadRoot, 'video', 'link.js'));
  } catch {
    t.skip('volume without symlink support');
    return;
  }
  writeCatalog(f3, [entry, unavailable('image'), unavailable('audio')]);
  assert.throws(() => prepareExtensionResources({ catalogPath: f3.catalogPath, outputRoot: f3.out(), payloadRoot: f3.payloadRoot }), /symlink refused/);
});

test('strict release accepts all three reviewed payloads and rejects malformed versions', () => {
  const f = fixture();
  const entries = ['video', 'image', 'audio'].map(id => acceptedEntry(f, id, { 'worker.js': '/* fixture */' }));
  writeCatalog(f, entries);
  assert.equal(prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: f.out(), payloadRoot: f.payloadRoot, requireComplete: true }).available, 3);
  entries[0].version = '1.70000'; writeCatalog(f, entries);
  assert.throws(() => prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: f.out(), payloadRoot: f.payloadRoot, requireComplete: true }), /valid Chrome manifest version/);
});

test('owner-skipped optional audio never blocks required image/video or silently becomes available', () => {
  const f = fixture();
  const video = acceptedEntry(f, 'video', { 'worker.js': '/* reviewed video fixture */' });
  const image = acceptedEntry(f, 'image', { 'worker.js': '/* reviewed image fixture */' });
  const audio = { ...unavailable('audio', 'Owner skipped browser TTS; optional off'), required: false };
  writeCatalog(f, [video, image, audio]);
  const out = f.out();
  const result = prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: out, payloadRoot: f.payloadRoot, requireComplete: true });
  assert.equal(result.available, 2);
  assert.deepEqual(result.rows.map(row => [row.id, row.required, row.available]), [['video', true, true], ['image', true, true], ['audio', false, false]]);
  assert.equal(fs.existsSync(path.join(out, 'extensions', 'audio')), false);
  writeCatalog(f, [video, unavailable('image'), audio]);
  assert.throws(() => prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: f.out(), payloadRoot: f.payloadRoot, requireComplete: true }), /required extensions unavailable: image/);
  writeCatalog(f, [video, image, { ...audio, required: 'false' }]);
  assert.throws(() => prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: f.out(), payloadRoot: f.payloadRoot, requireComplete: true }), /required must be true or false/);
});

test('existing output root is refused, never wiped', () => {
  const f = fixture();
  writeCatalog(f, [unavailable('video'), unavailable('image'), unavailable('audio')]);
  const outRoot = f.out();
  prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: outRoot, payloadRoot: f.payloadRoot });
  assert.throws(() => prepareExtensionResources({ catalogPath: f.catalogPath, outputRoot: outRoot, payloadRoot: f.payloadRoot }), /refusing to overwrite/);
  assert.equal(fs.existsSync(path.join(outRoot, 'extensions', 'index.json')), true);
});
