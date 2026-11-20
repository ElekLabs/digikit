import { DkSession, EmuUsbTransport, RuntimeEvents } from '@digi/dk-session';
import type { Clock } from '@digi/dk-session';
import type { Session } from '@digi/test-support';

/**
 * A protocol session over a daemon that the rig keeps paused (a card rig, `CardRig.load`): each exchange steps the emulator, so a wait on a reply, or on log events,
 * advances emulated time. The `dk` of a card rig's runtime belongs to its baseline boot and gets no reply; `emulatorDk` is for a daemon that runs free.
 */
export function pausedDk(session: Pick<Session, 'client'>): DkSession {
  return new DkSession(
    new EmuUsbTransport((method, params) => session.client.call(method, params), { stepInstructions: 400_000 }),
  );
}

/**
 * The runtime log events of a card rig's boot, from now on. A card has no debug module, which serves the log configuration commands, so the events are the ones the runtime
 * keeps without it: those at error level (the fixtures and core/ui log at that level) read from the log ring.
 */
export async function pausedEvents(session: Pick<Session, 'client'>, clock: Clock): Promise<RuntimeEvents> {
  const dk = pausedDk(session);
  const noConfig = {
    logPage: dk.logPage.bind(dk),
    logConfig: async () => ({ level: 0, modules: [] as number[] }),
    setLogConfig: async () => ({ level: 0, modules: [] as number[] }),
    modules: async () => [],
  };
  const events = new RuntimeEvents(noConfig as never, clock, {});
  await events.listen();
  return events;
}
