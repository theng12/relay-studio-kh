import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { compareVersions, guardBundleAgainstRepos, refreshCatalogLock, stagePinnedPayloads } from '../scripts/extension-sources.mjs';

const IDS = ['video', 'image', 'audio'];
const REPOS = {
  video: 'story-studio-video-generator',
  image: 'story-studio-image-generator',
  audio: 'story-studio-audio-generator',
};

const git = (repo, args) => execFileSync('git', ['-C', repo, '-c', 'user.email=test@example.com', '-c', 'user.name=Test', ...args], { encoding: 'utf8' });

function repoDir(f, id) {
  return path.join(f.base, 'chrome-extensions', REPOS[id]);
}

// One committed extension per id; video's repo deliberately has no Gateway account module.
function fixture() {
  const base = fs.mkdtempSync(path.join(process.env.RELAY_TEST_ROOT || os.tmpdir(), 'relay extension sources '));
  const root = path.join(base, 'relay');
  fs.mkdirSync(root, { recursive: true });
  const catalog = { schema: 1, extensions: [] };
  for (const id of IDS) {
    const repo = repoDir({ base }, id);
    fs.mkdirSync(path.join(repo, 'src/lib'), { recursive: true });
    git(repo, ['init', '-q']);
    fs.writeFileSync(path.join(repo, 'CHANGELOG.md'), `# ${id}\n`);
    fs.writeFileSync(path.join(repo, 'manifest.json'), JSON.stringify({ manifest_version: 3, version: '1.0.0' }));
    fs.writeFileSync(path.join(repo, 'src/lib/worker.js'), '// worker\n');
    if (id !== 'video') fs.writeFileSync(path.join(repo, 'src/lib/gateway-account.js'), '// account\n');
    git(repo, ['add', '.']);
    git(repo, ['commit', '-q', '-m', 'release']);
    catalog.extensions.push({ id, name: id, version: '1.0.0', source_commit: git(repo, ['rev-parse', 'HEAD']).trim(), ...(id === 'audio' ? { required: false } : {}), accepted: false, reason: 'candidate' });
  }
  const catalogPath = path.join(root, 'extensions.json');
  fs.writeFileSync(catalogPath, JSON.stringify(catalog, null, 2));
  refreshCatalogLock({ catalogPath, root });
  return { base, root, catalogPath };
}

test('versions compare as Chrome manifest numbers', () => {
  assert.equal(compareVersions('1.3.18', '1.3.5'), 1);
  assert.equal(compareVersions('0.50.16', '0.50.16'), 0);
  assert.equal(compareVersions('0.7.28', '0.7.9'), 1);
  assert.throws(() => compareVersions('1.3.18-beta', '1.3.18'), /not a Chrome manifest version/);
});

test('a bundle at its repo version passes, and video\'s missing account module is only reported', () => {
  const f = fixture();
  const notes = [];
  guardBundleAgainstRepos({ catalogPath: f.catalogPath, root: f.root, log: (line) => notes.push(line) });
  assert.equal(notes.length, 1);
  assert.match(notes[0], /video.*has no src\/lib\/gateway-account\.js/);
});

test('a bundle behind its repo fails the build', () => {
  const f = fixture();
  const repo = repoDir(f, 'image');
  fs.writeFileSync(path.join(repo, 'manifest.json'), JSON.stringify({ manifest_version: 3, version: '1.0.1' }));
  git(repo, ['add', '.']);
  git(repo, ['commit', '-q', '-m', 'bump']);
  assert.throws(() => guardBundleAgainstRepos({ catalogPath: f.catalogPath, root: f.root, log: () => {} }), /image: bundled manifest 1\.0\.0 is behind its repo 1\.0\.1/);
});

test('a bundle ahead of its repo fails the build', () => {
  const f = fixture();
  const catalog = JSON.parse(fs.readFileSync(f.catalogPath, 'utf8'));
  catalog.extensions.find((entry) => entry.id === 'audio').version = '9.9.9';
  fs.writeFileSync(f.catalogPath, JSON.stringify(catalog));
  assert.throws(() => guardBundleAgainstRepos({ catalogPath: f.catalogPath, root: f.root, log: () => {} }), /audio: bundled manifest 9\.9\.9 is not in its repo/);
});

test('the account module cannot fall out of a bundle whose repo carries it', () => {
  const f = fixture();
  const catalog = JSON.parse(fs.readFileSync(f.catalogPath, 'utf8'));
  const image = catalog.extensions.find((entry) => entry.id === 'image');
  image.files = image.files.filter((file) => file.path !== 'src/lib/gateway-account.js');
  fs.writeFileSync(f.catalogPath, JSON.stringify(catalog));
  assert.throws(() => guardBundleAgainstRepos({ catalogPath: f.catalogPath, root: f.root, log: () => {} }), /image: its repo has src\/lib\/gateway-account\.js but the bundled lock omits it/);
});

test('staging reads the pinned commit, never the working tree', () => {
  const f = fixture();
  const repo = repoDir(f, 'video');
  fs.writeFileSync(path.join(repo, 'src/lib/worker.js'), '// uncommitted edit\n');
  const out = path.join(f.base, 'payloads');
  stagePinnedPayloads({ catalogPath: f.catalogPath, root: f.root, outputRoot: out });
  assert.equal(fs.readFileSync(path.join(out, 'video/src/lib/worker.js'), 'utf8'), '// worker\n');
  assert.equal(fs.existsSync(path.join(out, 'video/manifest.json')), true);
  assert.equal(fs.readFileSync(path.join(out, 'image/src/lib/gateway-account.js'), 'utf8'), '// account\n');
});
