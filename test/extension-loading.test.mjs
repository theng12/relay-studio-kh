import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const load = (path) => readFile(new URL(path, import.meta.url), 'utf8');

test('Relay loads its extensions into every Chrome it opens, without --load-extension', async () => {
  const manager = await load('../src-tauri/src/browser_manager.rs');
  const devtools = await load('../src-tauri/src/devtools.rs');
  const extensions = await load('../src-tauri/src/extensions.rs');
  const setup = await load('../desktop/setup.mjs');
  // Chrome 137+ ignores --load-extension; the supported route is Extensions.loadUnpacked over
  // --remote-debugging-pipe, held open for the browser's lifetime.
  assert.doesNotMatch(manager, /--load-extension/, 'the removed Chrome flag is not used');
  assert.match(devtools, /Extensions\.loadUnpacked/, 'extensions are loaded through Chrome\'s own debug protocol');
  assert.match(devtools, /remote-debugging-pipe/, 'the control pipe is how Chrome accepts the command');
  assert.match(manager, /attached\.load_unpacked/, 'a launch passes the prepared extension folders to Chrome');
  assert.match(manager, /loadable_extensions\(&shared\.app\)/, 'only available, enabled extensions are loaded');
  assert.match(extensions, /fn loadable_extensions/, 'the extension loader prepares folders on demand');
  assert.match(extensions, /fn set_extension_enabled/, 'the next launch follows the Extensions page choice');
  assert.match(setup, /Load in Relay’s browsers/, 'each extension has a visible load switch');
  assert.match(setup, /set_extension_enabled/, 'the switch is saved natively');
  assert.match(setup, /Show folder/, 'the prepared folder is reachable without typing a path');
  assert.match(setup, /manager_loaded_extensions/, 'the page can ask Chrome what it actually loaded');
});

test('Relay seeds bound workers into the extensions it opens, never inventing configuration', async () => {
  const manager = await load('../src-tauri/src/browser_manager.rs');
  const seeding = await load('../src-tauri/src/seeding.rs');
  const app = await load('../desktop/app.mjs');
  const html = await load('../desktop/index.html');
  const accounts = await load('../desktop/accounts.mjs');
  assert.match(manager, /seeding::seed_extension/, 'each launched browser seeds its extensions');
  assert.match(seeding, /KEYCHAIN_SERVICE/, 'worker credentials live in the Keychain');
  assert.match(seeding, /save_extension_worker/, 'the Gateway page saves a worker credential');
  assert.match(seeding, /imageGeneratorWorkMode/, 'image activation uses the panel-selected mode key');
  assert.match(seeding, /gatewayWorkerToken/, 'video/audio tokens go to their own storage key');
  assert.match(seeding, /not seeded — no worker id is bound/, 'a kind without a binding is named, not half-seeded');
  assert.match(seeding, /not seeded — no worker token saved/, 'a kind without a token is named');
  assert.match(app, /loadWorkerCredentials/, 'the Gateway page renders the credential rows');
  assert.match(html, /id="extension-workers"/, 'the card exists');
  assert.match(accounts, /Extension seeding/, 'each account row reports its seeding result');
});
