// The Firmware page, shared by the desktop Workbench and the site. The rules are in firmware-core.ts, the run in firmware-flow.ts, and
// the state in firmware-state.ts. Transfer supplies the MIDI port, hardware connection indicator, and target selector.
import {
  For,
  Match,
  Show,
  Switch,
  batch,
  createEffect,
  createSignal,
  on,
  onCleanup,
  onMount,
  type JSX,
} from 'solid-js';
import Segmented from '../components/Segmented';
import AutoRefresh from '../transfer/AutoRefresh';
import ConnectionBadge from '../transfer/ConnectionBadge';
import TargetSelect from '../transfer/TargetSelect';
import { useFirmwareApp } from '../../src/transfer/app-context';
import DefaultModules from './DefaultModules';
import { CUSTOM_BOOT_SCREEN_HELP } from '../../src/firmware/default-modules-core';
import { createEffectOn } from '../../src/firmware/on-change';
import { newestReleasesText, runtimeDeviceIds } from '../../src/data/device-profiles';
import {
  deviceNameOf,
  PREPARE_STEPS,
  isDeviceId,
  type DeviceId,
  PREPARE_STEPS_RPC,
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
} from '../../src/firmware/firmware-core';
import './firmware-page.css';

const SEND_OPTIONS = [
  { id: 'patch', label: 'Patch and flash' },
  { id: 'restore', label: 'Restore stock', title: 'Send the stock file itself, to take digikit off the device' },
] as const;

const EMULATOR_STAGES = {
  leaving: 'Writing the image to the emulator’s flash…',
  returning: 'Waiting for the emulator to restart and verifying the firmware…',
  asking: 'The emulator booted the flashed firmware.',
};

/**
 * One step of the page. A done step that has a `summary` folds to its header with that one line (the steps before the current one stay out of
 * the way); its Show button opens it again. `aside` stays in the header either way.
 */
function Step(props: {
  n: number;
  title: string;
  mark: StepMark;
  children: JSX.Element;
  aside?: JSX.Element;
  summary?: JSX.Element;
  /** The step stays open while true, e.g. step 1 until the flash starts, so its controls stay in reach. */
  hold?: boolean;
}): JSX.Element {
  const [shown, setShown] = createSignal(false);
  const folded = () => props.mark === 'done' && props.summary !== undefined && !props.hold && !shown();
  // a new file or a failed check: the user's Show is forgotten
  createEffect(
    on(
      () => props.mark,
      (m) => m !== 'done' && setShown(false),
      { defer: true },
    ),
  );
  return (
    <section class="step" classList={{ [props.mark]: true, folded: folded() }} aria-labelledby={`fw-step-${props.n}`}>
      <header class="steph">
        <span class="mark" aria-hidden="true">
          {props.mark === 'done' ? '✓' : props.mark === 'bad' ? '!' : props.n}
        </span>
        <h3 id={`fw-step-${props.n}`}>{props.title}</h3>
        <Show when={folded()}>
          <span class="summary">{props.summary}</span>
        </Show>
        <span class="sp" />
        {props.aside}
        <Show when={props.mark === 'done' && props.summary !== undefined && !props.hold}>
          <button
            type="button"
            class="linkbtn fold"
            aria-expanded={!folded()}
            aria-controls={`fw-step-${props.n}-body`}
            onClick={() => setShown(!shown())}
          >
            {folded() ? 'Show' : 'Hide'}
          </button>
        </Show>
      </header>
      <div class="stepb" id={`fw-step-${props.n}-body`} hidden={folded()}>
        {props.children}
      </div>
    </section>
  );
}

