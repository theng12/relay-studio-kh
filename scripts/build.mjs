import { access, readFile, mkdir, copyFile, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const shared = resolve(process.env.KH_SHARED_UI || resolve(root, '../kh-shared-ui'));
const python = process.env.RELAY_PYTHON || (process.platform === 'win32' ? 'python' : 'python3');
try {
  await Promise.all(['whats-new.py', 'design-tokens.json'].map((name) => access(resolve(shared, name))));
} catch (cause) {
  throw new Error('Shared KH UI tools are missing. Place kh-shared-ui next to Relay or set KH_SHARED_UI to its folder.', { cause });
}
const version = (await readFile(resolve(root, 'VERSION'), 'utf8')).trim();
if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('VERSION must be MAJOR.MINOR.PATCH.');
const releases = JSON.parse(execFileSync(python, ['-I', '-X', 'utf8', resolve(shared, 'whats-new.py'), resolve(root, 'CHANGELOG.md')], { encoding: 'utf8' }));
if (releases[0]?.version !== version) throw new Error('VERSION and the newest CHANGELOG entry must match.');
const tokens = JSON.parse(await readFile(resolve(shared, 'design-tokens.json'), 'utf8'));
const declarations = [];
for (const [name, value] of Object.entries(tokens.space)) declarations.push(`--space-${name}:${value}px`);
for (const [name, value] of Object.entries(tokens.radius)) declarations.push(`--radius-${name}:${value}px`);
for (const [name, value] of Object.entries(tokens.type)) declarations.push(`--type-${name}:${value.size}px`);
const dist = resolve(root, 'dist');
await mkdir(dist, { recursive: true });
const info = { version, releases };
const html = await readFile(resolve(root, 'web/index.html'), 'utf8');
const css = await readFile(resolve(root, 'web/app.css'), 'utf8');
const api = (await readFile(resolve(root, 'web/api.mjs'), 'utf8')).replace(/^export /gm, '');
const app = (await readFile(resolve(root, 'web/app.mjs'), 'utf8')).replace(/^import .*\n/, '').replace(/^export /gm, '');
const scopedCSS = `.relay-connector { ${declarations.join(';')};\n${css.replace(/:root/g, '&').replace(/\bbody\s*\{/g, '& {')}\n}`;
const markup = html.match(/<body>([\s\S]*)<\/body>/)[1].trim();
const fragment = `<style>\n${scopedCSS}\n</style>\n${markup}\n`;
const bundle = `(() => {\n${api}\n${app}\nwindow.mountRelayConnector = (root, options = {}) => mountRelayConnector(root, { ...options, appInfo: ${JSON.stringify(info)} });\n})();\n`;
await writeFile(resolve(dist, 'relay-pane.html'), fragment);
await writeFile(resolve(dist, 'relay-pane.js'), bundle);
await copyFile(resolve(root, 'web/bootstrap.mjs'), resolve(dist, 'bootstrap.mjs'));
await writeFile(resolve(dist, 'index.html'), html.replace(/  <link rel="stylesheet"[^\n]+\n/g, '').replace('<script type="module"', '<script src="./relay-pane.js"></script>\n  <script type="module"').replace(/<body>[\s\S]*<\/body>/, `<body>${fragment}</body>`));
console.info(`Built Relay Studio KH ${version}; Gateway Providers pane at ${dist}`);
