import { createSignal } from 'solid-js';
import type { AnySettingValue, DynamicSettings, Setting } from '@digi/module-ui';

/** The value a setting shows while nothing valid is stored: its default (text and file names show empty). */
export const defaultValue = (setting: Setting): Exclude<AnySettingValue, null> =>
  setting.kind === 'text' || setting.kind === 'file-name' ? '' : setting.default;

export interface FieldSaved {
  /** The stored display value; null for text that is unset. A refused stored value reads as unset. */
  value: AnySettingValue;
  unset: boolean;
}

/** The state of one generic form field: the saved value, the draft the control edits and the requests that move them. A control only renders it. */
export function createSettingFieldModel(setting: Setting, settings: DynamicSettings) {
  const [saved, setSaved] = createSignal<FieldSaved>();
  const [draft, setDraft] = createSignal<AnySettingValue>(defaultValue(setting));
  const [edited, setEdited] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  const [refused, setRefused] = createSignal(false);

  async function update(action: 'read' | 'save' | 'reset') {
    setBusy(true);
    setError('');
    try {
      const id = setting.id;
      const next =
        action === 'read'
          ? await settings.get(id)
          : action === 'save'
            ? await settings.put(id, draft() as Exclude<AnySettingValue, null>)
            : await settings.reset(id);
      if (!next.ok && next.error.code === 'invalid-stored' && action === 'read') {
        setRefused(true);
        setSaved({ value: defaultValue(setting), unset: true });
        setDraft(defaultValue(setting));
        setEdited(false);
        return;
      }
      if (!next.ok) {
        setError(next.error.message);
        return;
      }
      setRefused(false);
      setSaved({ value: next.value as AnySettingValue, unset: next.unset });
      setDraft((next.value as AnySettingValue) ?? defaultValue(setting));
      setEdited(false);
    } finally {
      setBusy(false);
    }
  }

  return {
    saved,
    draft,
    busy,
    error,
    /** True while the device refused the stored value on the last read: the form shows the default. */
    storedRefused: refused,
    /** The control edits the draft through this. */
    edit(value: AnySettingValue) {
      setEdited(true);
      setDraft(value);
    },
    canSave: () => !busy() && !!saved() && edited() && (saved()!.unset || draft() !== saved()!.value),
    canReset: () => !busy() && !!saved() && !saved()!.unset,
    read: () => update('read'),
    save: () => update('save'),
    reset: () => update('reset'),
  };
}

export type SettingFieldModel = ReturnType<typeof createSettingFieldModel>;
