import type { Setting, SettingsSchema } from '@digi/module-ui';
import type { PageDescriptor } from './module-page.ts';

export type ModulePageTab = 'page' | 'settings';
export type FormSetting = Exclude<Setting, { kind: 'file-name' }>;

/** The settings the generic form shows. A file-name setting has its own control in the module's file panel. */
export const formFields = (schema: SettingsSchema): FormSetting[] =>
  schema.settings.filter((setting): setting is FormSetting => setting.kind !== 'file-name');

/** The tabs of a module's view: the page it ships first, the generic form beside it when it also has public settings, the form alone without a page. */
export function modulePageTabs(descriptor: Pick<PageDescriptor, 'schema' | 'hasPage'>): ModulePageTab[] {
  const form = formFields(descriptor.schema).length > 0;
  return descriptor.hasPage ? (form ? ['page', 'settings'] : ['page']) : form ? ['settings'] : [];
}

/** True when the module has something to show: a page, public settings or a file panel. Without it the module has no entry. */
export const hasModuleEntry = (descriptor: PageDescriptor): boolean =>
  descriptor.hasFiles || modulePageTabs(descriptor).length > 0;
