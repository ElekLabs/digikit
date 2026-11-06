import { Show, createEffect, createSignal, on, type JSX } from 'solid-js';
import { errorLine, formatBytes } from '@web/modules/backend';
import { noteActivity } from '@web/transfer/activity';
import type { TransferSlot } from '@web/transfer/env';
import {
  DevicePane,
  ListRow,
  LocalPane,
  createConfirm,
  type ChipStatus,
  type PageCtx,
} from '@web/transfer/TransferParts';
import SplitPane from '@web/transfer/SplitPane';
import { target } from '@web/transfer/target';
import { deviceInfo } from '@web/transfer/transfer-state';
import type { BarInfo } from '@web/transfer/transfer-core';
import { openBootscreens, readActiveBootscreen } from './bootscreens-backend';
import {
  ACTIVE_BOOTSCREEN,
  readBootscreen,
  removeBootscreen,
  uploadBootscreen,
  type Bootscreen,
} from './bootscreens-core';

const [candidate, setCandidate] = createSignal<Bootscreen>();
const [active, setActive] = createSignal<{ size: number }>();
const [status, setStatus] = createSignal<ChipStatus>();
const [bar, setBar] = createSignal<BarInfo>();
let generation = 0;

export const bootscreens: TransferSlot = {
  Mode: BootscreensMode,
  async read() {
    const current = generation;
    const entry = await readActiveBootscreen();
    if (current === generation) setActive(entry);
  },
  reset() {
    generation++;
    setActive(undefined);
    setStatus(undefined);
    setBar(undefined);
  },
  bar(busy) {
    const info = bar();
    return info ? { info, running: busy === 'Uploading' } : undefined;
  },
};

export default function BootscreensMode(props: { ctx: PageCtx }): JSX.Element {
  const { busy, run, setError } = props.ctx;
  const confirm = createConfirm();
  let input: HTMLInputElement | undefined;
  let selection = 0;
  createEffect(on(target, () => confirm.stop(), { defer: true }));
  const choose = async (files: File[]) => {
    if (busy()) return;
    const token = ++selection;
    setCandidate(undefined);
    setStatus(undefined);
    setBar(undefined);
    setError('');
    try {
      if (files.length !== 1) throw new Error('Choose one DKBS file.');
      const next = await readBootscreen(files[0]!);
      if (token === selection) setCandidate(next);
    } catch (e) {
      if (token === selection) setError(errorLine(e));
    }
  };
  const upload = () => {
    const file = candidate();
    if (!file) return;
    void run('Uploading', async () => {
      setStatus({ kind: 'waiting' });
      setBar({ fraction: 0, failed: false, message: 'Uploading bootscreen…', sent: 0, total: file.bytes.length });
      try {
        const { s } = await openBootscreens();
        await uploadBootscreen(
          s,
          file.bytes,
          (sent, total) => {
            noteActivity();
            setStatus({ kind: 'uploading', sent, bytes: total });
            setBar({ fraction: sent / total, failed: false, message: 'Uploading bootscreen…', sent, total });
          },
          () => {
            setStatus({ kind: 'verifying' });
            setBar((b) => b && { ...b, message: 'Verifying bootscreen…' });
          },
        );
        setStatus({ kind: 'ok' });
        setBar({
          fraction: 1,
          failed: false,
          message: 'Verified on the card. Applies on the next normal restart.',
          sent: file.bytes.length,
          total: file.bytes.length,
        });
      } catch (e) {
        setStatus({ kind: 'failed', message: errorLine(e) });
        setBar((b) => b && { ...b, failed: true, message: errorLine(e) });
        throw e;
      }
    });
  };
  return (
    <SplitPane id="transfer">
      <LocalPane
        selectAll={undefined}
        onDrop={(files) => void choose(files)}
        actions={
          <>
            <input
              ref={input}
              type="file"
              accept=".dkbs"
              hidden
              onChange={(e) => {
                const files = Array.from(e.currentTarget.files ?? []);
                e.currentTarget.value = '';
                void choose(files);
              }}
            />
            <button type="button" class="act small" disabled={!!busy()} onClick={() => input?.click()}>
              Add .dkbs…
            </button>
            <button
              type="button"
              class="act primary"
              disabled={!!busy() || !candidate() || !deviceInfo()}
              onClick={upload}
            >
              Upload
            </button>
          </>
        }
      >
        <Show when={candidate()} fallback={<p class="empty">Choose a packed DKBS bootscreen, or drop one here.</p>}>
          {(file) => (
            <>
              <ul class="rows">
                <ListRow name={file().name} size={formatBytes(file().bytes.length)} status={status()} />
              </ul>
              <p class="empty">128 × 64 monochrome · {(file().ticks / 30).toFixed(2)} s at 30 fps</p>
            </>
          )}
        </Show>
        <p class="empty note">Create and preview assets with the DKBS pack tool and its HTML preview.</p>
      </LocalPane>
      <DevicePane
        ctx={props.ctx}
        selectAll={undefined}
        actions={
          <button
            type="button"
            class="act"
            disabled={!!busy() || !active()}
            onClick={() =>
              confirm.click(
                () =>
                  void run('Deleting', async () => {
                    await removeBootscreen((await openBootscreens()).s);
                    setStatus(undefined);
                    setBar(undefined);
                    confirm.stop();
                  }),
              )
            }
          >
            {confirm.confirming() ? 'Confirm remove' : 'Remove active'}
          </button>
        }
      >
        <Show
          when={active()}
          fallback={<p class="empty">{deviceInfo() ? 'No active bootscreen on the card.' : 'No listing yet.'}</p>}
        >
          {(file) => (
            <ul class="rows">
              <ListRow name="active.dkbs" title={ACTIVE_BOOTSCREEN} size={formatBytes(file().size)} />
            </ul>
          )}
        </Show>
        <p class="empty note">Uploading replaces the active bootscreen. Changes apply at the next normal restart.</p>
        <p class="empty note">
          Requires firmware with custom bootscreen support. The device validates the full asset at boot; missing or
          invalid assets use the stock bootscreen.
        </p>
      </DevicePane>
    </SplitPane>
  );
}
