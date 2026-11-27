// The Remote page (desktop): the front panel of the selected target with its real screen, the keys and the encoders working on the device.
// The screen is the firmware's own Screenshot (stock MidiRpc 0x04), polled; keys and encoders are the commands of digikit/remote (KEY, TURN),
// which work while digikit/remote is loaded at the device. The page speaks the protocol itself in TypeScript over the target of
// target.ts: the emulator's USB capture or a MIDI port. No process is started.
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { Show, createSignal, onCleanup, onMount, type JSX } from 'solid-js';
import LivePanel, { type LivePanelApi } from '@digi/workbench-core/ui/components/LivePanel.tsx';
import PanelArt from '@digi/workbench-core/ui/components/PanelArt.tsx';
import { defaultFaceplate, panels } from '@digi/workbench-core/data/faceplates.ts';
import { deviceOfInfoPort, registry, type DeviceId } from '@digi/workbench-core/data/device-profiles.ts';
import { identifyDevice, type DkSession, type RemoteState } from '@digi/dk-session';
import {
  infoDue,
  isRemoteMissing,
  keyErrorLine,
  leaveCancels,
  remoteListed,
  remoteNotice,
  remotePaused,
  remoteRuntime,
  remoteStateWanted,
  stateProbeKey,
} from './session';
import { windowShown } from '../log-follow-core';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { targetLabel } from '@digi/workbench-core/transfer/target.ts';
import { failureLine } from '@digi/workbench-core/transfer/wait-core.ts';
import './remote-page.css';

const SCREEN_MS = { emulator: 250, midi: 400 };
const STATE_MS = 2000;
/** How long the window may stay hidden before the loops look again. */
const HIDDEN_MS = 500;
// Keys and knobs are LivePanel's (the knobs turn by drag and wheel), so the page has no extra buttons for them.
const HINT = 'Click keys; drag or scroll knobs. Shift: ×10.';

