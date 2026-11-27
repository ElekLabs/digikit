import type { BooleanSetting } from '@digi/module-ui';
import type { SettingFieldModel } from './setting-field-model';

export default function BooleanControl(props: { field: BooleanSetting; model: SettingFieldModel }) {
  return (
    <label class="module-toggle">
      <span>{props.field.label}</span>
      <input
        type="checkbox"
        role="switch"
        checked={props.model.draft() === true}
        disabled={!props.model.saved() || props.model.busy()}
        onChange={(e) => props.model.edit(e.currentTarget.checked)}
      />
    </label>
  );
}
