// The Firmware page, shared by the desktop workbench and the site (the app gives its parts in env.ts): six steps from the stock file to the
// device running the new image. Step by step, nothing is sent until the checks have passed, the device is the image's model, and the user
// has said that the device shows WAITING FOR SYSEX. The rules are in firmware-core.ts, the run in firmware-flow.ts, the state in
// firmware-state.ts. The MIDI port, the connection indicator and the target selector are the Transfer pane's.
import { For, Match, Show, Switch, createSignal, onCleanup, onMount, type JSX } from 'solid-js';
import AutoRefresh from '../transfer/AutoRefresh';
import ConnectionBadge from '../transfer/ConnectionBadge';
import TargetSelect from '../transfer/TargetSelect';
import { target } from '../transfer/target';
import { firmwareEnv } from './env';
import { createEffectOn } from './on-change';
import {
  PREPARE_STEPS,
  PREPARE_STEPS_RPC,
  confirmText,
  fmtMB,
  RECOVERY_STEPS,
  canFlash,
  checkRows,
  matchView,
  progressView,
  rebootText,
  running,
  sendSeconds,
  shortSha,
  stepMarks,
  groups,
  type StepMark,
} from './firmware-core';
import {
  ack,
  answerConfirm,
  cancelFlash,
  chooseFile,
  device,
  downloadImage,
  forgetDevice,
  notice,
  prepared,
  probeDevice,
  resetRun,
  restore,
  route,
  runInEmulator,
  run,
  setAck,
  setRestore,
  setRoute,
  startFlash,
} from './firmware-state';
import './firmware-page.css';

function Step(props: {
  n: number;
  title: string;
  mark: StepMark;
  children: JSX.Element;
  aside?: JSX.Element;
}): JSX.Element {
  return (
    <section class="step" classList={{ [props.mark]: true }} aria-labelledby={`fw-step-${props.n}`}>
      <header class="steph">
        <span class="mark" aria-hidden="true">
          {props.mark === 'done' ? '✓' : props.mark === 'bad' ? '!' : props.n}
        </span>
        <h3 id={`fw-step-${props.n}`}>{props.title}</h3>
        <span class="sp" />
        {props.aside}
      </header>
      <div class="stepb">{props.children}</div>
    </section>
  );
}

