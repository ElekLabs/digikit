import { Match, Show, Switch, onMount } from 'solid-js';
import type { DynamicSettings, Setting } from '@digi/module-ui';
import BooleanControl from './BooleanControl';
import ChoiceControl from './ChoiceControl';
import IntegerControl from './IntegerControl';
import TextControl from './TextControl';
import { createSettingFieldModel } from './setting-field-model';

/** One row of the generic form: the control of the setting's kind, Save and Reset, and what went wrong. File names have their own control. */
export default function SettingField(props: {
  field: Exclude<Setting, { kind: 'file-name' }>;
  settings: DynamicSettings;
}) {
  // oxlint-disable-next-line solid/reactivity -- a row belongs to one setting
  const model = createSettingFieldModel(props.field, props.settings);
  onMount(() => void model.read());
  const as = <K extends Setting['kind']>(kind: K) =>
    props.field.kind === kind ? (props.field as Extract<Setting, { kind: K }>) : undefined;
  return (
    <div class="module-field">
      <Switch>
        <Match when={as('text')}>{(field) => <TextControl field={field()} model={model} />}</Match>
        <Match when={as('integer')}>{(field) => <IntegerControl field={field()} model={model} />}</Match>
        <Match when={as('boolean')}>{(field) => <BooleanControl field={field()} model={model} />}</Match>
        <Match when={as('choice')}>{(field) => <ChoiceControl field={field()} model={model} />}</Match>
      </Switch>
      <button class="btn primary" disabled={!model.canSave()} onClick={() => void model.save()}>
        Save
      </button>
      <button class="btn" disabled={!model.canReset()} onClick={() => void model.reset()}>
        Reset
      </button>
      <Show when={model.storedRefused()}>
        <small role="status">Stored value is invalid; showing the default.</small>
      </Show>
      <Show when={model.error()}>
        <span role="alert">{model.error()}</span>
        <button class="btn" disabled={model.busy()} onClick={() => void model.read()}>
          Read again
        </button>
      </Show>
    </div>
  );
}
