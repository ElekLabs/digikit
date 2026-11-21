// The Remote page (desktop): the front panel of the selected target with its real screen, the keys and the encoders working on the device.
// The screen is the firmware's own Screenshot (stock MidiRpc 0x04), polled; keys and encoders are the commands of digikit/remote (KEY, TURN),
// which work while digikit/remote is loaded at the device. The page speaks the protocol itself in TypeScript over the target of
// target.ts: the emulator's USB capture or a MIDI port. No process is started.
import { Show, createEffect, createSignal, on, onCleanup, onMount, type JSX } from 'solid-js';
import LivePanel, { type LivePanelApi } from '@digi/workbench-core/ui/components/LivePanel.tsx';
import { panels } from '@digi/workbench-core/data/faceplates.ts';
import type { DeviceId } from '@digi/workbench-core/data/device-profiles.ts';
import type { DkSession, RemoteState } from '@digi/dk-session';
import {
  deviceOfPort,
  infoDue,
  isRemoteMissing,
  keyErrorLine,
  leaveCancels,
  remoteListed,
  remoteNotice,
  remotePaused,
  remoteRuntime,
  remoteReleaseDue,
  remoteStateWanted,
  stateProbeKey,
} from './remote-core';
import { windowShown } from './log-follow-core';
import type { Host } from '@digi/workbench-core/host/host.ts';
import SidePanels from './SidePanels';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { sameTarget, targetLabel } from '@digi/workbench-core/transfer/target.ts';
import { failureLine } from '@digi/workbench-core/transfer/wait-core.ts';
import './remote-page.css';

const SCREEN_MS = { emulator: 250, midi: 400 };
const STATE_MS = 2000;
/** How long the window may stay hidden before the loops look again. */
const HIDDEN_MS = 500;
// Keys and knobs are LivePanel's (the knobs turn by drag and wheel), so the page has no extra buttons for them.
const HINT = 'Click keys; drag or scroll knobs. Shift: ×10.';

export default function RemotePage(props: {
  fallbackDevice: DeviceId;
  emulatorLoaded: boolean;
  host?: Host;
}): JSX.Element {
  const app = useTransferApp();
  const { asked, noteReply } = app.connection;
  const { cancelPending, checkRuntime, reconnect, session, target } = app.target;
  const { busy } = app.state;
  const [state, setState] = createSignal<RemoteState>();
  const [stateError, setStateError] = createSignal<unknown>();
  const [screenError, setScreenError] = createSignal('');
  // a failed key or turn: it stays until a key succeeds or the target changes (the screen poll does not clear it)
  const [keyError, setKeyError] = createSignal('');
  // whether MODULES lists digikit/remote, read when the state request failed (undefined: not read)
  const [listed, setListed] = createSignal<boolean>();
  // oxlint-disable-next-line solid/reactivity -- the emulator's device is only the first guess; the target's INFO names the device
  const [device, setDevice] = createSignal<DeviceId>(props.fallbackDevice);
  let live: LivePanelApi | undefined;
  let alive = true;
  const panel = () => panels[device()];
  const runtime = remoteRuntime(() => session(), {
    ok: () => setKeyError(''),
    failed: (e) => setKeyError(keyErrorLine(e, targetLabel(target()))),
  });
  createEffect(
    on(
      () => targetLabel(target()),
      () => setKeyError(''),
      { defer: true },
    ),
  );
  const notice = () => remoteNotice(state(), stateError(), listed());
  const noEmulator = () => target().kind === 'emulator' && !props.emulatorLoaded;

  /** Screen loop: one Screenshot after the other; an error shows and the loop tries again slowly. */
  const screenLoop = async () => {
    while (alive) {
      // nothing is asked of the device while the window is hidden or a Transfer job uses the session
      if (!windowShown() || remotePaused(busy()) || noEmulator()) {
        await new Promise((r) => setTimeout(r, HIDDEN_MS));
        continue;
      }
      const t = target();
      try {
        const s: DkSession = await session();
        const shot = await s.screen();
        live?.showFrame(shot);
        setScreenError('');
        await new Promise((r) => setTimeout(r, SCREEN_MS[t.kind === 'midi' ? 'midi' : 'emulator']));
      } catch (e) {
        setScreenError(failureLine(e, targetLabel(target())));
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
      if (!windowShown() || remotePaused(busy()) || noEmulator()) {
        await new Promise((r) => setTimeout(r, HIDDEN_MS));
        continue;
      }
      const t = target();
      let loaded: Awaited<ReturnType<DkSession['modules']>> | undefined;
      try {
        await asked(async () => checkRuntime(target()));
        const s = await session();
        const key = targetLabel(target());
        if (infoDue(infoAt, Date.now(), key !== infoKey)) {
          const info = await s.info(true);
          infoKey = key;
          infoAt = Date.now();
          const d = deviceOfPort(info.port);
          if (d && d !== device() && Object.hasOwn(panels, d)) setDevice(d);
        }
        loaded = missing ? await s.modules().catch(() => undefined) : undefined;
        const probe = stateProbeKey(key, loaded);
        if (remoteStateWanted(missing, loaded, unavailableKey, probe)) {
          setState(await s.remoteState());
          setStateError(undefined);
          setListed(undefined);
          missing = false;
          unavailableKey = undefined;
          void noteReply(s, () => sameTarget(t, target()));
        } else if (loaded) setListed(remoteListed(loaded));
      } catch (e) {
        missing = isRemoteMissing(e);
        infoAt = undefined;
        setState(undefined);
        setStateError(e);
        // the module may be loaded without the optional state request: MODULES decides what the notice says
        if (missing) {
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
  onMount(() => {
    void screenLoop();
    void stateLoop();
  });
  onCleanup(() => {
    alive = false;
    // a poll that waits for a silent device must not hold up the next page's requests (they take turns on the session); a Transfer job's
    // requests are not ours to cancel
    if (leaveCancels(busy())) cancelPending();
    live?.releaseAll();
    // let go of the keys the page pressed (the module remembers them)
    if (remoteReleaseDue(state(), listed()))
      void session()
        .then((s) => s.remoteRelease())
        .catch(() => {});
  });

  return (
    <section class="remote-page" aria-label="Remote">
      <header class="head">
        <h2>Remote</h2>
        <span class="meta">
          {targetLabel(target())} · {device()}
        </span>
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
      </header>
      <Show when={noEmulator()}>
        <p class="remote-empty">
          No firmware is loaded in the emulator. Open one on the Emulator page, or choose a MIDI device as the target.
        </p>
      </Show>
      {/* the body stays mounted while hidden, so the side panels keep their state (the loops above ask nothing while noEmulator);
          display: contents keeps the page's flex layout as if the wrapper were not there */}
      <div style={{ display: noEmulator() ? 'none' : 'contents' }}>
        <Show when={remotePaused(busy())}>
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
        {/* the same markup as the Emulator page's faceplate (Console.tsx), so the screen has the same place and scale; the side column with the Memory panel is beside it */}
        <div class="remote-row">
          <main
            class="stage"
            aria-label={`${panel().name} front panel`}
            title={HINT}
            style={{ '--accent': panel().accent, '--func-legend': panel().func_legend }}
          >
            <div class="surface">
              <LivePanel
                panel={panel()}
                runtime={() => runtime}
                onError={() => {} /* key and turn failures are reported by the runtime above (keyError) */}
                ref={(api) => (live = api)}
              />
            </div>
          </main>
          <SidePanels
            host={props.host}
            visible={!noEmulator()}
            session={session}
            where={() => targetLabel(target())}
            track={target}
          />
        </div>
      </div>
    </section>
  );
}
