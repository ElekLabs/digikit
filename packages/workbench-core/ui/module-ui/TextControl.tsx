import type { TextSetting } from '@digi/module-ui';
import type { SettingFieldModel } from './setting-field-model';

export default function TextControl(props: { field: TextSetting; model: SettingFieldModel }) {
  return (
    <label>
      <span>{props.field.label}</span>
      <small>Up to {props.field.maxBytes} bytes</small>
      <input
        type="text"
        value={String(props.model.draft() ?? '')}
        placeholder={props.model.saved()?.unset ? 'Unset' : undefined}
        disabled={!props.model.saved() || props.model.busy()}
        onInput={(e) => props.model.edit(e.currentTarget.value)}
      />
    </label>
  );
}