export default function FirmwarePage(): JSX.Element {
  const [reload, setReload] = createSignal(0);
  const [dragging, setDragging] = createSignal(false);
  let input: HTMLInputElement | undefined;
  let probing = false;
  const read = async () => {
    if (probing) return;
    probing = true;
    try {
      await probeDevice();
    } finally {
      probing = false;
    }
  };
  onMount(() => void read());
  // Leaving the page while the image is sent or the question is open: the device gets End with commit 0 and keeps its flash.
  const leave = () => {
    const r = run().phase;
    if (r === 'sending' || r === 'confirm') cancelFlash();
  };
  onMount(() => window.addEventListener('pagehide', leave));
  onCleanup(() => {
    window.removeEventListener('pagehide', leave);
    leave();
  });
  // another port: the old reading is not shown
  createEffectOn(target, () => {
    forgetDevice();
    void read();
  });

  const inputs = () => ({ prepare: prepared(), device: device(), ack: ack(), run: run(), route: route() });
  const marks = () => stepMarks(inputs());
  const flash = () => canFlash(inputs());
  const ready = () => {
    const p = prepared();
    return p.kind === 'ready' ? p : undefined;
  };
  const match = () => matchView(ready()?.p, device());
  /** Why there is no device: no port, or the port does not answer. */
  const notConnected = () => {
    const d = device();
    if (d.kind === 'none') return d.why;
    if (d.kind === 'lost')
      return `${d.reason.replace(/\.?$/, '.')} Check the USB cable. The device must be on, and its USB mode must be MIDI.`;
    return undefined;
  };
  const working = () => {
    const p = prepared();
    return p.kind === 'working' ? p : undefined;
  };
  const refused = () => {
    const p = prepared();
    return p.kind === 'error' ? p : undefined;
  };
  const sending = () => {
    const r = run();
    return r.phase === 'sending' ? r : undefined;
  };
  /** rpc: the device has the image, the question is asked or answered and the flash is under way. */
  const afterSend = () => {
    const r = run();
    return route() === 'rpc' && (r.phase === 'confirm' || r.phase === 'rebooting') ? r : undefined;
  };
  const rebooting = () => {
    const r = run();
    return r.phase === 'rebooting' ? r : undefined;
  };
  const stopped = () => {
    const r = run();
    return r.phase === 'stopped' ? r : undefined;
  };
  const finished = () => {
    const r = run();
    return r.phase === 'done' || r.phase === 'failed' ? r : undefined;
  };
  const shortName = (d: string) => d.toUpperCase();

  return (
    <section class="firmware-page" aria-label="Firmware">
      <header class="head">
        <h2>Firmware</h2>
        <span class="dim">Patch the stock firmware and flash it over MIDI, or go back to stock.</span>
      </header>
      <AutoRefresh
        busy={() => (running(run()) ? 'Flashing' : '')}
        read={() => void read()}
        rescanPorts={() => setReload((n) => n + 1)}
      />

      <div class="steps">
        {/* 1 */}
        <Step n={1} title="Firmware" mark={marks()[0]!}>
          <input
            ref={input}
            class="hidden"
            type="file"
            accept=".syx,application/octet-stream"
            aria-label="Stock firmware file"
            onChange={(e) => {
              void chooseFile(e.currentTarget.files?.[0]);
              e.currentTarget.value = '';
            }}
          />
          <div
            class="filerow"
            classList={{ dragging: dragging() }}
            onDragOver={(e) => {
              e.preventDefault();
              setDragging(true);
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={(e) => {
              e.preventDefault();
              setDragging(false);
              void chooseFile(e.dataTransfer?.files[0]);
            }}
          >
            <div class="fileinfo">
              <Switch>
                <Match when={prepared().kind === 'none'}>
                  <span class="dim">
                    No file yet. Choose the stock .syx of your device (Digitakt II OS 1.17 or Digitone II OS 1.12). It
                    is not uploaded.
                  </span>
                </Match>
                <Match when={working()}>
                  {(p) => (
                    <>
                      <span class="fname">{p().name}</span>
                      <span class="dim">patching and checking…</span>
                    </>
                  )}
                </Match>
                <Match when={refused()}>
                  {(p) => (
                    <>
                      <span class="fname">{p().name}</span>
                      <span class="bad" role="alert">
                        {p().message}
                      </span>
                    </>
                  )}
                </Match>
                <Match when={ready()}>
                  {(p) => (
                    <>
                      <span class="fname">{p().name}</span>
                      <span class="dim">
                        {shortName(p().p.device)} {p().p.os}
                      </span>
                      <span class="okt">stock ✓ (sha {shortSha(p().p.shaStock)})</span>
                    </>
                  )}
                </Match>
              </Switch>
            </div>
            <button type="button" class="act" disabled={running(run())} onClick={() => input?.click()}>
              Choose…
            </button>
          </div>
          <label class="tick">
            <input
              type="checkbox"
              checked={restore()}
              disabled={running(run())}
              onChange={(e) => setRestore(e.currentTarget.checked)}
            />
            <span>
              Restore stock <span class="dim">(send the stock file itself, to take digikit off the device)</span>
            </span>
          </label>
        </Step>

        {/* 2 */}
        <Step
          n={2}
          title="Checks"
          mark={marks()[1]!}
          aside={
            <Show when={ready()?.p.ok}>
              <>
                <Show when={firmwareEnv().runInEmulator && !ready()!.p.restore}>
                  <button type="button" class="act small" onClick={() => void runInEmulator()}>
                    Run in emulator
                  </button>
                </Show>
                <button type="button" class="act small" onClick={() => void downloadImage()}>
                  Download .syx
                </button>
              </>
            </Show>
          }
        >
          <Show
            when={ready()}
            fallback={
              <p class="dim">
                {prepared().kind === 'working'
                  ? 'Patching and checking. This takes a few seconds.'
                  : 'The checks run when a file is chosen.'}
              </p>
            }
          >
            {(r) => (
              <>
                <ul class="checks">
                  <For each={checkRows(r().p)}>
                    {(c) => (
                      <li classList={{ ok: c.ok, bad: !c.ok }} title={c.detail}>
                        <span class="ic" aria-hidden="true">
                          {c.ok ? '✓' : '✗'}
                        </span>
                        <span class="lb">{c.label}</span>
                        <Show when={!c.ok}>
                          <span class="why">{c.detail}</span>
                        </Show>
                      </li>
                    )}
                  </For>
                </ul>
                <p class="dim">
                  {r().p.restore ? 'Stock image' : `Patched image with digikit runtime ${r().p.runtime}`} ·{' '}
                  {groups(r().p.image.length)} bytes
                  {route() === 'menu' &&
                    ` · ${groups(r().p.ends.length)} messages · about ${sendSeconds(r().p)} s to send`}
                </p>
                <Show when={notice()}>
                  <p class="dim" role="status">
                    {notice()}
                  </p>
                </Show>
              </>
            )}
          </Show>
        </Step>

        {/* 3 */}
        <Step n={3} title="Device" mark={marks()[2]!} aside={<TargetSelect reload={reload} />}>
          <div class="devrow">
            <ConnectionBadge />
            <Show when={match().state === 'match'}>
              <span class="okt">{match().text} ✓</span>
            </Show>
            <Show when={match().state === 'mismatch' || match().state === 'unknown'}>
              <span class="bad" role="alert">
                {match().text}
              </span>
            </Show>
          </div>
          <Show when={match().os}>
            <p class="dim">{match().os}</p>
          </Show>
          <Show when={notConnected()}>{(why) => <p class="dim">{why()}</p>}</Show>
        </Step>

        {/* 4 */}
        <Step n={4} title="Prepare the device" mark={marks()[3]!}>
          <Show
            when={route() === 'rpc'}
            fallback={
              <>
                <ol class="plain">
                  <For each={PREPARE_STEPS}>{(s) => <li>{s}</li>}</For>
                </ol>
                <RecoveryNote />
                <label class="tick">
                  <input
                    type="checkbox"
                    checked={ack()}
                    disabled={running(run())}
                    onChange={(e) => setAck(e.currentTarget.checked)}
                  />
                  <span>The device shows WAITING FOR SYSEX</span>
                </label>
                <button type="button" class="linkbtn" disabled={running(run())} onClick={() => setRoute('rpc')}>
                  Send directly instead (no menu on the device)
                </button>
              </>
            }
          >
            <ol class="plain">
              <For each={PREPARE_STEPS_RPC}>{(s) => <li>{s}</li>}</For>
            </ol>
            <RecoveryNote />
            <button type="button" class="linkbtn" disabled={running(run())} onClick={() => setRoute('menu')}>
              Use the OS UPGRADE menu instead
            </button>
          </Show>
        </Step>

        {/* 5 */}
        <Step n={5} title={route() === 'rpc' ? 'Send' : restore() ? 'Restore' : 'Flash'} mark={marks()[4]!}>
          <Switch>
            <Match when={sending()}>
              {(s) => (
                <div class="flashing">
                  <div
                    class="progress"
                    role="progressbar"
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={Math.round(progressView(s()).fraction * 100)}
                  >
                    <i style={{ width: `${progressView(s()).fraction * 100}%` }} />
                  </div>
                  <span class="ptext">{progressView(s()).text}</span>
                  <button type="button" class="act" onClick={cancelFlash}>
                    Cancel
                  </button>
                </div>
              )}
            </Match>
            <Match when={afterSend()}>
              <div class="flashing">
                <div class="progress ok" role="progressbar" aria-valuenow={100} aria-valuemin={0} aria-valuemax={100}>
                  <i style={{ width: '100%' }} />
                </div>
                <span class="ptext okt">
                  received {fmtMB(ready()?.p.image.length ?? 0)} · the device checked the image ✓
                </span>
              </div>
            </Match>
            <Match when={rebooting()}>
              {(s) => (
                <div class="flashing">
                  <div class="progress full" role="progressbar">
                    <i style={{ width: '100%' }} />
                  </div>
                  <span class="ptext">{rebootText(s())}</span>
                  <button type="button" class="act" onClick={cancelFlash}>
                    Stop waiting
                  </button>
                </div>
              )}
            </Match>
            <Match when={finished()}>
              <p class="dim">
                {run().phase === 'done'
                  ? `The ${restore() ? 'restore' : 'flash'} is finished. The result is below.`
                  : `The ${restore() ? 'restore' : 'flash'} did not finish as planned. The reason is below.`}
              </p>
            </Match>
            <Match when={true}>
              <div class="flashrow">
                <button type="button" class="act primary" disabled={!flash().ok} onClick={() => void startFlash()}>
                  {route() === 'rpc'
                    ? restore()
                      ? 'Send stock firmware'
                      : 'Send'
                    : restore()
                      ? 'Restore stock'
                      : 'Flash'}
                </button>
                <Show when={!flash().ok}>
                  <span class="dim">{flash().reason}</span>
                </Show>
                <Show when={flash().ok && ready()}>
                  <span class="dim">
                    {route() === 'rpc'
                      ? `Sends ${fmtMB(ready()!.p.image.length)} and lets the device check it. Nothing is flashed until you say so.`
                      : `Sends ${groups(ready()!.p.ends.length)} messages, about ${sendSeconds(ready()!.p)} s. The device then writes its flash and restarts.`}
                  </span>
                </Show>
              </div>
              <Show when={stopped()}>
                {(s) => (
                  <p class={s().reason === 'cancelled' ? 'warn' : 'bad'} role="alert">
                    {s().message}
                  </p>
                )}
              </Show>
            </Match>
          </Switch>
        </Step>

        {/* 6, the RPC route only: the question the device does not ask */}
        <Show when={route() === 'rpc'}>
          <Step n={6} title="Confirm" mark={marks()[5]!}>
            <Switch>
              <Match when={run().phase === 'confirm'}>
                <p>{ready() ? confirmText(ready()!.p) : ''} Flash now?</p>
                <div class="flashrow">
                  <button type="button" class="act primary" onClick={() => answerConfirm(true)}>
                    Flash now
                  </button>
                  <button type="button" class="act" onClick={() => answerConfirm(false)}>
                    Cancel
                  </button>
                </div>
                <p class="dim">Cancel leaves the device as it was.</p>
              </Match>
              <Match when={run().phase === 'rebooting' || run().phase === 'done' || run().phase === 'failed'}>
                <p class="dim">You said yes. The device flashed itself.</p>
              </Match>
              <Match when={true}>
                <p class="dim">When the device has checked the image, you decide here whether it is flashed.</p>
              </Match>
            </Switch>
          </Step>
        </Show>

        {/* last */}
        <Step n={route() === 'rpc' ? 7 : 6} title="Done" mark={marks()[route() === 'rpc' ? 6 : 5]!}>
          <Switch>
            <Match when={finished()}>
              {(s) => (
                <>
                  <Show
                    when={s().phase === 'done'}
                    fallback={
                      <p class="bad" role="alert">
                        {(s() as { message: string }).message}
                      </p>
                    }
                  >
                    <p class="okt">{(s() as { message: string }).message}</p>
                    <ul class="plain">
                      <For each={(s() as { lines: string[] }).lines}>{(l) => <li>{l}</li>}</For>
                    </ul>
                  </Show>
                  <button type="button" class="act small" onClick={resetRun}>
                    Close
                  </button>
                </>
              )}
            </Match>
            <Match when={route() === 'rpc' && rebooting()}>
              {(s) => (
                <>
                  <p class="dim">{rebootText(s())}</p>
                  <button type="button" class="act small" onClick={cancelFlash}>
                    Stop waiting
                  </button>
                </>
              )}
            </Match>
            <Match when={true}>
              <p class="dim">After the restart the page asks the device which image it runs.</p>
            </Match>
          </Switch>
        </Step>
      </div>
    </section>
  );
}

/** The recovery steps, folded away. */
function RecoveryNote(): JSX.Element {
  return (
    <details class="recovery">
      <summary>Recovery steps</summary>
      <p class="dim">If the device does not start again after a flash:</p>
      <ol class="plain">
        <For each={RECOVERY_STEPS}>{(s) => <li>{s}</li>}</For>
      </ol>
    </details>
  );
}