export default function RemotePage(props: { source: TargetBinding }): JSX.Element {
  const app = useTransferApp();
  const { noteReply, noteConnection } = app.connection;
  const { cancelPending, checkRuntime, reconnect, target } = app.target;
  const session = () => props.source.session();
  const { busy } = app.state;
  const [state, setState] = createSignal<RemoteState>();
  const [stateError, setStateError] = createSignal<unknown>();
  const [screenError, setScreenError] = createSignal('');
  // a failed key or turn: it stays until a key succeeds or the target changes (the screen poll does not clear it)
  const [keyError, setKeyError] = createSignal('');
  // whether MODULES lists digikit/remote, read when the state request failed (undefined: not read)
  const [listed, setListed] = createSignal<boolean>();
  const [device, setDevice] = createSignal<DeviceId>();
  let live: LivePanelApi | undefined;
  let alive = true;
  let stock = false;
  let stockOs: string | undefined;
  const panel = () => {
    const d = device();
    return d ? panels[d] : undefined;
  };
  const artwork = () => panel() ?? panels[defaultFaceplate];
  const runtime = remoteRuntime(
    () => session(),
    {
      ok: () => setKeyError(''),
      failed: (e) => setKeyError(keyErrorLine(e, targetLabel(props.source.target))),
    },
    () => {
      props.source.check();
      if (!alive) throw new Error('Remote page closed.');
    },
  );
  const notice = () => {
    if (!device())
      return stateError()
        ? 'No supported device identified. Check the selected MIDI port. Retrying…'
        : 'Waiting for device identification…';
    return remoteNotice(state(), stateError(), listed());
  };

  /** Screen loop: one Screenshot after the other; an error shows and the loop tries again slowly. */
  const screenLoop = async () => {
    while (alive) {
      // nothing is asked of the device while the window is hidden or a Transfer job uses the session
      if (!device() || !windowShown() || remotePaused(busy()) || app.jobs.busy('target')) {
        await new Promise((r) => setTimeout(r, HIDDEN_MS));
        continue;
      }
      const t = props.source.target;
      try {
        const s: DkSession = await session();
        const shot = await s.screen();
        if (!alive || t !== target()) return;
        if (stock && app.connection.connection().state !== 'noruntime')
          noteConnection({ type: 'noruntime', reason: 'Stock firmware', os: stockOs });
        live?.showFrame(shot);
        setScreenError('');
        await new Promise((r) => setTimeout(r, SCREEN_MS[t.kind === 'midi' ? 'midi' : 'emulator']));
      } catch (e) {
        if (!alive || t !== target()) return;
        if (stock) noteConnection({ type: 'lost', reason: failureLine(e, targetLabel(t)) });
        setScreenError(failureLine(e, targetLabel(t)));
        // the session recovers the link between tries; this covers a failure outside a request
        await reconnect().catch(() => {});
        await new Promise((r) => setTimeout(r, 1500));
      }
    }
  };
  const stateLoop = async () => {
    // set when the device answered E_PROTO: from then on the MODULES list says when digikit/remote is there (no failing request every poll)
    let missing = false;
    // set when digikit/remote is listed but REMOTE_STATE answered E_PROTO: not asked again until the target or the module list changes
    let unavailableKey: string | undefined;
    // INFO only names the device for the faceplate: ask when the target changed, after an error and now and then, not every poll
    let infoKey = '';
    let infoAt: number | undefined;
    while (alive) {
      if (!windowShown() || remotePaused(busy()) || app.jobs.busy('target')) {
        await new Promise((r) => setTimeout(r, HIDDEN_MS));
        continue;
      }
      const t = props.source.target;
      let loaded: Awaited<ReturnType<DkSession['modules']>> | undefined;
      try {
        checkRuntime(t);
        const s = await session();
        const key = targetLabel(target());
        if (infoDue(infoAt, Date.now(), key !== infoKey)) {
          const identity = await identifyDevice(s, registry, { preferRuntimeInfo: true });
          if (!alive || t !== target()) return;
          infoKey = key;
          infoAt = Date.now();
          const d = deviceOfInfoPort(identity.id);
          setDevice(d);
          stock = !identity.info;
          stockOs = identity.os;
          if (stock) {
            noteConnection({ type: 'noruntime', reason: 'Stock firmware', os: identity.os });
            setStateError('Remote key control requires the digikit runtime.');
          }
        }
        if (stock) {
          await new Promise((r) => setTimeout(r, STATE_MS));
          continue;
        }
        loaded = missing ? await s.modules().catch(() => undefined) : undefined;
        const probe = stateProbeKey(key, loaded);
        if (remoteStateWanted(missing, loaded, unavailableKey, probe)) {
          setState(await s.remoteState());
          setStateError(undefined);
          setListed(undefined);
          missing = false;
          unavailableKey = undefined;
          void noteReply(s, () => alive && t === target());
        } else if (loaded) setListed(remoteListed(loaded));
      } catch (e) {
        if (!alive || t !== target()) return;
        missing = isRemoteMissing(e);
        infoAt = undefined;
        setState(undefined);
        setStateError(e);
        // the module may be loaded without the optional state request: MODULES decides what the notice says
        if (!device()) {
          await reconnect().catch(() => {});
        } else if (missing) {
          const list =
            loaded ??
            (await session()
              .then((s) => s.modules())
              .catch(() => undefined));
          const isListed = remoteListed(list);
          setListed(isListed);
          unavailableKey = isListed ? stateProbeKey(targetLabel(t), list) : undefined;
        }
      }
      await new Promise((r) => setTimeout(r, STATE_MS));
    }
  };
  let releasing: Promise<void> | undefined;
  const release = () => (releasing ??= runtime.releaseHeld().catch(() => {}));
  const stopListening = props.source.beforeLeave(release);
  onMount(() => {
    void screenLoop();
    void stateLoop();
  });
  onCleanup(() => {
    alive = false;
    // a poll that waits for a silent device must not hold up the next page's requests (they take turns on the session); a Transfer job's
    // requests are not ours to cancel
    if (props.source.target === target() && !app.target.switching() && !app.jobs.busy('target') && leaveCancels(busy()))
      cancelPending();
    void release().finally(stopListening);
  });

  return (
    <section class="remote-page" aria-label="Remote">
      <header class="head">
        <h2>Remote</h2>
        <span class="meta">
          {targetLabel(props.source.target)}
          {device() && ` · ${device()}`}
        </span>
        <Show when={panel()}>
          <span class="hints" title={HINT}>
            <span>
              <kbd>Click</kbd> keys
            </span>
            <span>
              <kbd>Drag</kbd> or <kbd>Scroll</kbd> knobs
            </span>
            <span>
              <kbd>⇧</kbd> ×10
            </span>
          </span>
        </Show>
      </header>
      <Show when={remotePaused(busy()) || app.jobs.busy('target')}>
        <p class="note" role="status">
          Paused during the transfer.
        </p>
      </Show>
      <Show when={notice()}>
        <p class="note" role="status">
          {notice()}
        </p>
      </Show>
      <Show when={screenError()}>
        <p class="err" role="alert">
          {screenError()}
        </p>
      </Show>
      <Show when={keyError()}>
        <p class="err" role="alert">
          {keyError()}
        </p>
      </Show>
      {/* the same markup as the Emulator page's faceplate (WorkbenchShell.tsx), so the screen has the same place and scale; the side column with the Memory panel is beside it */}
      <div class="remote-row">
        <Show when={artwork()}>
          {(faceplate) => (
            <main
              class="stage"
              aria-label={panel() ? `${faceplate().name} front panel` : 'Inactive front panel'}
              title={panel() ? HINT : undefined}
              style={{ '--accent': faceplate().accent, '--func-legend': faceplate().func_legend }}
            >
              <div class="surface">
                <Show
                  when={panel()}
                  fallback={
                    <div inert>
                      <PanelArt panel={faceplate()} />
                    </div>
                  }
                >
                  <LivePanel
                    panel={faceplate()}
                    runtime={() => runtime}
                    onError={() => {} /* key and turn failures are reported by the runtime above (keyError) */}
                    ref={(api) => (live = api)}
                  />
                </Show>
              </div>
            </main>
          )}
        </Show>
      </div>
    </section>
  );
}
