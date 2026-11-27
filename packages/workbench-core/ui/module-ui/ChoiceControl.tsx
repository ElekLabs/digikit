import { For } from 'solid-js';
import type { ChoiceSetting } from '@digi/module-ui';
import type { SettingFieldModel } from './setting-field-model';

export default function ChoiceControl(props: { field: ChoiceSetting; model: SettingFieldModel }) {
  return (
    <label>
      <span>{props.field.label}</span>
      <select
        disabled={!props.model.saved() || props.model.busy()}
        onChange={(e) => props.model.edit(e.currentTarget.value)}
      >
        <For each={props.field.choices}>
          {(choice) => (
            <option value={choice} selected={props.model.draft() === choice}>
              {choice}
            </option>
          )}
        </For>
      </select>
    </label>
  );
}
