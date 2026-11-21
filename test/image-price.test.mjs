// The per-channel per-image price contract (owner decision 2026-11-21):
// default 0 credits, per-channel value round-trips, and the account/quote
// payload Relay gives Gateway carries it.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  DEFAULT_IMAGE_CREDITS_PER_IMAGE,
  normalizeImageCreditsPerImage,
  storedImageCreditsPerImage,
  saveImageCreditsPerImage,
  imageQuotePrice,
} from '../web/app.mjs';

const memoryStore = new Map();
globalThis.localStorage = {
  getItem: (key) => (memoryStore.has(key) ? memoryStore.get(key) : null),
  setItem: (key, value) => memoryStore.set(key, String(value)),
};

test('the image channel starts free: the default price is 0 credits', () => {
  memoryStore.clear();
  assert.equal(DEFAULT_IMAGE_CREDITS_PER_IMAGE, 0);
  assert.equal(storedImageCreditsPerImage(), 0);
  assert.equal(normalizeImageCreditsPerImage(undefined), 0);
  assert.equal(normalizeImageCreditsPerImage(''), 0);
  assert.equal(normalizeImageCreditsPerImage('not a number'), 0);
  assert.equal(normalizeImageCreditsPerImage(-3), 0);
  assert.equal(normalizeImageCreditsPerImage(Number.POSITIVE_INFINITY), 0);
});

test('a per-channel value round-trips, and clearing it returns to free', () => {
  memoryStore.clear();
  assert.equal(saveImageCreditsPerImage('2.5'), 2.5);
  assert.equal(storedImageCreditsPerImage(), 2.5);
  assert.equal(saveImageCreditsPerImage(7), 7);
  assert.equal(storedImageCreditsPerImage(), 7);
  assert.equal(saveImageCreditsPerImage(-1), 0);
  assert.equal(storedImageCreditsPerImage(), 0);
});

test('the account/quote payload carries the price with the extension quote fields', () => {
  assert.deepEqual(imageQuotePrice(2.5), { value: 2.5, unit: 'credits', source: 'settings.image-credits' });
  assert.deepEqual(imageQuotePrice(0), { value: 0, unit: 'credits', source: 'settings.image-credits' });
  assert.deepEqual(imageQuotePrice('bad'), { value: 0, unit: 'credits', source: 'settings.image-credits' });
});
