// The image channel's per-image price (owner decision 2026-11-21): the helper
// normalizes to free (0) and the Settings page is wired to it.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { DEFAULT_IMAGE_CREDITS_PER_IMAGE, imageCreditsPerImage } from '../desktop/accounts.mjs';

test('the image price starts free and every invalid value normalizes to free', () => {
  assert.equal(DEFAULT_IMAGE_CREDITS_PER_IMAGE, 0);
  assert.equal(imageCreditsPerImage(undefined), 0);
  assert.equal(imageCreditsPerImage(''), 0);
  assert.equal(imageCreditsPerImage('not a number'), 0);
  assert.equal(imageCreditsPerImage(-3), 0);
  assert.equal(imageCreditsPerImage(-0.5), 0);
  assert.equal(imageCreditsPerImage(Number.POSITIVE_INFINITY), 0);
  assert.equal(imageCreditsPerImage(Number.NaN), 0);
});

test('a real price is kept as given', () => {
  assert.equal(imageCreditsPerImage('2.5'), 2.5);
  assert.equal(imageCreditsPerImage(7), 7);
  assert.equal(imageCreditsPerImage(0), 0);
});

test('the Settings page carries the image price field and wires it to the helper', async () => {
  const root = new URL('../', import.meta.url);
  const [html, app] = await Promise.all([
    readFile(new URL('desktop/index.html', root), 'utf8'),
    readFile(new URL('desktop/app.mjs', root), 'utf8'),
  ]);
  const settingsSection = html.slice(html.indexOf('id="page-settings"'), html.indexOf('id="page-whats-new"'));
  assert.ok(settingsSection.includes('id="image-price"'), 'the image price input lives in the Settings page');
  assert.ok(app.includes('imageCreditsPerImage'), 'the desktop app uses the normalization helper');
});
