// File selection, transfer and preview for one module data directory.
import { For, Show, createEffect, createSignal, on, onCleanup, type JSX } from 'solid-js';
import ModuleFilePreview, { previewError } from '../components/ModuleFilePreview';
import { errorLine, formatBytes } from '../../src/modules/backend';
import { ListRow, LocalPane, DevicePane, createConfirm, DeleteButton, writing, type PageCtx } from './TransferParts';
import type { ModuleFilesSpec } from '../../src/transfer/env';
import { createPaneSelect } from '../../src/transfer/pane-select';
import SplitPane from './SplitPane';
import { countLabel } from '../../src/transfer/transfer-core';
import type { ModuleFileClient } from '../../src/module-ui/module-files';

const SENT_NOTE = "Choose it in the device's module settings; it applies on next restart.";

interface Local {
  name: string;
  bytes: Uint8Array;
  error: string | undefined;
}

export default function ModuleFilesArea(props: {
  ctx: PageCtx;
  id: string;
  spec: ModuleFilesSpec;
  files: ModuleFileClient;
  refresh: () => unknown;
  prepareSend?: () => Promise<void>;
}): JSX.Element {
  const { busy, run, setError } = props.ctx;
  let input: HTMLInputElement | undefined;
  const [local, setLocal] = createSignal<Local[]>([]);
  const [device, setDevice] = createSignal<{ name: string; size: number }[]>([]);
  const [lsel, setLsel] = createSignal<Set<string>>(new Set());
  const [dsel, setDsel] = createSignal<Set<string>>(new Set());
  const [shown, setShown] = createSignal<{ name: string; bytes: Uint8Array }>();
  const [note, setNote] = createSignal('');
  const confirm = createConfirm();
  // a list or read that finishes after the module or the view changed is dropped
  let listSeq = 0;
  let readSeq = 0;

  const accept = props.spec.ext.map((e) => `.${e}`).join(',');
  const wanted = (name: string) => props.spec.ext.some((e) => name.toLowerCase().endsWith(`.${e.toLowerCase()}`));

  let mounted = true;
  onCleanup(() => {
    mounted = false;
  });
  createEffect(
    on(props.refresh, async () => {
      const mine = ++listSeq;
      try {
        const files = await props.files.list();
        if (!mounted || mine !== listSeq) return;
        setDevice(files);
        setDsel((selected) => new Set(files.filter((file) => selected.has(file.name)).map((file) => file.name)));
      } catch (e) {
        if (!mounted || mine !== listSeq) return;
        setDevice([]);
        setDsel(new Set<string>());
        setError(errorLine(e));
      }
    }),
  );

  const addFiles = async (files: File[]) => {
    if (busy()) return;
    const picked = files.filter((f) => wanted(f.name));
    if (!picked.length) {
      setError(`Only ${props.spec.ext.map((e) => `.${e}`).join(', ')} files can be added.`);
      return;
    }
    const added: Local[] = await Promise.all(
      picked.map(async (f) => {
        const bytes = new Uint8Array(await f.arrayBuffer());
        return { name: f.name, bytes, error: previewError(props.spec.preview, bytes) };
      }),
    );
    if (!mounted) return;
    setLocal((cur) => [...cur.filter((c) => !added.some((a) => a.name === c.name)), ...added]);
    setShown({ name: added[0]!.name, bytes: added[0]!.bytes });
    setNote('');
  };

  const ticked = () => local().filter((l) => lsel().has(l.name));
  const canSend = () => ticked().length > 0 && ticked().every((l) => !l.error);
  const send = () =>
    run('Sending', async () => {
      const files = ticked();
      await props.prepareSend?.();
      for (const f of files) {
        setNote(`Sending ${f.name}…`);
        await props.files.send(f.name, f.bytes, (sent, total) =>
          setNote(`Sending ${f.name}… ${Math.round((100 * sent) / total)}%`),
        );
      }
      setNote(SENT_NOTE);
      setLsel(new Set<string>());
    });
  const removeSelected = () =>
    run('Deleting', async () => {
      for (const name of dsel()) await props.files.remove(name);
      setDsel(new Set<string>());
      setShown(undefined);
    });
  const showDevice = (name: string) => {
    const mine = ++readSeq;
    void props.files.read(name).then(
      (bytes) => {
        if (mounted && mine === readSeq) setShown({ name, bytes });
      },
      (e) => {
        if (mounted && mine === readSeq) setError(errorLine(e));
      },
    );
  };

  const lpane = createPaneSelect({
    keys: () => local().map((l) => l.name),
    selected: lsel,
    set: setLsel,
    disabled: () => writing(busy()),
  });
  const dpane = createPaneSelect({
    keys: () => device().map((d) => d.name),
    selected: dsel,
    set: (next) => {
      setDsel(next);
      confirm.stop();
    },
    disabled: () => writing(busy()),
  });

  return (
    <div class="group">
      <h4>{props.spec.label}</h4>
      <SplitPane id="transfer-files">
        <LocalPane
          onDrop={(f) => void addFiles(f)}
          select={lpane}
          selectAll={<span class="tick" />}
          actions={
            <>
              <input
                ref={input}
                type="file"
                accept={accept}
                multiple
                hidden
                onChange={(e) => {
                  const files = Array.from(e.currentTarget.files ?? []);
                  e.currentTarget.value = '';
                  void addFiles(files);
                }}
              />
              <button type="button" class="act small" disabled={!!busy()} onClick={() => input?.click()}>
                Choose…
              </button>
              <span class="drop">or drop files</span>
              <button
                type="button"
                class="act primary"
                disabled={!!busy() || !canSend()}
                title={canSend() ? 'Send the ticked files to the device' : 'Tick a valid file to send'}
                onClick={() => void send()}
              >
                {busy() === 'Sending' ? 'Sending…' : countLabel('Send', ticked().length)}
              </button>
            </>
          }
        >
          <Show when={local().length} fallback={<p class="empty">No files chosen.</p>}>
            <ul class="rows">
              <For each={local()}>
                {(l) => (
                  <ListRow
                    rowKey={l.name}
                    selected={lsel().has(l.name)}
                    onSelect={(e) => {
                      lpane.click(l.name, e);
                      setShown({ name: l.name, bytes: l.bytes });
                    }}
                    check={{
                      label: `Send ${l.name}`,
                      checked: lsel().has(l.name),
                      disabled: writing(busy()),
                      onChange: () => lpane.tick(l.name),
                    }}
                    name={l.name}
                    tags={l.error ? [{ text: 'invalid', tone: 'warn', title: l.error }] : []}
                    size={formatBytes(l.bytes.length)}
                  />
                )}
              </For>
            </ul>
          </Show>
        </LocalPane>
        <DevicePane
          ctx={props.ctx}
          hideTarget
          select={dpane}
          selectAll={<span class="tick" />}
          actions={
            <DeleteButton
              n={dsel().size}
              busy={busy()}
              confirming={confirm.confirming()}
              title="Delete the ticked files from the device"
              onClick={() => confirm.click(() => void removeSelected())}
            />
          }
        >
          <Show when={device().length} fallback={<p class="empty">The device holds no files of this module.</p>}>
            <ul class="rows">
              <For each={device()}>
                {(d) => (
                  <ListRow
                    rowKey={d.name}
                    selected={dsel().has(d.name)}
                    onSelect={(e) => {
                      dpane.click(d.name, e);
                      showDevice(d.name);
                    }}
                    check={{
                      label: `Delete ${d.name}`,
                      checked: dsel().has(d.name),
                      disabled: writing(busy()),
                      onChange: () => dpane.tick(d.name),
                    }}
                    name={d.name}
                    title={`data/${props.id}/${d.name}`}
                    size={formatBytes(d.size)}
                  />
                )}
              </For>
            </ul>
          </Show>
        </DevicePane>
      </SplitPane>
      <Show when={note()}>
        <p class="meta">{note()}</p>
      </Show>
      <Show when={shown()}>
        {(f) => (
          <>
            <p class="meta">{f().name}</p>
            <ModuleFilePreview preview={props.spec.preview} bytes={f().bytes} />
          </>
        )}
      </Show>
    </div>
  );
}
