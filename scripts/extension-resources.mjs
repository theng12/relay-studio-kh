// Reviewed-extension packaging: catalog + reviewed build payload -> staged desktop resources.
// The caller supplies a fresh staging dir; this never overwrites or wipes existing output.
// No fallback to source checkouts: only files that pass the reviewed lock are copied.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const EXTENSION_KINDS = ['video', 'image', 'audio'];

const HEX64 = /^[0-9a-f]{64}$/;
const COMMIT40 = /^[0-9a-f]{40}$/;

const sha256 = (buffer) => createHash('sha256').update(buffer).digest('hex');

function fail(message) {
  throw new Error(`extension-resources: ${message}`);
}

// Lock paths must be plain, already-normalised relative POSIX paths.
// shortcut: rejects backslashes/./.. and .git/.env names; upgrade if a payload ever needs other layouts.
function isNormalRelativePath(p) {
  if (typeof p !== 'string' || p.length === 0 || p.includes('\\') || path.isAbsolute(p)) return false;
  if (path.posix.normalize(p) !== p || /[\x00-\x1f\x7f:]/.test(p)) return false;
  return !p.split('/').some((s) => s === '' || s === '.' || s === '..' || s === '.git' || s.startsWith('.env'));
}

// Deterministic digest of the reviewed lock: sorted "path hash" lines.
function payloadSha256(files) {
  return sha256(Buffer.from(files.map((f) => `${f.path} ${f.sha256}`).sort().join('\n'), 'utf8'));
}

function walkFiles(dir, prefix) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const rel = prefix + entry.name;
    if (entry.isSymbolicLink()) fail(`symlink refused: ${rel}`);
    if (entry.isDirectory()) out.push(...walkFiles(path.join(dir, entry.name), rel + '/'));
    else if (entry.isFile()) out.push(rel);
    else fail(`non-regular file refused: ${rel}`);
  }
  return out;
}

// Any problem here throws: a malformed accepted payload must never downgrade to unavailable.
function stageAccepted(entry, payloadRoot, extensionsOut) {
  const where = `extension ${entry.id}`;
  if (typeof payloadRoot !== 'string' || !payloadRoot) fail(`${where}: set RELAY_EXTENSION_PAYLOADS to the reviewed payload directory`);
  if (!COMMIT40.test(entry.source_commit ?? '')) fail(`${where}: accepted requires a pinned 40-hex source_commit`);
  if (!HEX64.test(entry.manifest_sha256 ?? '')) fail(`${where}: accepted requires manifest_sha256 (64-hex)`);
  if (!Array.isArray(entry.files) || entry.files.length === 0) fail(`${where}: accepted requires a reviewed files lock`);
  const lock = new Map();
  for (const f of entry.files) {
    if (!isNormalRelativePath(f?.path) || !HEX64.test(f?.sha256 ?? '')) fail(`${where}: bad lock entry ${JSON.stringify(f?.path)}`);
    if (lock.has(f.path)) fail(`${where}: duplicate lock path ${f.path}`);
    lock.set(f.path, f.sha256);
  }
  if (!lock.has('manifest.json')) fail(`${where}: lock must include manifest.json`);
  let dirStat;
  const dir = path.join(payloadRoot, entry.id);
  try { dirStat = fs.lstatSync(dir); } catch { fail(`${where}: payload directory missing: ${dir}`); }
  if (!dirStat.isDirectory()) fail(`${where}: payload is not a directory: ${dir}`);
  const present = walkFiles(dir, '');
  const missing = [...lock.keys()].filter((p) => !present.includes(p));
  const unlisted = present.filter((p) => !lock.has(p));
  if (missing.length) fail(`${where}: missing files ${missing.join(', ')}`);
  if (unlisted.length) fail(`${where}: unlisted files ${unlisted.join(', ')}`);
  const outDir = path.join(extensionsOut, entry.id);
  const staged = [];
  for (const p of [...lock.keys()].sort()) {
    const bytes = fs.readFileSync(path.join(dir, p));
    if (sha256(bytes) !== lock.get(p)) fail(`${where}: hash mismatch for ${p}`);
    const dest = path.join(outDir, p);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.writeFileSync(dest, bytes);
    staged.push({ path: p, sha256: lock.get(p) });
  }
  const manifestBytes = fs.readFileSync(path.join(dir, 'manifest.json'));
  if (sha256(manifestBytes) !== entry.manifest_sha256) fail(`${where}: manifest_sha256 mismatch`);
  let manifest;
  try { manifest = JSON.parse(manifestBytes.toString('utf8')); } catch { fail(`${where}: manifest.json is not valid JSON`); }
  if (manifest.manifest_version !== 3) fail(`${where}: manifest_version must be 3`);
  if (manifest.version !== entry.version) fail(`${where}: catalog version ${entry.version} != manifest version ${manifest.version}`);
  return { files: staged, payload_sha256: payloadSha256(staged) };
}