export default function FirmwarePage(): JSX.Element {
  const fw = useFirmwareApp();
  const { rescanTargets, target } = fw.transfer.target;
  const { customBootScreen, postRun } = fw.defaults;
  const {
    ack,
    cancelFlash,
    chooseFile,
    device,
    downloadImage,
    fileDevice,
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
    setCustomBoot,
    setRestore,
    setRoute,
    startFlash,
  } = fw.state;
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
  // Leaving the page while the image is sent: the device gets End with commit 0 and keeps its flash.
  const leave = () => {
    const r = run();
    if (r.phase === 'sending' && !r.flashing) cancelFlash();
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

  const emulator = () => target().kind === 'emulator' && !!fw.env.emulatorFlash;
  const flashRoute = () => (emulator() ? 'rpc' : route());
  /** The chosen file's device has no boot screen image to add. */
  const noBootImage = () => {
    const d = fileDevice();
    return d !== undefined && !d.patchSets['device-introblank'];
  };
  const inputs = () => ({ prepare: prepared(), device: device(), ack: ack(), run: run(), route: flashRoute() });
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
      return emulator()
        ? d.reason
        : `${d.reason.replace(/\.?$/, '.')} Check the USB cable. The device must be on, and its USB mode must be MIDI.`;
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
  const rebooting = () => {
    const r = run();
    return r.phase === 'rebooting' ? r : undefined;
  };
  const setup = () => {
    const r = rebooting();
    return r?.stage === 'asking' ? r.setup : undefined;
  };
  const stopped = () => {
    const r = run();
    return r.phase === 'stopped' ? r : undefined;
  };
  const finished = () => {
    const r = run();
    return r.phase === 'done' || r.phase === 'failed' ? r : undefined;
  };
  /** The name of the connected device (else the image's device). */
  const deviceName = () => {
    const d = device();
    const id = d.kind === 'connected' ? d.d.model : undefined;
    const p = ready()?.p.device;
    return deviceNameOf(
      id ?? (isDeviceId(p ?? '') ? (p as DeviceId) : ((fw.env.devices ?? runtimeDeviceIds)[0] ?? '')),
    );
  };
  const shortName = (d: string) => d.toUpperCase();
  /** The device id the default modules are for: the image's device, else the connected one. */
  const modelId = () => {
    const d = device();
    return ready()?.p.device ?? (d.kind === 'connected' ? d.d.model : undefined);
  };

  return (
    <section class="firmware-page" aria-label="Firmware">
      <header class="head">
        <h2>Firmware</h2>
        <Segmented
          label="What to send"
          options={SEND_OPTIONS}
          value={restore() ? 'restore' : 'patch'}
          onChange={(v) => setRestore(v === 'restore')}
          disabled={running(run())}
        />
        <span class="dim">
          <Show
            when={emulator()}
            fallback={
              restore()
                ? 'Send the stock file itself, to take digikit off the device.'
                : 'Patch the stock firmware and flash it over MIDI.'
            }
          >
            {restore()
              ? 'Reinstall the stock firmware in the emulator.'
              : 'Patch the stock firmware and flash the emulator.'}
          </Show>
        </span>
      </header>
      <AutoRefresh
        busy={() => (running(run()) ? 'Flashing' : '')}
        read={() => void read()}
        readOnPoll={emulator}
        rescanPorts={() => {
          batch(() => {
            setReload((n) => n + 1);
            rescanTargets();
          });
        }}
      />

      <div class="steps">
        {/* 1 */}
        <Step
          n={1}
          title="Firmware"
          mark={marks()[0]!}
          hold={run().phase === 'idle'}
          summary={
            ready() ? (
              <>
                <span class="fname">{ready()!.name}</span> {shortName(ready()!.p.device)} {ready()!.p.os}
                {ready()!.p.restore ? ' · stock, sent as it is' : ''}
              </>
            ) : undefined
          }
        >
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
                    No file yet. Choose the stock .syx of your device ({newestReleasesText(fw.env.devices)}). It is not
                    uploaded.
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
            <DefaultModules
              device={modelId()}
              off={
                restore() ? 'Not used when the stock firmware is restored' : running(run()) ? 'A flash is running' : ''
              }
              note="Uploaded and enabled after the flash."
            />
          </div>
          <label class="tick">
            <input
              type="checkbox"
              checked={customBootScreen() && !noBootImage()}
              disabled={running(run()) || restore() || noBootImage()}
              title={noBootImage() ? 'This device has no boot screen image.' : undefined}
              onChange={(e) => setCustomBoot(e.currentTarget.checked)}
            />
            <span>
              Custom boot screen <span class="dim">({CUSTOM_BOOT_SCREEN_HELP})</span>
            </span>
          </label>
        </Step>

        {/* 2 */}
        <Step
          n={2}
          title="Checks"
          mark={marks()[1]!}
          summary={ready()?.p.ok ? `${checkRows(ready()!.p).length} checks passed` : undefined}
          aside={
            <Show when={ready()?.p.ok}>
              <>
                <Show when={fw.env.runInEmulator && !ready()!.p.restore}>
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
                  {flashRoute() === 'menu' &&
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
        <Step
          n={3}
          title={emulator() ? 'Target' : 'Device'}
          mark={marks()[2]!}
          summary={match().state === 'match' ? match().text : undefined}
          aside={fw.transfer.env.targets.inHeader ? undefined : <TargetSelect reload={reload} />}
        >
          <div class="devrow">
            <Show when={emulator()} fallback={<ConnectionBadge />}>
              <span role="status">
                {device().kind === 'connected'
                  ? `${deviceName()} emulator`
                  : device().kind === 'checking'
                    ? 'Checking the emulator…'
                    : 'Emulator'}
              </span>
            </Show>
            <Show when={match().state === 'match'}>
              <span class="okt">{match().text} ✓</span>
            </Show>
            <Show when={match().text && (match().state === 'mismatch' || match().state === 'unknown')}>
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
        <Step
          n={4}
          title={emulator() ? 'Prepare the emulator' : 'Prepare the device'}
          mark={marks()[3]!}
          hold={run().phase === 'idle'}
          summary={
            emulator()
              ? 'Workbench handles the restart'
              : route() === 'rpc'
                ? 'Power adapter in, USB cable stays in'
                : 'The device shows WAITING FOR SYSEX'
          }
        >
          <Switch>
            <Match when={emulator()}>
              <p class="dim">
                Boot the emulator from your stock file and let it run. Workbench sends the update and restarts the
                emulator automatically.
              </p>
            </Match>
            <Match when={route() === 'rpc'}>
              <ol class="plain">
                <For each={PREPARE_STEPS_RPC}>{(s) => <li>{s}</li>}</For>
              </ol>
              <RecoveryNote />
              <button type="button" class="linkbtn" disabled={running(run())} onClick={() => setRoute('menu')}>
                Use the OS UPGRADE menu instead
              </button>
            </Match>
            <Match when={true}>
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
            </Match>
          </Switch>
        </Step>

        {/* 5 */}
        <Step n={5} title={restore() ? 'Restore' : 'Flash'} mark={marks()[4]!}>
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
                  <span class="ptext" classList={{ okt: s().flashing }}>
                    {emulator() && s().flashing
                      ? EMULATOR_STAGES.leaving
                      : emulator() && s().checking
                        ? 'The emulator is checking the image…'
                        : progressView(s()).text}
                  </span>
                  <Show when={!s().flashing}>
                    <button type="button" class="act" onClick={cancelFlash}>
                      Cancel
                    </button>
                  </Show>
                </div>
              )}
            </Match>
            <Match when={rebooting()}>
              {(s) => (
                <Show when={!emulator()} fallback={<p role="status">{EMULATOR_STAGES[s().stage]}</p>}>
                  <div class="flashing">
                    <div class="progress full" role="progressbar">
                      <i style={{ width: '100%' }} />
                    </div>
                    <span class="ptext">{rebootText(s())}</span>
                    <button type="button" class="act" onClick={cancelFlash}>
                      Stop waiting
                    </button>
                  </div>
                </Show>
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
                  {emulator()
                    ? restore()
                      ? 'Restore stock on emulator'
                      : 'Flash emulator'
                    : restore()
                      ? 'Restore stock'
                      : 'Flash'}
                </button>
                <Show when={!flash().ok}>
                  <span class="dim">{flash().reason}</span>
                </Show>
                <Show when={flash().ok && ready()}>
                  <span class="dim">
                    {emulator()
                      ? `Sends ${fmtMB(ready()!.p.image.length)} to the ${deviceName()} emulator, then verifies the image after it restarts.`
                      : route() === 'rpc'
                        ? `Sends ${fmtMB(ready()!.p.image.length)} to your ${deviceName()}; the device checks it, then flashes and restarts.`
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

        {/* last */}
        <Step n={6} title="Done" mark={marks()[5]!}>
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
                  <PostRunView />
                  <button type="button" class="act small" disabled={postRun().kind === 'working'} onClick={resetRun}>
                    Close
                  </button>
                </>
              )}
            </Match>
            <Match when={emulator() && setup()}>
              {(s) => (
                <>
                  <p class="okt">Firmware verified. Workbench is finishing emulator setup.</p>
                  <Show when={postRun().kind === 'working'} fallback={<SetupProgress progress={s()} />}>
                    <PostRunView />
                  </Show>
                </>
              )}
            </Match>
            <Match when={!emulator() && route() === 'rpc' && rebooting()}>
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
              <p class="dim">
                {emulator()
                  ? 'Workbench shows the running firmware here after verification.'
                  : 'After the restart the page asks the device which image it runs.'}
              </p>
            </Match>
          </Switch>
        </Step>
      </div>
    </section>
  );
}

function SetupProgress(props: { progress: { fraction?: number; text: string } }): JSX.Element {
  const percent = () => (props.progress.fraction ?? 0) * 100;
  return (
    <div class="flashing" role="status">
      <Show when={props.progress.fraction !== undefined}>
        <div
          class="progress"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(percent())}
        >
          <i style={{ width: `${percent()}%` }} />
        </div>
      </Show>
      <span class="ptext">{props.progress.text}</span>
    </div>
  );
}

/** The default modules that follow a flash: the progress, the lines, or the module that failed. */
function PostRunView(): JSX.Element {
  const { postRun } = useFirmwareApp().defaults;
  return (
    <Switch>
      <Match when={postRun().kind === 'working' && (postRun() as { text: string; done: number; total: number })}>
        {(r) => <SetupProgress progress={{ text: r().text, fraction: r().done / r().total }} />}
      </Match>
      <Match when={postRun().kind === 'done' && (postRun() as { lines: string[] })}>
        {(r) => (
          <>
            <p class="okt">Default modules installed.</p>
            <ul class="plain">
              <For each={r().lines}>{(l) => <li>{l}</li>}</For>
            </ul>
          </>
        )}
      </Match>
      <Match when={postRun().kind === 'failed' && (postRun() as { message: string })}>
        {(r) => (
          <p class="bad" role="alert">
            {r().message}
          </p>
        )}
      </Match>
    </Switch>
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
