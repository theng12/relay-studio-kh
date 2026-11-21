// The three extension repos Relay bundles. The build reads immutable git objects at the
// commit pinned in desktop/extensions.json, and refuses to bundle a manifest that is behind
// its repo. Nothing here builds or modifies an extension.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const EXTENSION_REPOS = {
  video: '../chrome-extensions/story-studio-video-generator',
  image: '../chrome-extensions/story-studio-image-generator',
  audio: '../chrome-extensions/story-studio-audio-generator',
};
// Same reviewed scope as scripts/export-extension-drafts.mjs: popups fetch CHANGELOG.md.
const SCOPE = ['manifest.json', 'src', 'CHANGELOG.md'];
export const ACCOUNT_MODULE = 'src/lib/gateway-account.js';

const fail = (message) => { throw new Error(`extension-sources: ${message}`); };
const sha256 = (buffer) => createHash('sha256').update(buffer).digest('hex');
const git = (repo, args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
const gitBytes = (repo, args) => execFileSync('git', ['-C', repo, ...args], { maxBuffer: 64 * 1024 * 1024 });

// Default: a `chrome-extensions` folder beside the app, with each checkout directly inside it.
// RELAY_EXTENSION_REPOS names a group folder instead, holding `<repo>/app` — the layout after the
// extensions moved under `_orchestra/developer-extensions` (2026-11-22). Absolute, never a walk
// from the app's own checkout: the app moved too, so a relative default is right only by accident.
const repoRoot = (root, id, group = process.env.RELAY_EXTENSION_REPOS) => {
  return group
    ? path.join(path.resolve(group), path.basename(EXTENSION_REPOS[id]), 'app')
    : path.resolve(root, EXTENSION_REPOS[id]);
};
const readCatalog = (catalogPath) => JSON.parse(fs.readFileSync(catalogPath, 'utf8'));

function repoHead(repo, id) {
  try { return git(repo, ['rev-parse', 'HEAD']).trim(); }
  catch { fail(`extension ${id}: its repo is missing at ${repo}`); }
}
function repoVersion(repo, id) {
  try { return JSON.parse(git(repo, ['show', 'HEAD:manifest.json'])).version; }
  catch { fail(`extension ${id}: HEAD has no readable manifest.json`); }
}
function hasAt(repo, ref, file) {
  try { execFileSync('git', ['-C', repo, 'cat-file', '-e', `${ref}:${file}`], { stdio: 'ignore' }); return true; } catch { return false; }
}

// Chrome manifest versions are dot-separated whole numbers.
export function compareVersions(left, right) {
  const parse = (value) => {
    const parts = String(value ?? '').split('.');
    if (!parts.length || parts.some((part) => !/^\d+$/.test(part))) fail(`not a Chrome manifest version: ${value}`);
    return parts.map(Number);
  };
  const a = parse(left);
  const b = parse(right);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const diff = (a[index] ?? 0) - (b[index] ?? 0);
    if (diff !== 0) return Math.sign(diff);
  }
  return 0;
}

// Fails the build when the bundled version drifts from its repo, or when the repo carries
// the Gateway account module and the reviewed bundle lock would leave it out.
export function guardBundleAgainstRepos({ catalogPath, root, log = console.info, reposGroup = process.env.RELAY_EXTENSION_REPOS }) {
  const catalog = readCatalog(catalogPath);
  for (const id of Object.keys(EXTENSION_REPOS)) {
    const entry = catalog.extensions?.find((item) => item.id === id);
    if (!entry) fail(`catalog has no ${id} entry`);
    const repo = repoRoot(root, id, reposGroup);
    repoHead(repo, id);
    const version = repoVersion(repo, id);
    const order = compareVersions(entry.version, version);
    if (order < 0) fail(`extension ${id}: bundled manifest ${entry.version} is behind its repo ${version}; refresh desktop/extensions.json`);
    if (order > 0) fail(`extension ${id}: bundled manifest ${entry.version} is not in its repo (repo has ${version})`);
    if (hasAt(repo, 'HEAD', ACCOUNT_MODULE)) {
      if (!entry.files?.some((file) => file.path === ACCOUNT_MODULE)) {
        fail(`extension ${id}: its repo has ${ACCOUNT_MODULE} but the bundled lock omits it`);
      }
    } else {
      log(`extension ${id}: its repo has no ${ACCOUNT_MODULE}; the bundled payload keeps the files the repo does have`);
    }
  }
}

