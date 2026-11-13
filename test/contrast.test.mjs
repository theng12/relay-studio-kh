import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const css = readFileSync(new URL('../desktop/desktop.css', import.meta.url), 'utf8');

const pair = (name) => {
  const match = css.match(new RegExp(`--${name}: light-dark\\(([^,]+),\\s*([^)]+)\\)`));
  assert.ok(match, `--${name} is a light-dark pair`);
  return { light: match[1].trim(), dark: match[2].trim() };
};
const channel = (value) => {
  const c = value / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};
const luminance = (hex) => {
  const value = hex.replace('#', '');
  const [r, g, b] = [0, 2, 4].map((i) => channel(parseInt(value.slice(i, i + 2), 16)));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const ratio = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

test('body and secondary text keep 4.5:1 in both appearances', () => {
  const text = pair('text');
  const muted = pair('muted');
  const ground = pair('ground');
  const surface = pair('surface');
  for (const mode of ['light', 'dark']) {
    assert.ok(ratio(text[mode], ground[mode]) >= 4.5, `body text on ${mode} ground`);
    assert.ok(ratio(text[mode], surface[mode]) >= 4.5, `body text on ${mode} surface`);
    assert.ok(ratio(muted[mode], ground[mode]) >= 4.5, `secondary text on ${mode} ground`);
    assert.ok(ratio(muted[mode], surface[mode]) >= 4.5, `secondary text on ${mode} surface`);
  }
});

test('the primary button and status text stay readable in both appearances', () => {
  const accent = pair('accent');
  const accentText = pair('accent-contrast');
  const ground = pair('ground');
  for (const mode of ['light', 'dark']) {
    assert.ok(ratio(accentText[mode], accent[mode]) >= 4.5, `primary button text (${mode})`);
    assert.ok(ratio(accent[mode], ground[mode]) >= 4.5, `accent text (${mode})`);
    for (const state of ['ok', 'warn', 'bad']) {
      assert.ok(ratio(pair(state)[mode], ground[mode]) >= 4.5, `${state} on ${mode} ground`);
    }
  }
});

test('the neutral ramp separates ground, surface and border', () => {
  const ground = pair('ground');
  const surface = pair('surface');
  const line = pair('line');
  for (const mode of ['light', 'dark']) {
    assert.ok(ratio(ground[mode], surface[mode]) >= 1.1, `${mode} ground vs surface`);
    assert.ok(ratio(line[mode], surface[mode]) >= 1.3, `${mode} border vs surface`);
  }
});
