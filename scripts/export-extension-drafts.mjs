// Draft export recipe for Relay candidate extension payloads (v2: adds CHANGELOG.md).
// Scope per extension, from an immutable pinned commit: manifest.json + full src/** + CHANGELOG.md.
// Reads only git objects (never working-tree files); the popups fetch root CHANGELOG.md for
// What's New, so omitting it (v1 recipe) broke that fetch. Output is always accepted=false;
// nothing here is release acceptance. Refuses to overwrite or mutate an existing output root.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Pinned sources: read-only worktrees on ATLAS, exact commits from the verified v1 locks.
// shortcut: paths are this host's known worktrees; if they move, git object access fails loudly.
const SOURCES = [
  { id: 'video', version: '1.3.2',
    commit: 'eeb06f8a8d2cf76c2104892f254af79976c31b93',
    source: '/Volumes/KH-STUDIO-ATLAS/KH-Worktrees/story-studio-video-generator/relay-integration-20261110' },
  { id: 'image', version: '0.50.11',
    commit: 'ba28396931dab5d48c546cfb0f0801678b72fbcf',
    source: '/Volumes/KH-STUDIO-ATLAS/KH-Worktrees/story-studio-image-generator/relay-gateway-20261109' },
  { id: 'audio', version: '0.7.27',
    commit: 'eddeb1b83b0ee22b54f8f698513d62b2fe4f91ef',
    source: '/Volumes/KH-STUDIO-ATLAS/KH-Worktrees/story-studio-audio-generator/relay-gateway-20261109' },
];
const SCOPE = ['manifest.json', 'src', 'CHANGELOG.md'];

const fail = (message) => { throw new Error(`export-extension-drafts: ${message}`); };
const sha256 = (buffer) => createHash('sha256').update(buffer).digest('hex');

// Same digest as scripts/extension-resources.mjs payloadSha256(): sorted "path hash" lines, no trailing newline.
const payloadSha256 = (files) => sha256(Buffer.from(files.map((f) => `${f.path} ${f.sha256}`).sort().join('\n'), 'utf8'));
const DIGEST_RECIPE = "sha256(utf8(sorted '<path> <sha256>' lines joined by '\\n', no trailing newline)); matches scripts/extension-resources.mjs payloadSha256()";

// Path safety copied from scripts/extension-resources.mjs isNormalRelativePath().
function isNormalRelativePath(p) {
  if (typeof p !== 'string' || p.length === 0 || p.includes('\\') || path.isAbsolute(p)) return false;
  if (path.posix.normalize(p) !== p || /[\x00-\x1f\x7f:]/.test(p)) return false;
  return !p.split('/').some((s) => s === '' || s === '.' || s === '..' || s === '.git' || s.startsWith('.env'));
}

