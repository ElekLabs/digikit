// The native-feel rules of the desktop app (apps/desktop/ui/src/native-core.ts, native.css).
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { contextMenuAllowed, dragAllowed } from '../src/native-core.ts';

/** A stand-in for an element: `closest(selector)` finds `self` when the selector list names one of its `matches`. */
const el = (...matches: string[]) => ({
  closest: (selector: string) => (selector.split(',').some((s) => matches.includes(s.trim())) ? {} : null),
});

test('the context menu stays in text fields and selectable text only', () => {
  for (const s of ['input', 'textarea', '.selectable', 'pre', '.err', '.log', '.path'])
    assert.ok(contextMenuAllowed(el(s)), s);
  assert.equal(contextMenuAllowed(el('button')), false);
  assert.equal(contextMenuAllowed(el()), false);
  assert.equal(contextMenuAllowed(null), false);
  assert.equal(contextMenuAllowed({}), false, 'not an element');
});

test('links and images are not dragged, anything else may start a drag', () => {
  assert.equal(dragAllowed(el('a')), false);
  assert.equal(dragAllowed(el('img')), false);
  assert.equal(dragAllowed(el('button')), true);
});