export function prepareExtensionResources({ catalogPath, outputRoot, payloadRoot, requireComplete = false }) {
  if (fs.existsSync(outputRoot)) fail(`refusing to overwrite existing output root: ${outputRoot}`);
  const catalog = JSON.parse(fs.readFileSync(catalogPath, 'utf8'));
  if (catalog.schema !== 1 || !Array.isArray(catalog.extensions)) fail('catalog must be {schema:1, extensions:[...]}');
  const ids = catalog.extensions.map((e) => e?.id);
  if (ids.length !== EXTENSION_KINDS.length || [...ids].sort().join() !== [...EXTENSION_KINDS].sort().join()) {
    fail(`catalog must list exactly ${EXTENSION_KINDS.join('/')}`);
  }
  const extensionsOut = path.join(outputRoot, 'extensions');
  fs.mkdirSync(extensionsOut, { recursive: true });
  const rows = [];
  for (const kind of EXTENSION_KINDS) {
    const entry = catalog.extensions.find((e) => e.id === kind);
    for (const field of ['name']) {
      if (typeof entry[field] !== 'string' || entry[field] === '') fail(`extension ${kind}: ${field} required`);
    }
    if (entry.accepted === false) {
      if (typeof entry.reason !== 'string' || entry.reason === '') fail(`extension ${kind}: unavailable needs an honest reason`);
      rows.push({
        id: kind, name: entry.name, version: entry.version ?? null, source_commit: entry.source_commit ?? null,
        available: false, reason: entry.reason, files: [], payload_sha256: null,
      });
      continue;
    }
    if (entry.accepted !== true) fail(`extension ${kind}: accepted must be true or false`);
    if (typeof entry.version !== 'string' || !/^\d+(\.\d+){0,3}$/.test(entry.version) || entry.version.split('.').some(n => +n > 65535 || (n.length > 1 && n.startsWith('0'))) || entry.version.split('.').every(n => +n === 0)) fail(`extension ${kind}: accepted requires a valid Chrome manifest version`);
    const staged = stageAccepted(entry, payloadRoot, extensionsOut);
    rows.push({ id: kind, name: entry.name, version: entry.version, source_commit: entry.source_commit, available: true, reason: null, ...staged });
  }
  const available = rows.filter((r) => r.available).length;
  if (requireComplete && available < rows.length) fail(`require-complete: ${rows.length - available} of ${rows.length} extensions unavailable`);
  fs.writeFileSync(path.join(extensionsOut, 'index.json'), JSON.stringify({ schema: 1, extensions: rows }, null, 2) + '\n');
  return { rows, available, total: rows.length };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  if (!process.env.RELAY_EXTENSION_PAYLOADS) fail('set RELAY_EXTENSION_PAYLOADS to the reviewed payload directory');
  const result = prepareExtensionResources({
    catalogPath: path.join(repoRoot, 'desktop', 'extensions.json'),
    outputRoot: path.join(repoRoot, 'desktop-resources'),
    payloadRoot: path.resolve(process.env.RELAY_EXTENSION_PAYLOADS),
    requireComplete: process.argv.includes('--require-complete'),
  });
  console.log(`extensions packaged: ${result.available} of ${result.total} available`);
}
