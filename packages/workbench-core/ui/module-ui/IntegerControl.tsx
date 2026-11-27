import { Show, createEffect, on } from 'solid-js';
import type { IntegerSetting } from '@digi/module-ui';
import type { SettingFieldModel } from './setting-field-model';

/** An integer setting's display bound: the stored value over 10^decimals, rounded to its decimals. */
export const shownInteger = (field: IntegerSetting, stored: number) =>
  Number((stored / 10 ** field.decimals).toFixed(field.decimals));

export default function IntegerControl(props: { field: IntegerSetting; model: SettingFieldModel }) {
  let input: HTMLInputElement | undefined;
  // The input owns what is typed until a read, save or reset replaces the saved value.
  createEffect(
    on(props.model.saved, (saved) => {
      if (input && saved) input.value = String(saved.value);
    }),
  );
  return (
    <label>
      <span>{props.field.label}</span>
      <small>
        {shownInteger(props.field, props.field.min)} to {shownInteger(props.field, props.field.max)}
        {props.field.unit ? ` ${props.field.unit}` : ''}
      </small>
      <span class="module-unit">
        <input
          ref={input}
          type="number"
          min={shownInteger(props.field, props.field.min)}
          max={shownInteger(props.field, props.field.max)}
          step={shownInteger(props.field, props.field.step)}
          disabled={!props.model.saved() || props.model.busy()}
          onInput={(e) => props.model.edit(e.currentTarget.valueAsNumber)}
        />
        <Show when={props.field.unit}>
          <span aria-hidden="true">{props.field.unit}</span>
        </Show>
      </span>
    </label>
  );
}