// Copies manifest.json + src/** + CHANGELOG.md from each pinned commit into outputRoot/{id}.
// Bytes come from git objects, so a dirty working tree can never leak into a release.
export function stagePinnedPayloads({ catalogPath, outputRoot, root, reposGroup = process.env.RELAY_EXTENSION_REPOS }) {
  const catalog = readCatalog(catalogPath);
  for (const id of Object.keys(EXTENSION_REPOS)) {
    const entry = catalog.extensions.find((item) => item.id === id);
    if (entry.accepted === false) continue;
    const repo = repoRoot(root, id, reposGroup);
    if (typeof entry.source_commit !== 'string' || !/^[0-9a-f]{40}$/.test(entry.source_commit)) {
      fail(`extension ${id}: accepted requires a pinned 40-hex source_commit`);
    }
    if (!hasAt(repo, entry.source_commit, 'manifest.json')) {
      fail(`extension ${id}: pinned commit ${entry.source_commit} is not in ${repo}; refresh the catalog`);
    }
    const listing = git(repo, ['ls-tree', '-r', entry.source_commit, '--', ...SCOPE]);
    for (const line of listing.split('\n').filter(Boolean)) {
      const [meta, file] = line.split('\t');
      const [mode, type] = meta.split(' ');
      if (type !== 'blob') fail(`extension ${id}: ${file} is not a file (${type})`);
      if (mode === '120000') fail(`extension ${id}: symlink refused: ${file}`);
      if (mode === '160000') fail(`extension ${id}: gitlink refused: ${file}`);
      const bytes = gitBytes(repo, ['cat-file', 'blob', `${entry.source_commit}:${file}`]);
      const dest = path.join(outputRoot, id, file);
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.writeFileSync(dest, bytes);
    }
    const stagedManifest = JSON.parse(fs.readFileSync(path.join(outputRoot, id, 'manifest.json'), 'utf8'));
    if (stagedManifest.version !== entry.version) {
      fail(`extension ${id}: pinned commit holds ${stagedManifest.version}, catalog says ${entry.version}`);
    }
    if (hasAt(repo, entry.source_commit, ACCOUNT_MODULE) && !fs.existsSync(path.join(outputRoot, id, ACCOUNT_MODULE))) {
      fail(`extension ${id}: ${ACCOUNT_MODULE} is missing from the staged payload`);
    }
  }
}

// Rewrites desktop/extensions.json from the repos' HEAD commits. This is the deliberate
// step for a new extension version; the build itself only verifies and stages.
export function refreshCatalogLock({ catalogPath, root, reposGroup = process.env.RELAY_EXTENSION_REPOS }) {
  const catalog = readCatalog(catalogPath);
  for (const id of Object.keys(EXTENSION_REPOS)) {
    const repo = repoRoot(root, id, reposGroup);
    const commit = repoHead(repo, id);
    const manifestBytes = gitBytes(repo, ['show', 'HEAD:manifest.json']);
    const manifest = JSON.parse(manifestBytes.toString('utf8'));
    if (manifest.manifest_version !== 3) fail(`extension ${id}: manifest_version must be 3`);
    const files = [];
    const listing = git(repo, ['ls-tree', '-r', commit, '--', ...SCOPE]);
    for (const line of listing.split('\n').filter(Boolean)) {
      const [meta, file] = line.split('\t');
      const [mode, type] = meta.split(' ');
      if (type !== 'blob' || mode === '120000' || mode === '160000') fail(`extension ${id}: unsafe ${type} at ${file}`);
      files.push({ path: file, sha256: sha256(gitBytes(repo, ['cat-file', 'blob', `${commit}:${file}`])) });
    }
    files.sort((a, b) => (a.path < b.path ? -1 : 1));
    const existing = catalog.extensions.find((item) => item.id === id) ?? { id, name: id };
    const entry = {
      id,
      name: existing.name || id,
      version: manifest.version,
      source_commit: commit,
      ...(existing.required === false ? { required: false } : {}),
      accepted: true,
      manifest_sha256: sha256(manifestBytes),
      files,
    };
    const index = catalog.extensions.findIndex((item) => item.id === id);
    if (index >= 0) catalog.extensions[index] = entry;
    else catalog.extensions.push(entry);
  }
  fs.writeFileSync(catalogPath, `${JSON.stringify(catalog, null, 2)}\n`);
  return catalog;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const catalogPath = path.join(root, 'desktop/extensions.json');
  if (args.includes('--refresh-lock')) refreshCatalogLock({ catalogPath, root });
  else guardBundleAgainstRepos({ catalogPath, root });
}
