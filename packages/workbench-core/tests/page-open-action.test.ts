import { describe, expect, it } from 'vitest';
import { pageOpenActions } from '../src/module-ui/page-open-action.ts';

const row = { id: 'test/a', name: 'a', known: true, loaded: true };

describe('the Open action of a Transfer row', () => {
  it('navigates to the page of a loaded module that has one', () => {
    const opened: string[] = [];
    const [open] = pageOpenActions(row, { has: (id) => id === 'test/a', open: (id) => opened.push(id) });
    expect(open?.label).toBe('Open');
    open!.onClick();
    expect(opened).toEqual(['test/a']);
  });
  it('is absent for a module without a page, one that is not loaded and a row of no known module', () => {
    const pages = { has: () => true, open: () => {} };
    expect(pageOpenActions(row, { has: () => false, open: () => {} })).toEqual([]);
    expect(pageOpenActions({ ...row, loaded: false }, pages)).toEqual([]);
    expect(pageOpenActions({ ...row, id: null, known: false }, pages)).toEqual([]);
    expect(pageOpenActions(row, undefined)).toEqual([]);
  });
  it('names a card-only row by its header name when the library does not know it', () => {
    const ids: string[] = [];
    const [open] = pageOpenActions(
      { id: null, name: 'test/card', known: true, loaded: true },
      {
        has: () => true,
        open: (id) => ids.push(id),
      },
    );
    open!.onClick();
    expect(ids).toEqual(['test/card']);
  });
});
