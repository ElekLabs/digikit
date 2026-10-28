// Module icons (modules/<category>/<name>/gallery/icon.svg) are inlined into the page (ModuleCard.tsx),
// so each must be plain line art: a 48 grid, the theme's colours, and nothing that runs or loads.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../..', import.meta.url));
const modules = path.join(root, 'modules');
const icons = fs
  .readdirSync(modules)
  .flatMap((category) =>
    fs.statSync(path.join(modules, category)).isDirectory()
      ? fs.readdirSync(path.join(modules, category)).map((name) => path.join(modules, category, name))
      : [],
  )
  .map((dir) => path.join(dir, 'gallery', 'icon.svg'))
  .filter((file) => fs.existsSync(file));

const ELEMENTS = new Set(['svg', 'g', 'path', 'circle', 'rect', 'line', 'polyline', 'polygon', 'ellipse']);
const ATTRIBUTES = new Set([
  'viewBox',
  'fill',
  'stroke',
  'stroke-width',
  'stroke-linecap',
  'stroke-linejoin',
  'stroke-dasharray',
  'opacity',
  'aria-hidden',
  'style',
  'd',
  'x',
  'y',
  'width',
  'height',
  'rx',
  'ry',
  'cx',
  'cy',
  'r',
  'x1',
  'y1',
  'x2',
  'y2',
  'points',
]);
// Colours come from the page: currentColor for the line, --icon-accent for the one highlight.
const COLOUR = /^(none|currentColor)$/;
const STYLE = /^(\s*(stroke|fill)\s*:\s*var\(--icon-accent\)\s*;?)+\s*$/;

test('every public module has an icon', () => {
  for (const id of ['fx/freqshift', 'machines/pluck']) {
    assert.ok(fs.existsSync(path.join(modules, id, 'gallery', 'icon.svg')), `${id} has no gallery/icon.svg`);
  }
});

for (const file of icons) {
  const name = path.relative(root, file);
  test(`${name} is plain line art`, () => {
    const text = fs.readFileSync(file, 'utf8');
    assert.ok(text.length < 4096, 'icons stay under 4 KB');
    assert.match(text.trim(), /^<svg viewBox="0 0 48 48"[^>]*>[\s\S]*<\/svg>$/, 'one <svg> on a 48 grid');
    assert.doesNotMatch(text, /<!|<\?|&/, 'no comments, doctypes, processing instructions or entities');
    for (const [, tag, attrs] of text.matchAll(/<\/?([a-zA-Z]+)([^>]*)>/g)) {
      assert.ok(ELEMENTS.has(tag), `element <${tag}> is not allowed`);
      for (const [, attr, value] of attrs.matchAll(/([a-zA-Z:-]+)="([^"]*)"/g)) {
        assert.ok(ATTRIBUTES.has(attr), `attribute ${attr} on <${tag}> is not allowed`);
        if (attr === 'fill' || attr === 'stroke') assert.match(value, COLOUR, `${attr}="${value}"`);
        if (attr === 'style') assert.match(value, STYLE, `style="${value}"`);
      }
    }
  });
}
