import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { processTier } from '../desktop/accounts.mjs';

const source = () => readFile(new URL('../desktop/browsers.mjs', import.meta.url), 'utf8');

test('Open windows renders the process state through the shared processTier mapping', async () => {
  const browsers = await source();
  assert.match(browsers, /import \{[^}]*\bprocessTier\b[^}]*\} from '\.\/accounts\.mjs'/, 'the shared mapping is imported, not duplicated');
  assert.match(browsers, /const tier = processTier\(profile\)/, 'each card asks the shared mapping for its state');
  assert.match(browsers, /refs\.state\.className = `state \$\{tier\.tone\}`/, 'the tone is the shared mapping tone');
  assert.match(browsers, /refs\.state\.textContent = tier\.label/, 'the label is the shared mapping label');
  assert.doesNotMatch(browsers, /\btone:\s*['"](?:ok|warn|bad|info|off)['"]/, 'no parallel tone table lives in browsers.mjs');
});

test('the shared mapping keeps every process_state label and tone the Open windows cards show', () => {
  const cases = [
    ['closed', 'Closed', 'off'],
    ['opening', 'Opening', 'info'],
    ['open', 'Open', 'ok'],
    ['queued', 'Waiting for memory', 'warn'],
    ['closing', 'Closing', 'warn'],
    ['error', 'Error', 'bad'],
  ];
  for (const [state, label, tone] of cases) {
    assert.deepEqual(processTier({ process_state: state, error: null }), {
      tone, label, detail: state === 'error' ? 'Process error' : null,
    });
  }
  assert.equal(processTier({ process_state: 'open', close_pending: true }).label, 'Waiting to close');
});
