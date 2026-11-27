import { describe, expect, it } from 'vitest';
import { definePage, isModulePage } from '../src/index.ts';

const Page = () => null;

describe('module page definitions', () => {
  it('keeps what definePage was given and recognises it', () => {
    const page = definePage({ id: 'test/typed', api: 1, Page });
    expect(page).toEqual({ id: 'test/typed', api: 1, Page });
    expect(isModulePage(page)).toBe(true);
  });
  it('refuses another api version, a missing id or Page, and a value that is no page', () => {
    expect(isModulePage({ id: 'test/typed', api: 2, Page })).toBe(false);
    expect(isModulePage({ api: 1, Page })).toBe(false);
    expect(isModulePage({ id: 'test/typed', api: 1 })).toBe(false);
    expect(isModulePage(undefined)).toBe(false);
    expect(isModulePage('page')).toBe(false);
  });
});
