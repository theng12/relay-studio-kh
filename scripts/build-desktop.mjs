// Local web assets only; this does not invoke Rust, a native packager or OS services.
import { readFile, writeFile, mkdir, copyFile, mkdtemp, rename } from 'node:fs/promises';
import { prepareExtensionResources } from './extension-resources.mjs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('..', import.meta.url));
execFileSync(process.execPath, [resolve(root, 'scripts/build.mjs')], { stdio: 'inherit' });
const version = (await readFile(resolve(root, 'VERSION'), 'utf8')).trim();
const native = JSON.parse(await readFile(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'));
if (native.version !== version) throw new Error('Native and Relay versions must match.');
const shared = resolve(process.env.KH_SHARED_UI || resolve(root, '../kh-shared-ui'));
const python = process.env.RELAY_PYTHON || (process.platform === 'win32' ? 'python' : 'python3');
const releases = JSON.parse(execFileSync(python, ['-I', '-X', 'utf8', resolve(shared, 'whats-new.py'), resolve(root, 'CHANGELOG.md')], { encoding: 'utf8' }));
const style = (await readFile(resolve(root, 'dist/relay-pane.html'), 'utf8')).match(/^<style>[\s\S]*?<\/style>/)?.[0];
if (!style) throw new Error('Shared Relay styles are unavailable.');
const staging = await mkdtemp(resolve(root, '.desktop-resources-'));
const resources = resolve(staging, 'prepared');
await prepareExtensionResources({ catalogPath: resolve(root, 'desktop/extensions.json'), outputRoot: resources, payloadRoot: process.env.RELAY_EXTENSION_PAYLOADS, requireComplete: process.argv.includes('--require-complete') });
const resourceIndex = JSON.parse(await readFile(resolve(resources, 'extensions/index.json'), 'utf8'));
try { await rename(resolve(root, 'desktop-resources'), resolve(staging, 'previous')); } catch (error) { if (error.code !== 'ENOENT') throw error; }
await rename(resources, resolve(root, 'desktop-resources'));
const output = resolve(root, 'desktop-dist');
await mkdir(output, { recursive: true });
const html = await readFile(resolve(root, 'desktop/index.html'), 'utf8');
await writeFile(resolve(output, 'index.html'), html.replace('<!-- RELAY_STYLE -->', style));
for (const name of ['app.mjs', 'recovery.mjs', 'setup.mjs', 'desktop.css']) await copyFile(resolve(root, 'desktop', name), resolve(output, name));
execFileSync(python, ['-I', '-X', 'utf8', resolve(root, 'scripts/make-icon.py'), resolve(root, 'src-tauri/icons')], { stdio: 'inherit' });
await writeFile(resolve(output, 'app-info.mjs'), `export default ${JSON.stringify({ version, releases })};\n`);
await writeFile(resolve(output, 'extension-info.mjs'), `export default ${JSON.stringify(resourceIndex)};\n`);
console.info(`Prepared desktop web assets ${version}; native build not run.`);
