// Per-target card sessions and faceplate controls for the browser Workbench.
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { createSessionHub } from '@digi/workbench-core/transfer/session-core.ts';
import { deviceOfPort, deviceProfiles } from '@digi/workbench-core/data/device-profiles.ts';
import { previewEmulatorDevice } from './card-mock';
import { DkError, DkSession, RPC_SOFTWARE_VERSION, RPC_PING } from '@digi/dk-session';
import type { PreviewLibrary } from './module-library';
import { createPreviewStores } from './module-store';
import { sim } from './sim';
import { createPreviewScreen } from './preview-screen';

export function previewSessions(library: PreviewLibrary, emulatorReady: () => boolean) {
  const storeSession = createPreviewStores(library);
  const screens = new Map<string, ReturnType<typeof createPreviewScreen>>();
  const query = new URLSearchParams(location.search);
  const unidentified = query.has('unidentified');
  const history = query.has('history');
  const began = Date.now();
  return createSessionHub((transport, opts, key) => {
    const session = new DkSession(transport, { timeoutMs: 6000, ...opts });
    const profile = deviceProfiles[deviceOfPort(key) ?? previewEmulatorDevice(query)];
    const screen = screens.get(key) ?? createPreviewScreen(profile.short);
    screens.set(key, screen);
    const midi = key.startsWith('midi:');
    const version = profile.firmware.at(-1)?.version ?? '';
    const store = storeSession(session, profile, key, unidentified || (!midi && query.has('stock')));
    const wireInfo = session.info.bind(session);
    const wireRpc = session.rpc.bind(session);
    return Object.assign(session, {
      ...store,
      async info(quick: Parameters<DkSession['info']>[0]) {
        if (!midi && !emulatorReady()) throw new DkError({ error: 'No runtime is running.', kind: 'timeout' });
        if (!midi || unidentified || sim.dev === 'noreply' || sim.dev === 'error') return store.info(quick);
        const { runtime, runtime_version, port, image_id } = await wireInfo(quick);
        const card = await store.info(quick);
        return { ...card, runtime, runtime_version, port, image_id };
      },
      async rpc(...args: Parameters<DkSession['rpc']>) {
        const [type] = args;
        if (sim.dev === 'noreply') throw new DkError({ error: 'No reply from the device.', kind: 'timeout' });
        if (type === RPC_PING || type === RPC_SOFTWARE_VERSION) {
          if (unidentified || query.has('runtimeOnly')) throw new Error('Unknown MIDI endpoint');
          if (!midi)
            return type === RPC_PING
              ? Uint8Array.of(0, ...new TextEncoder().encode(profile.name), 0)
              : new TextEncoder().encode(version);
        }
        return wireRpc(...args);
      },
      screen: () => screen.read(),
      logPage: async () => {
        const tick = history ? Math.floor((Date.now() - began) / 5000) : 0;
        const seq = 1 + tick;
        const cpu = [213, 350, 290, 180][tick % 4]!;
        const dsp = [345, 730, 510, 290][tick % 4]!;
        return [
          seq,
          seq,
          [
            {
              seq,
              time: seq * 15000,
              kind: LOG_KIND.HEALTH,
              flags: 0,
              args: [(6 << 16) | cpu, dsp, Math.max(dsp, 420), 0, 0],
            },
          ],
        ];
      },
      remoteState: async () => ({ enabled: true, format: 1, turnLimit: 10 }),
      remoteKey: async (code: number, down: boolean) => screen.key(code, down),
      remoteTurn: async (encoder: number, delta: number) => screen.turn(encoder, delta),
      remoteRelease: async () => screen.release(),
    });
  });
}