const git = (source, args) => execFileSync('git', ['-C', source, ...args], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
const gitBytes = (source, args) => execFileSync('git', ['-C', source, ...args], { maxBuffer: 64 * 1024 * 1024 });

function readExtension(entry) {
  let listing;
  try {
    listing = git(entry.source, ['ls-tree', '-r', entry.commit, '--', ...SCOPE]);
  } catch (err) {
    fail(`cannot read git objects for ${entry.id} from ${entry.source}: ${err.message.split('\n')[0]}`);
  }
  const files = [];
  for (const line of listing.split('\n').filter(Boolean)) {
    const [meta, p] = line.split('\t');
    const [mode, type] = meta.split(' ');
    if (type !== 'blob') fail(`${entry.id}: ${p} is not a blob (type ${type})`);
    if (mode === '120000') fail(`${entry.id}: symlink refused: ${p}`);
    if (mode === '160000') fail(`${entry.id}: gitlink refused: ${p}`);
    if (!isNormalRelativePath(p)) fail(`${entry.id}: unsafe path: ${p}`);
    const bytes = gitBytes(entry.source, ['cat-file', 'blob', `${entry.commit}:${p}`]);
    files.push({ path: p, sha256: sha256(bytes), bytes });
  }
  const names = files.map((f) => f.path);
  for (const required of ['manifest.json', 'CHANGELOG.md']) {
    if (!names.includes(required)) fail(`${entry.id}: locked scope missing ${required}`);
  }
  if (!names.some((p) => p.startsWith('src/'))) fail(`${entry.id}: locked scope has no src/** files`);
  const manifest = JSON.parse(files.find((f) => f.path === 'manifest.json').bytes.toString('utf8'));
  if (manifest.version !== entry.version) fail(`${entry.id}: manifest version ${manifest.version} != pinned ${entry.version}`);
  return files.sort((a, b) => (a.path < b.path ? -1 : 1));
}

const [outRoot, ...flags] = process.argv.slice(2);
if (!outRoot) fail('usage: export-extension-drafts.mjs <output-dir> [--supersedes <old-receipt.json>]');
if (fs.existsSync(outRoot)) fail(`refusing to overwrite existing output: ${outRoot}`);
const supIdx = flags.indexOf('--supersedes');
const supersedesPath = supIdx >= 0 ? path.resolve(flags[supIdx + 1]) : null;
let supersedes = null;
if (supersedesPath) {
  if (!fs.statSync(supersedesPath).isFile()) fail(`supersedes must be a regular file: ${supersedesPath}`);
  supersedes = { path: supersedesPath, sha256: sha256(fs.readFileSync(supersedesPath)) };
}
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const relayCheckpoint = git(repoRoot, ['rev-parse', 'HEAD']).trim();

// Read every git object first so a mid-run failure leaves no half-written output.
const planned = SOURCES.map((entry) => ({ entry, files: readExtension(entry) }));
const payloadsRoot = path.join(outRoot, 'payloads');
fs.mkdirSync(payloadsRoot, { recursive: true });
const extensions = [];
for (const { entry, files } of planned) {
  for (const f of files) {
    const dest = path.join(payloadsRoot, entry.id, f.path);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.writeFileSync(dest, f.bytes);
  }
  const lock = files.map((f) => ({ path: f.path, sha256: f.sha256 }));
  const manifest_sha256 = files.find((f) => f.path === 'manifest.json').sha256;
  extensions.push({
    id: entry.id, version: entry.version, source_commit: entry.commit,
    accepted: false, reason: 'Candidate export only; real browser/Gateway and release-input review remain pending.',
    files: lock, manifest_sha256, payload_sha256: payloadSha256(lock),
  });
}

// Self-check: read the written bytes back and reproduce every hash and digest from disk.
for (const ext of extensions) {
  const dir = path.join(payloadsRoot, ext.id);
  const fromDisk = ext.files.map((f) => {
    const sha = sha256(fs.readFileSync(path.join(dir, f.path)));
    if (sha !== f.sha256) fail(`${ext.id}: readback hash mismatch for ${f.path}`);
    return { path: f.path, sha256: sha };
  });
  if (payloadSha256(fromDisk) !== ext.payload_sha256) fail(`${ext.id}: readback payload digest mismatch`);
}

const receipt = {
  schema: 1,
  status: 'candidate-exports-v2-unaccepted',
  recipe: 'Read immutable git blobs for manifest.json, src/** and CHANGELOG.md from the recorded commit; no build/transformation. v2 adds root CHANGELOG.md (all three popups fetch it for What\'s New); v1 recipe omitted it. Review runtime closure before release acceptance.',
  digest_recipe: DIGEST_RECIPE,
  relay_checkpoint: relayCheckpoint,
  supersedes,
  exports_root: path.resolve(payloadsRoot),
  accepted_count: 0,
  source_modified: false,
  native_build_run: false,
  runtime_or_browser_run: false,
  extensions,
};
fs.writeFileSync(path.join(outRoot, 'extension-draft-locks.json'), JSON.stringify(receipt, null, 2) + '\n');
for (const ext of extensions) {
  console.log(`${ext.id}: ${ext.files.length} files manifest=${ext.manifest_sha256} payload=${ext.payload_sha256}`);
}
console.log(`receipt: ${path.join(outRoot, 'extension-draft-locks.json')}`);
