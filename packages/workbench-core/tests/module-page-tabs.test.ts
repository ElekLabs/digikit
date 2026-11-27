import { describe, expect, it } from 'vitest';
import type { SettingsSchema } from '@digi/module-ui';
import { hasModuleEntry, modulePageTabs } from '../src/module-ui/module-page-tabs.ts';

const text = { id: 'n', label: 'N', kind: 'text', apply: 'immediate', maxBytes: 8 } as const;
const file = { id: 'f', label: 'F', kind: 'file-name', apply: 'immediate', extension: 'dkbs' } as never;
const withSettings = (...settings: SettingsSchema['settings']): SettingsSchema => ({ version: 1, settings });
const view = (hasPage: boolean, schema: SettingsSchema, hasFiles = false) => ({ schema, hasPage, hasFiles });

describe('the tabs of a module view', () => {
  it('shows the page and, beside it, the generic form when the module has public settings', () => {
    expect(modulePageTabs(view(true, withSettings(text)))).toEqual(['page', 'settings']);
  });
  it('shows the page alone without public settings, and ignores a file-name setting', () => {
    expect(modulePageTabs(view(true, withSettings()))).toEqual(['page']);
    expect(modulePageTabs(view(true, withSettings(file)))).toEqual(['page']);
  });
  it('lists a module with zero settings and a page as one entry with the page tab and no Settings tab', () => {
    const zero = view(true, withSettings());
    expect(modulePageTabs(zero)).toEqual(['page']);
    expect(hasModuleEntry({ id: 'x', name: 'X', ...zero })).toBe(true);
  });
  it('shows the form alone without a page', () => {
    expect(modulePageTabs(view(false, withSettings(text)))).toEqual(['settings']);
  });
  it('gives a module with neither page, settings nor file panel no entry', () => {
    expect(hasModuleEntry({ id: 'x', name: 'X', ...view(false, withSettings()) })).toBe(false);
    expect(hasModuleEntry({ id: 'x', name: 'X', ...view(false, withSettings(), true) })).toBe(true);
  });
});
